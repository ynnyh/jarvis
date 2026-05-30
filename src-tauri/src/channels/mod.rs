pub mod qqbot;
pub mod router;
pub mod telegram;
pub mod types;

use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use tauri::Manager;
use tokio::sync::{mpsc, watch};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChannelServiceStatus {
    pub running: bool,
    pub message: String,
}

pub struct ChannelServiceState {
    pub status: Arc<Mutex<ChannelServiceStatus>>,
    stop_tx: Mutex<Option<watch::Sender<bool>>>,
    pub outbound_tx: Mutex<Option<mpsc::Sender<router::OutboundMessage>>>,
}

impl Default for ChannelServiceState {
    fn default() -> Self {
        Self {
            status: Arc::new(Mutex::new(ChannelServiceStatus {
                running: false,
                message: "未启动".to_string(),
            })),
            stop_tx: Mutex::new(None),
            outbound_tx: Mutex::new(None),
        }
    }
}

impl ChannelServiceState {
    fn set_status(&self, running: bool, message: impl Into<String>) {
        if let Ok(mut status) = self.status.lock() {
            status.running = running;
            status.message = message.into();
        }
    }
}

pub fn should_auto_start() -> bool {
    let cfg = router::load_channels_config();
    cfg.auto_start && !router::enabled_names(&cfg).is_empty()
}

pub fn start_gateway_background(app: tauri::AppHandle) -> Result<ChannelServiceStatus, String> {
    let state = app.state::<ChannelServiceState>();
    {
        let mut slot = state
            .stop_tx
            .lock()
            .map_err(|e| format!("渠道服务锁定失败: {}", e))?;
        if slot.is_some() {
            return channel_status(app.clone());
        }
        let cfg = router::load_channels_config();
        let enabled = router::enabled_names(&cfg);
        if enabled.is_empty() {
            state.set_status(false, "没有启用任何聊天渠道");
            return channel_status(app.clone());
        }
        let (tx, rx) = watch::channel(false);
        *slot = Some(tx);
        state.set_status(true, format!("渠道服务运行中: {}", enabled.join(", ")));

        let status = state.status.clone();
        let app_handle = app.clone();
        tauri::async_runtime::spawn(async move {
            router::run_gateway(app_handle, status.clone(), rx).await;
            if let Ok(mut status) = status.lock() {
                status.running = false;
                if status.message.trim().is_empty() || status.message == "渠道服务启动中" {
                    status.message = "渠道服务已停止".to_string();
                }
            }
        });
    }
    channel_status(app)
}

#[tauri::command]
pub async fn channels_start(app: tauri::AppHandle) -> Result<ChannelServiceStatus, String> {
    start_gateway_background(app)
}

#[tauri::command]
pub fn channels_stop(app: tauri::AppHandle) -> Result<ChannelServiceStatus, String> {
    let state = app.state::<ChannelServiceState>();
    let tx = {
        let mut slot = state
            .stop_tx
            .lock()
            .map_err(|e| format!("渠道服务锁定失败: {}", e))?;
        slot.take()
    };
    if let Some(tx) = tx {
        let _ = tx.send(true);
        state.set_status(false, "正在停止渠道服务");
        if let Ok(mut outbound) = state.outbound_tx.lock() {
            *outbound = None;
        }
    } else {
        state.set_status(false, "渠道服务未启动");
    }
    channel_status(app)
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChannelsNotifyResult {
    pub sent: usize,
    pub skipped: Vec<String>,
}

#[tauri::command]
pub async fn channels_notify(
    app: tauri::AppHandle,
    text: String,
) -> Result<ChannelsNotifyResult, String> {
    if text.trim().is_empty() {
        return Err("通知内容不能为空".to_string());
    }
    let state = app.state::<ChannelServiceState>();
    let sender = state
        .outbound_tx
        .lock()
        .map_err(|e| format!("渠道服务锁定失败: {}", e))?
        .clone()
        .ok_or_else(|| "渠道服务未启动，无法发送机器人提醒".to_string())?;

    let cfg = router::load_channels_config();
    let targets = router::notification_targets(&cfg);
    if targets.is_empty() {
        return Ok(ChannelsNotifyResult {
            sent: 0,
            skipped: vec![
                "没有可用的机器人提醒目标。请启用 Telegram/QQ，并配置白名单。".to_string(),
            ],
        });
    }

    let mut sent = 0usize;
    let mut skipped = Vec::new();
    for target in targets {
        match sender
            .send(router::OutboundMessage {
                channel: target.channel,
                chat_id: target.chat_id,
                text: text.clone(),
            })
            .await
        {
            Ok(_) => sent += 1,
            Err(e) => skipped.push(format!("发送到渠道队列失败: {}", e)),
        }
    }
    Ok(ChannelsNotifyResult { sent, skipped })
}

#[tauri::command]
pub fn channel_status(app: tauri::AppHandle) -> Result<ChannelServiceStatus, String> {
    let state = app.state::<ChannelServiceState>();
    state
        .status
        .lock()
        .map(|s| s.clone())
        .map_err(|e| format!("渠道服务状态读取失败: {}", e))
}

#[tauri::command]
pub async fn telegram_probe(
    bot_token: String,
    api_base_url: Option<String>,
    proxy: Option<String>,
) -> Result<telegram::TelegramProbeResult, String> {
    let token = if bot_token.trim() == crate::settings::SECRET_PLACEHOLDER {
        crate::settings::secret_get("channels.telegram.botToken").unwrap_or_default()
    } else {
        bot_token
    };
    telegram::probe(token, api_base_url, proxy).await
}

#[tauri::command]
pub async fn qqbot_probe(
    app_id: String,
    app_secret: String,
    sandbox: bool,
) -> Result<qqbot::QqBotProbeResult, String> {
    let secret = if app_secret.trim() == crate::settings::SECRET_PLACEHOLDER {
        crate::settings::secret_get("channels.qqbot.appSecret").unwrap_or_default()
    } else {
        app_secret
    };
    qqbot::probe(app_id, secret, sandbox).await
}
