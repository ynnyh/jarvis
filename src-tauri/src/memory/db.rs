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
    // 全局 auto extension 只需注册一次，避免每次 MemoryState::new 重复注册。
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| unsafe {
        rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute(
            sqlite_vec::sqlite3_vec_init as *const (),
        )));
    });
}

pub struct MemoryDb {
    conn: Connection,
}

// ===== 数据结构 =====

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

    // ===== Long-term Memory 写入 =====

    pub fn insert_memory(
        &self,
        content: &str,
        category: &str,
        source_conversation_id: Option<&str>,
        importance: f32,
        embedding: &[f32],
    ) -> SqlResult<i64> {
        // 三表（memories/memory_vecs/memory_fts）写入用事务包裹，避免中途失败导致脱节。
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO memories(content, category, source_conversation_id, importance)
             VALUES (?1, ?2, ?3, ?4)",
            params![content, category, source_conversation_id, importance],
        )?;
        let rowid = tx.last_insert_rowid();

        tx.execute(
            "INSERT INTO memory_vecs(rowid, embedding) VALUES (?1, ?2)",
            params![rowid, embedding.as_bytes()],
        )?;

        tx.execute(
            "INSERT INTO memory_fts(rowid, content) VALUES (?1, ?2)",
            params![rowid, content],
        )?;

        tx.commit()?;
        Ok(rowid)
    }

    /// 无向量降级写入（嵌入服务不可用时）：只写 memories + memory_fts，跳过 memory_vecs。
    /// 记忆仍可被 FTS 关键词检索，只是不参与向量 KNN。
    pub fn insert_memory_fts_only(
        &self,
        content: &str,
        category: &str,
        source_conversation_id: Option<&str>,
        importance: f32,
    ) -> SqlResult<i64> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO memories(content, category, source_conversation_id, importance)
             VALUES (?1, ?2, ?3, ?4)",
            params![content, category, source_conversation_id, importance],
        )?;
        let rowid = tx.last_insert_rowid();
        tx.execute(
            "INSERT INTO memory_fts(rowid, content) VALUES (?1, ?2)",
            params![rowid, content],
        )?;
        tx.commit()?;
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
        // 三表写入用事务包裹，保证原子性。
        let tx = self.conn.unchecked_transaction()?;

        // memory_fts 是 external content 表（content='memories'），DELETE 时 FTS5 会回查
        // memories 当前 content 反算待删 token。必须在 UPDATE memories 之前删除，否则会用
        // 新内容反算 → 旧 token 残留、索引腐化（旧文本仍能被搜到）。
        tx.execute("DELETE FROM memory_fts WHERE rowid = ?1", params![id])?;

        tx.execute(
            "UPDATE memories SET content = ?2, updated_at = datetime('now') WHERE id = ?1",
            params![id, new_content],
        )?;

        // 向量表没有 UPDATE，需要删旧插新
        tx.execute("DELETE FROM memory_vecs WHERE rowid = ?1", params![id])?;
        tx.execute(
            "INSERT INTO memory_vecs(rowid, embedding) VALUES (?1, ?2)",
            params![id, new_embedding.as_bytes()],
        )?;

        // FTS 插入新内容
        tx.execute(
            "INSERT INTO memory_fts(rowid, content) VALUES (?1, ?2)",
            params![id, new_content],
        )?;

        tx.commit()?;
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
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }
        // 转义为 FTS5 phrase：内部 " 转义为 ""，再整体用引号包裹，
        // 避免用户输入里的 " * : ^ 或 AND/OR/NEAR 等被当成查询语法导致 MATCH 报错。
        let safe_query = format!("\"{}\"", query.replace('"', "\"\""));
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
        let rows = stmt.query_map(params![safe_query, limit], |row| {
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

    /// 取 importance 最高的活跃记忆，用于常驻"核心画像"注入。
    pub fn top_important(&self, limit: usize) -> SqlResult<Vec<Memory>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, content, category, source_conversation_id, importance,
                    valid_from, valid_to, created_at, updated_at
             FROM memories
             WHERE valid_to IS NULL
             ORDER BY importance DESC, updated_at DESC
             LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit], |row| {
            Ok(Memory {
                id: row.get(0)?,
                content: row.get(1)?,
                category: row.get(2)?,
                source_conversation_id: row.get(3)?,
                importance: row.get(4)?,
                valid_from: row.get(5)?,
                valid_to: row.get(6)?,
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
            })
        })?;
        rows.collect()
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
    fn update_memory_content_refreshes_fts() {
        // 回归：external content FTS 表必须在 UPDATE memories 之前删除旧索引项，
        // 否则旧 token 残留、更新后仍能搜到旧内容。
        let db = test_db();
        let id = db
            .insert_memory("user likes apple", "preference", None, 0.5, &dummy_embedding(0.4))
            .unwrap();
        assert!(!db.search_by_text("apple", 5).unwrap().is_empty());

        db.update_memory_content(id, "user likes banana", &dummy_embedding(0.45))
            .unwrap();

        assert!(
            db.search_by_text("apple", 5).unwrap().is_empty(),
            "更新后旧内容 token 不应残留在 FTS 索引中"
        );
        assert!(
            !db.search_by_text("banana", 5).unwrap().is_empty(),
            "更新后新内容应可被 FTS 搜到"
        );
    }

    #[test]
    fn search_by_text_handles_special_chars() {
        // 回归：含 FTS5 语法字符的 query 不应导致 MATCH 报错。
        let db = test_db();
        db.insert_memory("project uses Rust", "project", None, 0.5, &dummy_embedding(0.5))
            .unwrap();
        // 这些 query 含 " : * 等特殊字符，转义前会让 MATCH 抛错
        for q in ["\"unbalanced", "a:b", "foo*", "AND OR"] {
            assert!(db.search_by_text(q, 5).is_ok(), "query {:?} 不应报错", q);
        }
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
