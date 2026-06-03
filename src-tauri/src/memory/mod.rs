// Agent 记忆系统。
//
// 三层架构（参考 Letta/MemGPT）：
//   - Core Memory：常驻 system prompt 的键值对（用户画像、偏好）
//   - Working Memory：近期对话上下文（现有滑动窗口，不在此模块）
//   - Long-term Memory：持久事实，向量 + FTS 混合检索
//
// 存储：SQLite + sqlite-vec + FTS5
// 嵌入：LLM 提供商的 /embeddings API
// 提取：每轮对话后异步 LLM 调用

pub mod db;
pub mod embedding;
pub mod extractor;

use db::MemoryDb;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// 记忆系统全局状态，由 Tauri manage。
/// db 为 None 表示记忆系统不可用（初始化失败），所有操作降级为空。
pub struct MemoryState {
    pub db: Option<Mutex<MemoryDb>>,
}

impl MemoryState {
    pub fn new(db_path: &Path) -> Self {
        db::register_vec_extension();
        match MemoryDb::open(db_path) {
            Ok(db) => Self {
                db: Some(Mutex::new(db)),
            },
            Err(e) => {
                eprintln!("[memory] 记忆数据库打开失败，记忆系统不可用: {}", e);
                Self { db: None }
            }
        }
    }
}

/// 获取默认记忆数据库路径。
pub fn default_db_path() -> PathBuf {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_default();
    PathBuf::from(home).join(".jarvis").join("memory.db")
}

/// 同步读取 Core Memory 区段（不涉及 await，可在持有 MutexGuard 时调用）。
pub fn build_core_prompt(db: &MemoryDb) -> String {
    db.core_as_prompt_section()
}

/// 计算查询文本的嵌入向量（纯 async，不涉及 DB 锁）。
pub async fn compute_query_embedding(query: &str) -> Option<Vec<f32>> {
    if query.is_empty() {
        return None;
    }
    embedding::embed(query).await.ok()
}

/// 用已计算好的嵌入向量检索 Long-term Memory（纯同步，短暂持锁）。
pub fn search_longterm_prompt(db: &MemoryDb, embedding: &[f32], query: &str, max_memories: usize) -> String {
    if db.count_active().unwrap_or(0) == 0 {
        return String::new();
    }

    let hits = db.hybrid_search(embedding, query, max_memories, 0.7);
    if hits.is_empty() {
        return String::new();
    }

    let mut prompt = String::from("## 相关历史记忆\n\n");
    prompt.push_str("以下是你之前记住的关于用户的信息，请参考但不要直接提及\"记忆\"：\n\n");
    for hit in &hits {
        prompt.push_str(&format!("- {}\n", hit.memory.content));
    }
    prompt
}
