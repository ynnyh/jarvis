use crate::channels::types::{AgentReply, ChannelMessage, PendingAction};
use crate::settings;
use crate::tools;
use serde_json::json;

use super::effort_shortcuts;
use super::pending_actions;
use super::reminders;
use super::session;

pub(super) async fn handle_incoming(
    app: tauri::AppHandle,
    incoming: ChannelMessage,
) -> Result<AgentReply, String> {
    // 定时提醒命令优先处理
    if let Some(reply) = reminders::maybe_handle_reminder_command(&app, &incoming.text) {
        session::append_channel_messages(&incoming, &reply.text, None)?;
        return Ok(reply);
    }

    if let Some(reply) = pending_actions::maybe_handle_confirmation(&incoming).await? {
        session::append_channel_messages(&incoming, &reply.text, None)?;
        return Ok(reply);
    }

    let mut history = session::load_channel_history(&incoming)?;
    history.push(json!({ "role": "user", "content": incoming.text }));
    session::trim_history(&mut history);

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
        session::append_channel_messages(&incoming, &reply.text, None)?;
        return Ok(reply);
    }

    if let Some(reply) = effort_shortcuts::maybe_handle_effort_query(&incoming.text).await? {
        session::append_channel_messages(&incoming, &reply.text, None)?;
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

    // 优先从工具结果拿写入建议（避免重复走正则兜底）
    let pending = match extract_pending_from_tool_result(&new_messages) {
        Some(action) => Some(action),
        None => pending_actions::detect_effort_proposal(&incoming, &agent_text).await?,
    };
    let text = if let Some(action) = &pending {
        pending_actions::save_pending_action(action)?;
        format!(
            "{}\n\n我准备写入禅道：\n{}\n\n回复\u{201c}确认\u{201d}后我再执行；回复\u{201c}取消\u{201d}则放弃。",
            agent_text, action.summary
        )
    } else {
        agent_text.clone()
    };

    session::append_channel_messages(&incoming, &agent_text, Some(new_messages))?;

    let _ = app;
    Ok(AgentReply { text })
}

pub(super) async fn handle_plain_chat(
    history: Vec<serde_json::Value>,
    assistant_name: &str,
    user_title: &str,
) -> Result<AgentReply, String> {
    // 常见问候直接本地回复，不走 LLM
    let last_user_text = history
        .iter()
        .rev()
        .find(|m| m.get("role").and_then(|v| v.as_str()) == Some("user"))
        .and_then(|m| m.get("content").and_then(|v| v.as_str()))
        .unwrap_or("")
        .trim();
    let trimmed = last_user_text.trim_matches(|c: char| c.is_whitespace() || "，。！？,.!?".contains(c));
    if matches!(trimmed, "在吗" | "在不在" | "你好" | "嗨" | "hello" | "hi") {
        return Ok(AgentReply {
            text: format!("在的，{}！有什么需要帮忙的吗？", user_title),
        });
    }

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
        let content = msg
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
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

pub fn should_use_agent_tools(text: &str) -> bool {
    let lower = text.to_lowercase();
    [
        "禅道", "任务", "工时", "耗时", "小时", "风险", "延期", "逾期", "复盘", "日报", "周报",
        "项目", "成本", "进展", "本周", "写入", "记录", "提交", "commit", "bug",
        "需求",
    ]
    .iter()
    .any(|keyword| lower.contains(keyword))
}

pub(super) fn allowed_channel_tools() -> Vec<&'static str> {
    vec![
        "get_tasks",
        "get_today_tasks",
        "get_task_detail",
        "get_task_commits",
        "analyze_risk",
        "get_daily_review",
        "get_efforts",
        "get_effort_report",
        "cost_report_preview",
        "cost_report",
        "prepare-log-task-effort",
    ]
}

pub(super) fn last_assistant_text(messages: &[serde_json::Value]) -> Option<String> {
    messages
        .iter()
        .rev()
        .find(|m| m.get("role").and_then(|v| v.as_str()) == Some("assistant"))
        .and_then(|m| m.get("content").and_then(|v| v.as_str()))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// 扫 new_messages 中 prepare-log-task-effort 的成功结果，直接提取 PendingAction。
/// 优先于正则兜底，避免 agent 已准备写入建议后又跑一遍 regex。
fn extract_pending_from_tool_result(new_messages: &[serde_json::Value]) -> Option<PendingAction> {
    for msg in new_messages.iter().rev() {
        let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("");
        let name = msg.get("name").and_then(|v| v.as_str()).unwrap_or("");
        if role != "tool" || name != "prepare-log-task-effort" {
            continue;
        }
        let content = match msg.get("content").and_then(|v| v.as_str()) {
            Some(c) => c,
            None => continue,
        };
        let parsed: serde_json::Value = match serde_json::from_str(content) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if parsed.get("pendingWrite").and_then(|v| v.as_bool()) != Some(true) {
            continue;
        }
        let payload = match parsed.get("payload") {
            Some(p) => p,
            None => continue,
        };
        let summary = match parsed.get("summary").and_then(|v| v.as_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };
        let task_id = match payload.get("taskId").and_then(|v| v.as_str()) {
            Some(id) => id,
            None => continue,
        };
        let hours = match payload.get("hours").and_then(|v| v.as_f64()) {
            Some(h) => h,
            None => continue,
        };
        let work_val = match payload.get("work").and_then(|v| v.as_str()) {
            Some(w) => w,
            None => continue,
        };
        let id = format!("tool-prep-{}", task_id);
        return Some(PendingAction {
            id,
            channel: String::new(),
            chat_id: String::new(),
            kind: "log-task-effort".to_string(),
            payload: serde_json::json!({"taskId": task_id, "hours": hours, "work": work_val}),
            summary,
            created_at: chrono::Utc::now().timestamp_millis(),
        });
    }
    None
}
