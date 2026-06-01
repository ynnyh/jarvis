use crate::channels::types::{AgentReply, ChannelMessage, PendingAction};
use crate::tools;
use serde_json::json;
use std::collections::HashSet;

use super::session;

pub(super) async fn maybe_handle_confirmation(
    incoming: &ChannelMessage,
) -> Result<Option<AgentReply>, String> {
    let text = incoming.text.trim();
    if !matches!(text, "确认" | "取消" | "confirm" | "cancel") {
        return Ok(None);
    }
    let path = session::pending_path(incoming);
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

pub(super) fn save_pending_action(action: &PendingAction) -> Result<(), String> {
    let path = crate::settings::jarvis_dir().join("channel-pending").join(format!(
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

pub(super) async fn detect_effort_proposal(
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
    let Some(task_id) = task_id else {
        return Ok(None);
    };
    let Some(hours) = hours else { return Ok(None) };
    if hours <= 0.0 {
        return Ok(None);
    }

    let work = cleanup_effort_work(&incoming.text, &task_id);
    if work.is_empty() {
        return Ok(None);
    }

    let id = session::session_id(incoming);
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
        "帮我", "帮忙", "写入", "写", "禅道", "任务", "工时", "小时", "hour", "hours", "确认",
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
