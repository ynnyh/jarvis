pub mod effort_shortcuts;
pub mod message_handler;
pub mod pending_actions;
pub mod reminders;
pub mod session;

#[cfg(test)]
mod tests;

use crate::channels::types::{AgentReply, ChannelMessage, ChannelsConfig};
use crate::settings;
use std::sync::{Arc, Mutex};
use tauri::Manager;
use tokio::sync::{mpsc, watch};

use message_handler::handle_incoming;

const MAX_HISTORY_MESSAGES: usize = 20;

type Sender = mpsc::Sender<OutboundMessage>;
type Receiver = mpsc::Receiver<OutboundMessage>;

#[derive(Debug, Clone)]
pub struct OutboundMessage {
    pub channel: String,
    pub chat_id: String,
    pub text: String,
}

#[derive(Clone)]
pub struct GatewayContext {
    pub outbound: Sender,
}

#[derive(Debug, Clone)]
pub struct NotificationTarget {
    pub channel: String,
    pub chat_id: String,
}

pub async fn run_gateway(
    app: tauri::AppHandle,
    status: Arc<Mutex<crate::channels::ChannelServiceStatus>>,
    mut stop_rx: watch::Receiver<bool>,
) {
    let cfg = load_channels_config();
    let enabled = enabled_names(&cfg);
    if enabled.is_empty() {
        set_status(&status, false, "没有启用任何聊天渠道");
        return;
    }

    set_status(
        &status,
        true,
        format!("渠道服务运行中: {}", enabled.join(", ")),
    );
    let (in_tx, mut in_rx) = mpsc::channel::<ChannelMessage>(64);
    let (out_tx, out_rx) = mpsc::channel::<OutboundMessage>(64);
    let ctx = GatewayContext { outbound: out_tx };
    if let Some(state) = app.try_state::<crate::channels::ChannelServiceState>() {
        if let Ok(mut slot) = state.outbound_tx.lock() {
            *slot = Some(ctx.outbound.clone());
        }
    }
    let (telegram_out_tx, telegram_out_rx) = mpsc::channel::<OutboundMessage>(64);
    let (qq_out_tx, qq_out_rx) = mpsc::channel::<OutboundMessage>(64);

    tauri::async_runtime::spawn(outbound_dispatcher(out_rx, telegram_out_tx, qq_out_tx));

    if cfg.telegram.enabled {
        crate::channels::telegram::spawn(
            cfg.telegram.clone(),
            in_tx.clone(),
            telegram_out_rx,
            stop_rx.clone(),
        );
    } else {
        drop(telegram_out_rx);
    }
    if cfg.qqbot.enabled {
        crate::channels::qqbot::spawn(cfg.qqbot.clone(), in_tx.clone(), qq_out_rx, stop_rx.clone());
    } else {
        drop(qq_out_rx);
    }
    drop(in_tx);

    loop {
        tokio::select! {
            _ = stop_rx.changed() => {
                break;
            }
            incoming = in_rx.recv() => {
                let Some(incoming) = incoming else { break };
                set_status(
                    &status,
                    true,
                    format!("收到 {} 消息，正在处理", incoming.channel),
                );
                let ctx = ctx.clone();
                let app = app.clone();
                let status = status.clone();
                tauri::async_runtime::spawn(async move {
                    let reply = match handle_incoming(app, incoming.clone()).await {
                        Ok(r) => r,
                        Err(e) => AgentReply {
                            text: format!("处理失败: {}", e),
                        },
                    };
                    let _ = ctx.outbound.send(OutboundMessage {
                        channel: incoming.channel,
                        chat_id: incoming.chat_id,
                        text: reply.text,
                    }).await;
                    set_status(&status, true, "已处理最近一条渠道消息");
                });
            }
        }
    }
    if let Some(state) = app.try_state::<crate::channels::ChannelServiceState>() {
        if let Ok(mut slot) = state.outbound_tx.lock() {
            *slot = None;
        }
    }
    set_status(&status, false, "渠道服务已停止");
}

