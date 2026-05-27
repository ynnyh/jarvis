use crate::channels::types::{AgentReply, ChannelMessage, ChannelsConfig, PendingAction};
use crate::settings;
use crate::tools;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, watch};

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

    set_status(&status, true, format!("渠道服务运行中: {}", enabled.join(", ")));
    let (in_tx, mut in_rx) = mpsc::channel::<ChannelMessage>(64);
    let (out_tx, out_rx) = mpsc::channel::<OutboundMessage>(64);
    let ctx = GatewayContext { outbound: out_tx };
    let (telegram_out_tx, telegram_out_rx) = mpsc::channel::<OutboundMessage>(64);
    let (qq_out_tx, qq_out_rx) = mpsc::channel::<OutboundMessage>(64);

    tauri::async_runtime::spawn(outbound_dispatcher(
        out_rx,
        telegram_out_tx,
        qq_out_tx,
    ));

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
        crate::channels::qqbot::spawn(
            cfg.qqbot.clone(),
            in_tx.clone(),
            qq_out_rx,
            stop_rx.clone(),
        );
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
    set_status(&status, false, "渠道服务已停止");
}

async fn outbound_dispatcher(
    mut out_rx: Receiver,
    telegram_tx: Sender,
    qq_tx: Sender,
) {
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

async fn handle_incoming(app: tauri::AppHandle, incoming: ChannelMessage) -> Result<AgentReply, String> {
    if let Some(reply) = maybe_handle_confirmation(&incoming).await? {
        append_channel_messages(&incoming, &reply.text, None)?;
        return Ok(reply);
    }

    let mut history = load_channel_history(&incoming)?;
    history.push(json!({ "role": "user", "content": incoming.text }));
    trim_history(&mut history);

    let config = settings::load_raw_config().unwrap_or_else(|| json!({}));
    let assistant_name = config
        .get("assistantName")
        .and_then(|v| v.as_str())
        .unwrap_or("Jarvis");
    let user_title = config
        .get("userTitle")
        .and_then(|v| v.as_str())
        .unwrap_or("主人");

    let response = tools::dispatch(
        "chat_send",
        json!({
            "messages": history,
            "assistantName": assistant_name,
            "userTitle": user_title,
            "allowedTools": allowed_channel_tools(),
        }),
    )
    .await?;

    let new_messages = response
        .get("newMessages")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let agent_text = last_assistant_text(&new_messages)
        .unwrap_or_else(|| "我处理完了，但没有生成可展示的回复。".to_string());

    let pending = detect_effort_proposal(&incoming, &agent_text).await?;
    let text = if let Some(action) = &pending {
        save_pending_action(action)?;
        format!(
            "{}\n\n我准备写入禅道：\n{}\n\n回复“确认”后我再执行；回复“取消”则放弃。",
            agent_text, action.summary
        )
    } else {
        agent_text.clone()
    };

    append_channel_messages(&incoming, &agent_text, Some(new_messages))?;

    let _ = app;
    Ok(AgentReply { text })
}

fn allowed_channel_tools() -> Vec<&'static str> {
    vec![
        "get_tasks",
        "get_today_tasks",
        "get_task_detail",
        "get_task_commits",
        "analyze_risk",
        "get_daily_review",
    ]
}

