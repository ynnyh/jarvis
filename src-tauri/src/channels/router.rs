use crate::channels::types::{AgentReply, ChannelMessage, ChannelsConfig, PendingAction};
use crate::settings;
use crate::tools;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};
use tokio::sync::{mpsc, watch};

const MAX_HISTORY_MESSAGES: usize = 20;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledReminder {
    pub id: String,
    pub cron: String,
    pub message: String,
    pub enabled: bool,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
}

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

    set_status(&status, true, format!("渠道服务运行中: {}", enabled.join(", ")));
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
    if let Some(state) = app.try_state::<crate::channels::ChannelServiceState>() {
        if let Ok(mut slot) = state.outbound_tx.lock() {
            *slot = None;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notification_targets_prefer_explicit_notify_ids() {
        let mut cfg = ChannelsConfig::default();
        cfg.telegram.enabled = true;
        cfg.telegram.allow_chat_ids = vec!["allow-tg".to_string()];
        cfg.telegram.notify_chat_ids = vec!["notify-tg".to_string()];
        cfg.qqbot.enabled = true;
        cfg.qqbot.allow_user_ids = vec!["allow-user".to_string()];
        cfg.qqbot.allow_group_ids = vec!["allow-group".to_string()];
        cfg.qqbot.notify_user_ids = vec!["notify-user".to_string()];
        cfg.qqbot.notify_group_ids = vec!["notify-group".to_string()];

        let targets = notification_targets(&cfg);
        let pairs: Vec<_> = targets
            .iter()
            .map(|t| (t.channel.as_str(), t.chat_id.as_str()))
            .collect();

        assert_eq!(
            pairs,
            vec![
                ("telegram", "notify-tg"),
                ("qqbot", "c2c:notify-user"),
                ("qqbot", "group:notify-group"),
            ]
        );
    }

    #[test]
    fn notification_targets_fallback_to_allow_ids_for_old_configs() {
        let mut cfg = ChannelsConfig::default();
        cfg.telegram.enabled = true;
        cfg.telegram.allow_chat_ids = vec!["allow-tg".to_string()];
        cfg.qqbot.enabled = true;
        cfg.qqbot.allow_user_ids = vec!["allow-user".to_string()];
        cfg.qqbot.allow_group_ids = vec!["allow-group".to_string()];

        let targets = notification_targets(&cfg);
        let pairs: Vec<_> = targets
            .iter()
            .map(|t| (t.channel.as_str(), t.chat_id.as_str()))
            .collect();

        assert_eq!(
            pairs,
            vec![
                ("telegram", "allow-tg"),
                ("qqbot", "c2c:allow-user"),
                ("qqbot", "group:allow-group"),
            ]
        );
    }
}

async fn handle_incoming(app: tauri::AppHandle, incoming: ChannelMessage) -> Result<AgentReply, String> {
    // 定时提醒命令优先处理
    if let Some(reply) = maybe_handle_reminder_command(&app, &incoming.text) {
        append_channel_messages(&incoming, &reply.text, None)?;
        return Ok(reply);
    }

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

    if !should_use_agent_tools(&incoming.text) {
        let reply = handle_plain_chat(history, assistant_name, user_title).await?;
        append_channel_messages(&incoming, &reply.text, None)?;
        return Ok(reply);
    }

    if let Some(reply) = maybe_handle_effort_query(&incoming.text).await? {
        append_channel_messages(&incoming, &reply.text, None)?;
        return Ok(reply);
    }

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

async fn handle_plain_chat(
    history: Vec<Value>,
    assistant_name: &str,
    user_title: &str,
) -> Result<AgentReply, String> {
    let mut messages = vec![json!({
        "role": "system",
        "content": format!(
            "你是 {}，正在通过 QQ/Telegram 和{}聊天。请用中文自然、简洁地回答。遇到禅道任务、工时、风险、日报、项目进展等工作请求时，可以提示用户直接说明具体需求。",
            assistant_name, user_title
        ),
    })];

    for msg in history {
        let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("");
        if role == "tool" || msg.get("tool_calls").is_some() {
            continue;
        }
        let content = msg.get("content").and_then(|v| v.as_str()).unwrap_or("").trim();
        if content.is_empty() {
            continue;
        }
        messages.push(json!({
            "role": role,
            "content": content,
        }));
    }

    let response = tools::dispatch(
        "ask-llm",
        json!({
            "messages": messages,
            "temperature": 0.4,
            "maxTokens": 800,
        }),
    )
    .await?;

    let text = response
        .get("text")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "我在，但这次没有生成可展示的回复。".to_string());

    Ok(AgentReply { text })
}

fn should_use_agent_tools(text: &str) -> bool {
    let lower = text.to_lowercase();
    [
        "禅道",
        "任务",
        "工时",
        "耗时",
        "小时",
        "风险",
        "延期",
        "逾期",
        "复盘",
        "日报",
        "周报",
        "项目",
        "进展",
        "本周",
        "今天",
        "昨天",
        "明天",
        "写入",
        "记录",
        "提交",
        "commit",
        "bug",
        "需求",
    ]
    .iter()
    .any(|keyword| lower.contains(keyword))
}

async fn maybe_handle_effort_query(text: &str) -> Result<Option<AgentReply>, String> {
    if !is_effort_query(text) {
        return Ok(None);
    }

    let (range, label) = effort_query_range(text);
    let response = tools::dispatch(
        "get_efforts",
        json!({
            "range": range,
        }),
    )
    .await?;

    Ok(Some(AgentReply {
        text: format_effort_reply(&label, &response),
    }))
}

fn is_effort_query(text: &str) -> bool {
    let lower = text.to_lowercase();
    let has_effort_word = ["工时", "耗时", "小时", "effort"].iter().any(|w| lower.contains(w));
    if !has_effort_word {
        return false;
    }
    if ["写", "写入", "记录", "新增", "填", "补", "提交"].iter().any(|w| lower.contains(w)) {
        return false;
    }
    ["查", "查询", "看", "统计", "汇总", "多少", "明细", "本周", "今天", "昨天", "本月", "今年"]
        .iter()
        .any(|w| lower.contains(w))
}

fn effort_query_range(text: &str) -> (&'static str, String) {
    let lower = text.to_lowercase();
    if lower.contains("昨天") || lower.contains("昨日") {
        ("yesterday", "昨天".to_string())
    } else if lower.contains("今天") || lower.contains("今日") {
        ("today", "今天".to_string())
    } else if lower.contains("本月") || lower.contains("这个月") {
        ("thisMonth", "本月".to_string())
    } else if lower.contains("今年") || lower.contains("本年") {
        ("thisYear", "今年".to_string())
    } else {
        ("thisWeek", "本周".to_string())
    }
}

fn format_effort_reply(label: &str, response: &Value) -> String {
    let count = response.get("count").and_then(|v| v.as_u64()).unwrap_or(0);
    let total_hours = response
        .get("totalHours")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let records = response
        .get("records")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    if count == 0 || records.is_empty() {
        return format!("{}没有查到工时记录。", label);
    }

    let begin = response.get("begin").and_then(|v| v.as_str()).unwrap_or("");
    let end = response.get("end").and_then(|v| v.as_str()).unwrap_or("");
    let mut lines = vec![format!(
        "{}工时（{} ~ {}）：共 {} 条，合计 {:.1} 小时。",
        label, begin, end, count, total_hours
    )];
    for record in records.iter().take(8) {
        let date = record.get("date").and_then(|v| v.as_str()).unwrap_or("");
        let hours = record
            .get("itemHours")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let task = record
            .get("taskName")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .unwrap_or("未命名任务");
        let work = record
            .get("workContent")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        if work.is_empty() {
            lines.push(format!("- {} {:.1}h {}", date, hours, task));
        } else {
            lines.push(format!("- {} {:.1}h {}：{}", date, hours, task, work));
        }
    }
    if records.len() > 8 {
        lines.push(format!("还有 {} 条明细没有展开。", records.len() - 8));
    }
    lines.join("\n")
}

fn allowed_channel_tools() -> Vec<&'static str> {
    vec![
        "get_tasks",
        "get_today_tasks",
        "get_task_detail",
        "get_task_commits",
        "analyze_risk",
        "get_daily_review",
        "get_efforts",
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

// ===== 定时提醒命令 =====

/**
 * 支持的命令格式：
 *   定时 HH:MM 提醒内容        → 每天 HH:MM 触发
 *   定时 分 时 日 月 周 提醒内容 → 标准 cron 表达式
 *   定时列表                   → 列出所有提醒
 *   删除定时 N                 → 删除第 N 个提醒
 */
fn maybe_handle_reminder_command(app: &tauri::AppHandle, text: &str) -> Option<AgentReply> {
    let trimmed = text.trim();

    // 列表
    if trimmed == "定时列表" || trimmed == "提醒列表" || trimmed == "我的定时" {
        return Some(list_reminders());
    }

    // 删除
    if let Some(idx) = try_parse_delete_reminder(trimmed) {
        let reply = delete_reminder(idx);
        let _ = app.emit("reminders-changed", ());
        return Some(reply);
    }

    // 添加
    if trimmed.starts_with("定时") || trimmed.starts_with("添加定时") || trimmed.starts_with("添加提醒") {
        let reply = add_reminder(trimmed);
        let _ = app.emit("reminders-changed", ());
        return Some(reply);
    }

    None
}

fn try_parse_delete_reminder(text: &str) -> Option<usize> {
    let patterns = ["删除定时 ", "删除提醒 ", "取消定时 ", "取消提醒 "];
    for pat in patterns {
        if let Some(rest) = text.strip_prefix(pat) {
            if let Ok(n) = rest.trim().parse::<usize>() {
                if n > 0 { return Some(n - 1); }
            }
        }
    }
    None
}

fn load_reminders() -> Vec<ScheduledReminder> {
    let cfg = settings::load_raw_config().unwrap_or_else(|| json!({}));
    cfg.get("reminders")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

fn save_reminders(reminders: &[ScheduledReminder]) {
    let mut cfg = settings::load_raw_config().unwrap_or_else(|| json!({}));
    cfg["reminders"] = serde_json::to_value(reminders).unwrap_or(json!([]));
    let path = settings::config_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(content) = serde_json::to_string_pretty(&cfg) {
        let _ = std::fs::write(&path, content);
    }
}

fn add_reminder(text: &str) -> AgentReply {
    // 去掉前缀
    let content = text
        .trim_start_matches("添加定时")
        .trim_start_matches("添加提醒")
        .trim_start_matches("定时")
        .trim();

    let (cron, message) = parse_reminder_input(content);
    if message.is_empty() {
        return AgentReply {
            text: "格式不对。用法：\n定时 17:30 写日报\n定时 30 8 * * 1-5 晨会\n定时列表\n删除定时 1".to_string(),
        };
    }

    let reminder = ScheduledReminder {
        id: format!("r{}", chrono::Utc::now().timestamp_millis()),
        cron: cron.clone(),
        message: message.to_string(),
        enabled: true,
        created_at: chrono::Utc::now().timestamp_millis(),
    };

    let mut reminders = load_reminders();
    reminders.push(reminder);
    save_reminders(&reminders);

    AgentReply {
        text: format!("已添加定时提醒：\nCron: {}\n内容: {}\n\n用「定时列表」查看所有提醒，「删除定时 N」删除。", cron, message),
    }
}

fn list_reminders() -> AgentReply {
    let reminders = load_reminders();
    if reminders.is_empty() {
        return AgentReply {
            text: "当前没有定时提醒。发送「定时 17:30 写日报」来添加。".to_string(),
        };
    }

    let mut lines = vec!["📋 定时提醒列表：".to_string()];
    for (i, r) in reminders.iter().enumerate() {
        let status = if r.enabled { "✅" } else { "⏸" };
        lines.push(format!("{}. {} {} — {}", i + 1, status, r.cron, r.message));
    }
    lines.push("\n发送「删除定时 N」删除指定提醒".to_string());

    AgentReply { text: lines.join("\n") }
}

fn delete_reminder(index: usize) -> AgentReply {
    let mut reminders = load_reminders();
    if index >= reminders.len() {
        return AgentReply {
            text: format!("没有第 {} 个提醒，当前共 {} 个。", index + 1, reminders.len()),
        };
    }
    let removed = reminders.remove(index);
    save_reminders(&reminders);
    AgentReply {
        text: format!("已删除定时提醒：{} — {}", removed.cron, removed.message),
    }
}

/**
 * 解析用户输入为 (cron, message)。
 *   "17:30 写日报"     → ("30 17 * * *", "写日报")
 *   "30 8 * * 1-5 晨会" → ("30 8 * * 1-5", "晨会")
 */
fn parse_reminder_input(input: &str) -> (String, String) {
    // 尝试匹配标准 cron 格式：5 个数字段 + 消息
    let re = regex::Regex::new(
        r"^(\S+)\s+(\S+)\s+(\S+)\s+(\S+)\s+(\S+)\s+(.+)$"
    ).ok();

    if let Some(re) = &re {
        if let Some(caps) = re.captures(input) {
            let fields: Vec<&str> = (1..=5).filter_map(|i| caps.get(i).map(|m| m.as_str())).collect();
            if fields.len() == 5 {
                // 验证是否都是合法的 cron 字段
                let all_valid = fields.iter().all(|f| {
                    f.chars().all(|c| c.is_ascii_digit() || c == '*' || c == '-' || c == ',' || c == '/')
                });
                if all_valid {
                    let msg = caps.get(6).map(|m| m.as_str().trim()).unwrap_or("");
                    if !msg.is_empty() {
                        return (fields.join(" "), msg.to_string());
                    }
                }
            }
        }
    }

    // 尝试匹配 HH:MM 格式
    let time_re = regex::Regex::new(
        r"^([0-9]{1,2}):([0-9]{2})\s+(.+)$"
    ).ok();

    if let Some(re) = &time_re {
        if let Some(caps) = re.captures(input) {
            let hour: u32 = caps.get(1).and_then(|m| m.as_str().parse().ok()).unwrap_or(0);
            let minute: u32 = caps.get(2).and_then(|m| m.as_str().parse().ok()).unwrap_or(0);
            let msg = caps.get(3).map(|m| m.as_str().trim()).unwrap_or("");
            if !msg.is_empty() && hour < 24 && minute < 60 {
                return (
                    format!("{} {} * * *", minute, hour),
                    msg.to_string(),
                );
            }
        }
    }

    (String::new(), String::new())
}
