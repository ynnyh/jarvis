// 对话持久化层：每个会话一个 JSON 文件，存在 ~/.jarvis/conversations/
//
// 设计取舍：
// - list 只读 meta（id/title/updatedAt/messageCount），不拉 messages，避免侧栏一次加载几十个会话的全部消息
// - load 拉单个完整会话；save 整体覆写（消息追加由前端组装好再调 save）
// - id 用 timestamp+短随机，文件名同 id：既保证唯一也保证按时间排序
// - 文件名不可信场景下走严格白名单校验（防穿越攻击），但这是本机程序，校验仍做一道

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

fn jarvis_dir() -> PathBuf {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_default();
    PathBuf::from(home).join(".jarvis")
}

fn conversations_dir() -> PathBuf {
    jarvis_dir().join("conversations")
}

/// 文件名 = id + ".json"，id 必须满足 ^[A-Za-z0-9._-]+$，长度 1~80。防穿越。
fn validate_id(id: &str) -> Result<(), String> {
    if id.is_empty() || id.len() > 80 {
        return Err("会话 id 长度非法".into());
    }
    if !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.') {
        return Err("会话 id 含非法字符".into());
    }
    if id.starts_with('.') || id.contains("..") {
        return Err("会话 id 不能以点开头或含 '..'".into());
    }
    Ok(())
}

fn conversation_path(id: &str) -> Result<PathBuf, String> {
    validate_id(id)?;
    Ok(conversations_dir().join(format!("{}.json", id)))
}

// ===== 数据结构 =====

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ConversationMeta {
    pub id: String,
    pub title: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub message_count: usize,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Conversation {
    pub id: String,
    pub title: String,
    pub created_at: i64,
    pub updated_at: i64,
    /// 完整消息列表。结构对应 OpenAI chat completions 格式 + 扩展。
    /// 字段层面不强类型，便于以后加 toolCalls/toolCallId/citations 等而不破坏老文件。
    pub messages: Vec<serde_json::Value>,
}

// ===== Tauri 命令 =====

#[tauri::command]
pub fn conversations_list() -> Result<Vec<ConversationMeta>, String> {
    let dir = conversations_dir();
    if !dir.exists() {
        return Ok(vec![]);
    }
    let entries = fs::read_dir(&dir).map_err(|e| format!("读取会话目录失败: {}", e))?;
    let mut metas: Vec<ConversationMeta> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let raw = match fs::read_to_string(&path) {
            Ok(r) => r,
            Err(_) => continue,  // 单文件读失败不阻塞整列表
        };
        let parsed: serde_json::Value = match serde_json::from_str(&raw) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let id = parsed.get("id").and_then(|x| x.as_str()).unwrap_or("").to_string();
        if id.is_empty() {
            continue;
        }
        let title = parsed.get("title").and_then(|x| x.as_str()).unwrap_or("未命名").to_string();
        let created_at = parsed.get("createdAt").and_then(|x| x.as_i64()).unwrap_or(0);
        let updated_at = parsed.get("updatedAt").and_then(|x| x.as_i64()).unwrap_or(created_at);
        let message_count = parsed
            .get("messages")
            .and_then(|x| x.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        metas.push(ConversationMeta { id, title, created_at, updated_at, message_count });
    }
    // 最近更新的排前面
    metas.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(metas)
}

#[tauri::command]
pub fn conversations_load(id: String) -> Result<Conversation, String> {
    let path = conversation_path(&id)?;
    if !path.exists() {
        return Err(format!("会话不存在: {}", id));
    }
    let raw = fs::read_to_string(&path).map_err(|e| format!("读取会话失败: {}", e))?;
    let conv: Conversation = serde_json::from_str(&raw)
        .map_err(|e| format!("会话解析失败: {}", e))?;
    Ok(conv)
}

#[tauri::command]
pub fn conversations_save(conversation: Conversation) -> Result<(), String> {
    validate_id(&conversation.id)?;
    let dir = conversations_dir();
    fs::create_dir_all(&dir).map_err(|e| format!("创建会话目录失败: {}", e))?;
    let path = conversation_path(&conversation.id)?;
    let content = serde_json::to_string_pretty(&conversation)
        .map_err(|e| format!("序列化失败: {}", e))?;
    crate::util::write_atomic(&path, &content).map_err(|e| format!("写入失败: {}", e))?;
    Ok(())
}

#[tauri::command]
pub fn conversations_delete(id: String) -> Result<(), String> {
    let path = conversation_path(&id)?;
    if path.exists() {
        fs::remove_file(&path).map_err(|e| format!("删除失败: {}", e))?;
    }
    Ok(())
}