fn last_assistant_text(messages: &[Value]) -> Option<String> {
    messages
        .iter()
        .rev()
        .find(|m| m.get("role").and_then(|v| v.as_str()) == Some("assistant"))
        .and_then(|m| m.get("content").and_then(|v| v.as_str()))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn session_id(msg: &ChannelMessage) -> String {
    let raw = format!("{}-{}", msg.channel, msg.chat_id);
    raw.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

fn channels_dir() -> PathBuf {
    settings::jarvis_dir().join("channel-sessions")
}

fn session_path(msg: &ChannelMessage) -> PathBuf {
    channels_dir().join(format!("{}.json", session_id(msg)))
}

fn pending_path(msg: &ChannelMessage) -> PathBuf {
    settings::jarvis_dir()
        .join("channel-pending")
        .join(format!("{}.json", session_id(msg)))
}

fn load_channel_history(msg: &ChannelMessage) -> Result<Vec<Value>, String> {
    let path = session_path(msg);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = std::fs::read_to_string(path).map_err(|e| format!("读取渠道会话失败: {}", e))?;
    let parsed: Value = serde_json::from_str(&raw).map_err(|e| format!("渠道会话解析失败: {}", e))?;
    Ok(parsed
        .get("messages")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default())
}

fn append_channel_messages(
    incoming: &ChannelMessage,
    assistant_text: &str,
    agent_new_messages: Option<Vec<Value>>,
) -> Result<(), String> {
    let mut history = load_channel_history(incoming)?;
    history.push(json!({ "role": "user", "content": incoming.text }));
    if let Some(messages) = agent_new_messages {
        for msg in messages {
            let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("");
            if role == "tool" {
                continue;
            }
            history.push(msg);
        }
    } else {
        history.push(json!({ "role": "assistant", "content": assistant_text }));
    }
    trim_history(&mut history);
    let path = session_path(incoming);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建渠道会话目录失败: {}", e))?;
    }
    std::fs::write(
        path,
        serde_json::to_string_pretty(&json!({ "messages": history })).map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("保存渠道会话失败: {}", e))
}

fn trim_history(messages: &mut Vec<Value>) {
    if messages.len() > MAX_HISTORY_MESSAGES {
        let drain = messages.len() - MAX_HISTORY_MESSAGES;
        messages.drain(0..drain);
    }
}

async fn maybe_handle_confirmation(incoming: &ChannelMessage) -> Result<Option<AgentReply>, String> {
    let text = incoming.text.trim();
    if !matches!(text, "确认" | "取消" | "confirm" | "cancel") {
        return Ok(None);
    }
    let path = pending_path(incoming);
    if !path.exists() {
        return Ok(Some(AgentReply {
            text: "当前没有待确认的写入操作。".to_string(),
        }));
    }
    let raw = std::fs::read_to_string(&path).map_err(|e| format!("读取待确认操作失败: {}", e))?;
    let action: PendingAction =
        serde_json::from_str(&raw).map_err(|e| format!("待确认操作解析失败: {}", e))?;
    if matches!(text, "取消" | "cancel") {
        let _ = std::fs::remove_file(&path);
        return Ok(Some(AgentReply {
            text: "已取消，这次不会写入禅道。".to_string(),
        }));
    }

    let result = match action.kind.as_str() {
        "log-task-effort" => tools::dispatch("log-task-effort", action.payload.clone()).await,
        _ => Err(format!("未知待确认操作: {}", action.kind)),
    };
    let _ = std::fs::remove_file(&path);
    match result {
        Ok(_) => Ok(Some(AgentReply {
            text: "已写入禅道。".to_string(),
        })),
        Err(e) => Ok(Some(AgentReply {
            text: format!("写入禅道失败: {}", e),
        })),
    }
}

fn save_pending_action(action: &PendingAction) -> Result<(), String> {
    let path = settings::jarvis_dir()
        .join("channel-pending")
        .join(format!(
            "{}.json",
            action
                .id
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
                .collect::<String>()
        ));
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建待确认目录失败: {}", e))?;
    }
    std::fs::write(
        path,
        serde_json::to_string_pretty(action).map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("保存待确认操作失败: {}", e))
}

async fn detect_effort_proposal(
    incoming: &ChannelMessage,
    agent_text: &str,
) -> Result<Option<PendingAction>, String> {
    let combined = format!("{}\n{}", incoming.text, agent_text);
    let lower = combined.to_lowercase();
    let effort_words: HashSet<&str> = ["工时", "耗时", "小时", "h", "hour"].into_iter().collect();
    if !effort_words.iter().any(|w| lower.contains(w)) {
        return Ok(None);
    }

    let task_id = regex_capture(&combined, r"(?:任务|task|#)\s*([0-9]{2,})")
        .or_else(|| regex_capture(&combined, r"\b([0-9]{4,})\b"));
    let hours = regex_capture(&combined, r"([0-9]+(?:\.[0-9]+)?)\s*(?:小时|工时|h|hour)")
        .and_then(|s| s.parse::<f64>().ok());
    let Some(task_id) = task_id else { return Ok(None) };
    let Some(hours) = hours else { return Ok(None) };
    if hours <= 0.0 {
        return Ok(None);
    }

    let work = cleanup_effort_work(&incoming.text, &task_id);
    if work.is_empty() {
        return Ok(None);
    }

    let id = session_id(incoming);
    let summary = format!("任务: {}\n工时: {}h\n内容: {}", task_id, hours, work);
    Ok(Some(PendingAction {
        id,
        channel: incoming.channel.clone(),
        chat_id: incoming.chat_id.clone(),
        kind: "log-task-effort".to_string(),
        payload: json!({
            "taskId": task_id,
            "hours": hours,
            "work": work,
        }),
        summary,
        created_at: chrono::Utc::now().timestamp_millis(),
    }))
}

fn regex_capture(text: &str, pattern: &str) -> Option<String> {
    regex::Regex::new(pattern)
        .ok()?
        .captures(text)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().trim().to_string())
        .filter(|s| !s.is_empty())
}

fn cleanup_effort_work(text: &str, task_id: &str) -> String {
    let mut s = text.replace(task_id, "");
    for pat in [
        "帮我", "帮忙", "写入", "写", "禅道", "任务", "工时", "小时", "hour", "hours",
        "确认",
    ] {
        s = s.replace(pat, "");
    }
    if let Ok(re) = regex::Regex::new(r"[0-9]+(?:\.[0-9]+)?\s*(h|H)?") {
        s = re.replace_all(&s, "").to_string();
    }
    s.trim_matches(|c: char| c.is_whitespace() || "，,。.;；:：-_/".contains(c))
        .trim()
        .to_string()
}