async fn outbound_dispatcher(mut out_rx: Receiver, telegram_tx: Sender, qq_tx: Sender) {
    while let Some(msg) = out_rx.recv().await {
        match msg.channel.as_str() {
            "telegram" => {
                let _ = telegram_tx.send(msg).await;
            }
            "qqbot" => {
                let _ = qq_tx.send(msg).await;
            }
            _ => {}
        }
    }
}

pub fn enabled_names(cfg: &ChannelsConfig) -> Vec<&'static str> {
    let mut out = Vec::new();
    if cfg.telegram.enabled {
        out.push("Telegram");
    }
    if cfg.qqbot.enabled {
        out.push("QQ Bot");
    }
    out
}

pub fn notification_targets(cfg: &ChannelsConfig) -> Vec<NotificationTarget> {
    let mut out = Vec::new();
    if cfg.telegram.enabled {
        let ids = if cfg.telegram.notify_chat_ids.is_empty() {
            &cfg.telegram.allow_chat_ids
        } else {
            &cfg.telegram.notify_chat_ids
        };
        for id in ids {
            let id = id.trim();
            if !id.is_empty() {
                out.push(NotificationTarget {
                    channel: "telegram".to_string(),
                    chat_id: id.to_string(),
                });
            }
        }
    }
    if cfg.qqbot.enabled {
        let user_ids = if cfg.qqbot.notify_user_ids.is_empty() {
            &cfg.qqbot.allow_user_ids
        } else {
            &cfg.qqbot.notify_user_ids
        };
        for id in user_ids {
            let id = id.trim();
            if !id.is_empty() {
                out.push(NotificationTarget {
                    channel: "qqbot".to_string(),
                    chat_id: format!("c2c:{}", id),
                });
            }
        }
        let group_ids = if cfg.qqbot.notify_group_ids.is_empty() {
            &cfg.qqbot.allow_group_ids
        } else {
            &cfg.qqbot.notify_group_ids
        };
        for id in group_ids {
            let id = id.trim();
            if !id.is_empty() {
                out.push(NotificationTarget {
                    channel: "qqbot".to_string(),
                    chat_id: format!("group:{}", id),
                });
            }
        }
    }
    out
}

fn set_status(
    status: &Arc<Mutex<crate::channels::ChannelServiceStatus>>,
    running: bool,
    message: impl Into<String>,
) {
    if let Ok(mut s) = status.lock() {
        s.running = running;
        s.message = message.into();
    }
}

pub fn load_channels_config() -> ChannelsConfig {
    let cfg = settings::load_raw_config();
    let mut channels: ChannelsConfig = cfg
        .as_ref()
        .and_then(|v| v.get("channels").cloned())
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();
    if channels.telegram.bot_token.trim() == settings::SECRET_PLACEHOLDER {
        channels.telegram.bot_token.clear();
    }
    if channels.qqbot.app_secret.trim() == settings::SECRET_PLACEHOLDER {
        channels.qqbot.app_secret.clear();
    }
    if channels.telegram.bot_token.trim().is_empty() {
        if let Some(v) = settings::secret_get("channels.telegram.botToken") {
            channels.telegram.bot_token = v;
        }
    }
    if channels.qqbot.app_secret.trim().is_empty() {
        if let Some(v) = settings::secret_get("channels.qqbot.appSecret") {
            channels.qqbot.app_secret = v;
        }
    }
    if channels.telegram.bot_token.trim().is_empty() {
        if let Ok(v) = std::env::var("TELEGRAM_BOT_TOKEN") {
            channels.telegram.bot_token = v;
        }
    }
    if channels.qqbot.app_id.trim().is_empty() {
        if let Ok(v) = std::env::var("QQBOT_APP_ID") {
            channels.qqbot.app_id = v;
        }
    }
    if channels.qqbot.app_secret.trim().is_empty() {
        if let Ok(v) = std::env::var("QQBOT_APP_SECRET") {
            channels.qqbot.app_secret = v;
        }
    }
    channels
}
