use crate::channels::types::ChannelMessage;
use crate::settings;
use serde_json::json;
use std::path::PathBuf;

use super::MAX_HISTORY_MESSAGES;

pub(super) fn session_id(msg: &ChannelMessage) -> String {
    let raw = format!("{}-{}", msg.channel, msg.chat_id);
    raw.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

pub(super) fn channels_dir() -> PathBuf {
    settings::jarvis_dir().join("channel-sessions")
}

pub(super) fn session_path(msg: &ChannelMessage) -> PathBuf {
    channels_dir().join(format!("{}.json", session_id(msg)))
}

pub(super) fn pending_path(msg: &ChannelMessage) -> PathBuf {
    settings::jarvis_dir()
        .join("channel-pending")
        .join(format!("{}.json", session_id(msg)))
}

pub(super) fn load_channel_history(msg: &ChannelMessage) -> Result<Vec<serde_json::Value>, String> {
    let path = session_path(msg);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = std::fs::read_to_string(path).map_err(|e| format!("读取渠道会话失败: {}", e))?;
    let parsed: serde_json::Value =
        serde_json::from_str(&raw).map_err(|e| format!("渠道会话解析失败: {}", e))?;
    Ok(parsed
        .get("messages")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default())
}

pub(super) fn append_channel_messages(
    incoming: &ChannelMessage,
    assistant_text: &str,
    agent_new_messages: Option<Vec<serde_json::Value>>,
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

pub(super) fn trim_history(messages: &mut Vec<serde_json::Value>) {
    if messages.len() > MAX_HISTORY_MESSAGES {
        let drain = messages.len() - MAX_HISTORY_MESSAGES;
        messages.drain(0..drain);
    }
}
