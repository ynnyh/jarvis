// 记忆系统数据库层。
//
// SQLite + sqlite-vec + FTS5 三合一存储：
//   - core_memory: 常驻 prompt 的键值对（用户画像、偏好）
//   - memories: 持久事实，带向量索引和全文搜索
//   - memory_vecs: sqlite-vec 虚拟表，384 维 float 向量
//   - memory_fts: FTS5 全文搜索虚拟表
//
// 设计参考 Mem0（合并去重）、Letta（分层架构）、Zep（时序感知）。

use rusqlite::{params, Connection, Result as SqlResult};
use serde::{Deserialize, Serialize};
use std::path::Path;
use zerocopy::IntoBytes;

/// 注册 sqlite-vec 扩展。必须在第一次 Connection::open 之前调用。
pub fn register_vec_extension() {
    unsafe {
        rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute(
            sqlite_vec::sqlite3_vec_init as *const (),
        )));
    }
}

pub struct MemoryDb {
    conn: Connection,
}

// ===== 数据结构 =====

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoreMemory {
    pub key: String,
    pub value: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Memory {
    pub id: i64,
    pub content: String,
    pub category: String,
    pub source_conversation_id: Option<String>,
    pub importance: f32,
    pub valid_from: String,
    pub valid_to: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryHit {
    pub memory: Memory,
    pub score: f64,
}

// ===== 初始化 =====

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS core_memory (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS memories (
    id                    INTEGER PRIMARY KEY,
    content               TEXT NOT NULL,
    category              TEXT NOT NULL DEFAULT 'general',
    source_conversation_id TEXT,
    importance            REAL NOT NULL DEFAULT 0.5,
    valid_from            TEXT NOT NULL DEFAULT (datetime('now')),
    valid_to              TEXT,
    created_at            TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at            TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE VIRTUAL TABLE IF NOT EXISTS memory_vecs USING vec0(
    embedding float[384]
);

CREATE VIRTUAL TABLE IF NOT EXISTS memory_fts USING fts5(
    content,
    content='memories',
    content_rowid='id'
);

CREATE INDEX IF NOT EXISTS idx_memories_category ON memories(category);
CREATE INDEX IF NOT EXISTS idx_memories_valid ON memories(valid_to);
";

impl MemoryDb {
    pub fn open(path: &Path) -> SqlResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn })
    }

    // ===== Core Memory =====

    pub fn core_get_all(&self) -> SqlResult<Vec<CoreMemory>> {
        let mut stmt = self.conn.prepare(
            "SELECT key, value, updated_at FROM core_memory ORDER BY key",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(CoreMemory {
                key: row.get(0)?,
                value: row.get(1)?,
                updated_at: row.get(2)?,
            })
        })?;
        rows.collect()
    }

    #[allow(dead_code)]
    pub fn core_set(&self, key: &str, value: &str) -> SqlResult<()> {
        self.conn.execute(
            "INSERT INTO core_memory(key, value, updated_at) VALUES (?1, ?2, datetime('now'))
             ON CONFLICT(key) DO UPDATE SET value = ?2, updated_at = datetime('now')",
            params![key, value],
        )?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn core_delete(&self, key: &str) -> SqlResult<()> {
        self.conn.execute("DELETE FROM core_memory WHERE key = ?1", params![key])?;
        Ok(())
    }

    pub fn core_as_prompt_section(&self) -> String {
        let entries = self.core_get_all().unwrap_or_default();
        if entries.is_empty() {
            return String::new();
        }
        let mut s = String::from("## 关于用户的已知信息\n\n");
        for e in &entries {
            s.push_str(&format!("- **{}**: {}\n", e.key, e.value));
        }
        s
    }

    // ===== Long-term Memory 写入 =====

    pub fn insert_memory(
        &self,
        content: &str,
        category: &str,
        source_conversation_id: Option<&str>,
        importance: f32,
        embedding: &[f32],
    ) -> SqlResult<i64> {
        let rowid = {
            let mut stmt = self.conn.prepare(
                "INSERT INTO memories(content, category, source_conversation_id, importance)
                 VALUES (?1, ?2, ?3, ?4)",
            )?;
            stmt.execute(params![content, category, source_conversation_id, importance])?;
            self.conn.last_insert_rowid()
        };

        self.conn.execute(
            "INSERT INTO memory_vecs(rowid, embedding) VALUES (?1, ?2)",
            params![rowid, embedding.as_bytes()],
        )?;

        self.conn.execute(
            "INSERT INTO memory_fts(rowid, content) VALUES (?1, ?2)",
            params![rowid, content],
        )?;

        Ok(rowid)
    }

    #[allow(dead_code)]
    pub fn invalidate_memory(&self, id: i64) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE memories SET valid_to = datetime('now'), updated_at = datetime('now') WHERE id = ?1",
            params![id],
        )?;
        // FTS 删掉旧条目（不再被搜到）
        self.conn.execute("DELETE FROM memory_fts WHERE rowid = ?1", params![id])?;
        Ok(())
    }

    pub fn update_memory_content(
        &self,
        id: i64,
        new_content: &str,
        new_embedding: &[f32],
    ) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE memories SET content = ?2, updated_at = datetime('now') WHERE id = ?1",
            params![id, new_content],
        )?;

        // 向量表没有 UPDATE，需要删旧插新
        self.conn.execute("DELETE FROM memory_vecs WHERE rowid = ?1", params![id])?;
        self.conn.execute(
            "INSERT INTO memory_vecs(rowid, embedding) VALUES (?1, ?2)",
            params![id, new_embedding.as_bytes()],
        )?;

        // FTS 也要更新
        self.conn.execute("DELETE FROM memory_fts WHERE rowid = ?1", params![id])?;
        self.conn.execute(
            "INSERT INTO memory_fts(rowid, content) VALUES (?1, ?2)",
            params![id, new_content],
        )?;
        Ok(())
    }

    /// 查找与给定内容最相似的活跃记忆（向量距离 < threshold）。
    /// 返回 (id, distance, content)。
    pub fn find_similar(
        &self,
        embedding: &[f32],
        threshold: f64,
        limit: usize,
    ) -> SqlResult<Vec<(i64, f64, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT v.rowid, v.distance, m.content
             FROM memory_vecs v
             JOIN memories m ON m.id = v.rowid
             WHERE v.embedding MATCH ?1 AND k = ?2
               AND m.valid_to IS NULL",
        )?;
        let rows = stmt.query_map(params![embedding.as_bytes(), limit], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?;
        let mut results = Vec::new();
        for r in rows {
            let (id, dist, content) = r?;
            if dist < threshold {
                results.push((id, dist, content));
            }
        }
        Ok(results)
    }

    // ===== 检索 =====

    /// 向量 KNN 搜索（仅活跃记忆）
    pub fn search_by_vector(
        &self,
        embedding: &[f32],
        limit: usize,
    ) -> SqlResult<Vec<MemoryHit>> {
        let mut stmt = self.conn.prepare(
            "SELECT v.rowid, v.distance, m.content, m.category,
                    m.source_conversation_id, m.importance,
                    m.valid_from, m.valid_to, m.created_at, m.updated_at
             FROM memory_vecs v
             JOIN memories m ON m.id = v.rowid
             WHERE v.embedding MATCH ?1 AND k = ?2
               AND m.valid_to IS NULL",
        )?;
        let rows = stmt.query_map(params![embedding.as_bytes(), limit], |row| {
            Ok(MemoryHit {
                score: row.get(1)?,
                memory: Memory {
                    id: row.get(0)?,
                    content: row.get(2)?,
                    category: row.get(3)?,
                    source_conversation_id: row.get(4)?,
                    importance: row.get(5)?,
                    valid_from: row.get(6)?,
                    valid_to: row.get(7)?,
                    created_at: row.get(8)?,
                    updated_at: row.get(9)?,
                },
            })
        })?;
        rows.collect()
    }

    /// FTS5 全文搜索（仅活跃记忆）
    pub fn search_by_text(&self, query: &str, limit: usize) -> SqlResult<Vec<MemoryHit>> {
        let mut stmt = self.conn.prepare(
            "SELECT f.rowid, f.rank, m.content, m.category,
                    m.source_conversation_id, m.importance,
                    m.valid_from, m.valid_to, m.created_at, m.updated_at
             FROM memory_fts f
             JOIN memories m ON m.id = f.rowid
             WHERE memory_fts MATCH ?1
               AND m.valid_to IS NULL
             ORDER BY f.rank
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![query, limit], |row| {
            Ok(MemoryHit {
                score: row.get(1)?,
                memory: Memory {
                    id: row.get(0)?,
                    content: row.get(2)?,
                    category: row.get(3)?,
                    source_conversation_id: row.get(4)?,
                    importance: row.get(5)?,
                    valid_from: row.get(6)?,
                    valid_to: row.get(7)?,
                    created_at: row.get(8)?,
                    updated_at: row.get(9)?,
                },
            })
        })?;
        rows.collect()
    }

    /// 混合检索：向量 + FTS 结果合并去重，按分数排序。
    /// vector_weight: 向量分数的权重 (0~1)，FTS 权重 = 1 - vector_weight。
    ///
    /// 向量距离和 FTS rank 都是"越小越好"，这里统一转为
    /// "越大越好"的综合分 = importance + 相关性加权。
    pub fn hybrid_search(
        &self,
        embedding: &[f32],
        query: &str,
        limit: usize,
        vector_weight: f64,
    ) -> Vec<MemoryHit> {
        let vec_results = self.search_by_vector(embedding, limit * 2).unwrap_or_default();
        let fts_results = self.search_by_text(query, limit * 2).unwrap_or_default();

        let mut seen = std::collections::HashSet::new();
        let mut merged: Vec<MemoryHit> = Vec::new();

        // 向量距离越小越相关，转为相似度 (1 - dist)，钳制到 [0,1]
        for hit in vec_results {
            if seen.insert(hit.memory.id) {
                let similarity = (1.0 - hit.score).max(0.0);
                let mut h = hit;
                h.score = similarity * vector_weight;
                merged.push(h);
            }
        }
        // FTS rank 为负数（越小越好），取反转为正分
        for hit in fts_results {
            let fts_score = (-hit.score).max(0.0);
            if seen.insert(hit.memory.id) {
                let mut h = hit;
                h.score = fts_score * (1.0 - vector_weight);
                merged.push(h);
            } else if let Some(existing) = merged.iter_mut().find(|h| h.memory.id == hit.memory.id)
            {
                // 同一条记忆同时被向量+FTS命中，叠加加分
                existing.score += fts_score * (1.0 - vector_weight);
            }
        }

        // 综合分 = 相关性分 + importance 加权（越大越好）
        merged.sort_by(|a, b| {
            let sa = a.score + (a.memory.importance as f64) * 0.1;
            let sb = b.score + (b.memory.importance as f64) * 0.1;
            sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
        });

        merged.truncate(limit);
        merged
    }

    /// 统计活跃记忆数量
    pub fn count_active(&self) -> SqlResult<usize> {
        let count: usize = self
            .conn
            .query_row("SELECT COUNT(*) FROM memories WHERE valid_to IS NULL", [], |row| {
                row.get(0)
            })?;
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn test_db() -> MemoryDb {
        register_vec_extension();
        let id = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("jarvis_memory_test_{}", id));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        MemoryDb::open(&dir.join("test.db")).unwrap()
    }

    fn dummy_embedding(seed: f32) -> Vec<f32> {
        vec![seed; 384]
    }

    #[test]
    fn core_memory_crud() {
        let db = test_db();
        db.core_set("user_name", "张三").unwrap();
        db.core_set("role", "开发者").unwrap();

        let all = db.core_get_all().unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].key, "role");
        assert_eq!(all[1].key, "user_name");

        db.core_delete("role").unwrap();
        let all = db.core_get_all().unwrap();
        assert_eq!(all.len(), 1);

        let section = db.core_as_prompt_section();
        assert!(section.contains("张三"));
    }

    #[test]
    fn insert_and_search_vector() {
        let db = test_db();
        let emb = dummy_embedding(0.1);
        db.insert_memory("用户喜欢用 Python", "preference", None, 0.8, &emb)
            .unwrap();

        let results = db.search_by_vector(&emb, 5).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].memory.content, "用户喜欢用 Python");
    }

    #[test]
    fn invalidate_memory() {
        let db = test_db();
        let id = db
            .insert_memory("用户住在上海", "personal", None, 0.5, &dummy_embedding(0.2))
            .unwrap();

        db.invalidate_memory(id).unwrap();

        // 被过期的记忆不应出现在搜索结果中
        let results = db.search_by_vector(&dummy_embedding(0.2), 5).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn update_memory_content() {
        let db = test_db();
        let id = db
            .insert_memory("用户用 DeepSeek", "preference", None, 0.7, &dummy_embedding(0.3))
            .unwrap();

        db.update_memory_content(id, "用户改用 Claude", &dummy_embedding(0.35))
            .unwrap();

        // 用新向量搜到更新后的内容
        let new = db.search_by_vector(&dummy_embedding(0.35), 5).unwrap();
        assert!(!new.is_empty());
        assert_eq!(new[0].memory.content, "用户改用 Claude");
    }

    #[test]
    fn hybrid_search_merges_results() {
        let db = test_db();

        let emb1 = {
            let mut v = vec![0.0f32; 384];
            v[0] = 1.0;
            v
        };
        let emb2 = {
            let mut v = vec![0.0f32; 384];
            v[1] = 1.0;
            v
        };

        db.insert_memory("项目 A 使用 React", "project", None, 0.6, &emb1)
            .unwrap();
        db.insert_memory("项目 B 使用 Vue", "project", None, 0.6, &emb2)
            .unwrap();

        let results = db.hybrid_search(&emb1, "React", 5, 0.7);
        // 至少应该找到结果
        assert!(!results.is_empty());
        // React 条目应该在结果中
        let found_react = results.iter().any(|r| r.memory.content.contains("React"));
        assert!(found_react);
    }
}
