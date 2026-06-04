# Memory System

> Agent 记忆系统（SQLite + sqlite-vec + FTS5）的执行契约与易错点。

---

## 1. 存储结构

| 表 | 作用 |
|----|------|
| `memories` | 事实正文（id, content, category, importance, valid_to, ...） |
| `memory_vecs` | sqlite-vec `vec0` 虚拟表，384 维向量，rowid = `memories.id` |
| `memory_fts` | FTS5 **external content** 表（`content='memories'`, `content_rowid='id'`） |

**三表写入必须用事务包裹**（`conn.unchecked_transaction()` → 多次 `tx.execute` → `tx.commit()`），避免中途失败导致正文/向量/FTS 脱节。`MemoryState.db` 用 `Arc<Mutex<MemoryDb>>` 以便记忆提取在后台 `spawn` 任务里共享。

---

## 2. Gotcha：FTS5 external content 表的更新顺序

> **Warning**：`memory_fts` 是 external content 表，`DELETE FROM memory_fts WHERE rowid=?` 时 FTS5 会**回查 `memories` 当前 content** 反算待删 token。

**Common Mistake**：更新内容时先 `UPDATE memories SET content=新值` 再 `DELETE memory_fts` → FTS5 用新内容反算删除 → 旧内容倒排 token 删不掉、残留 → 旧文本仍被搜到、索引腐化。

### Wrong
```rust
tx.execute("UPDATE memories SET content=?2 WHERE id=?1", ...)?; // 先改正文
tx.execute("DELETE FROM memory_fts WHERE rowid=?1", ...)?;       // FTS 按新值反算 → 旧 token 残留
tx.execute("INSERT INTO memory_fts(rowid,content) VALUES(?1,?2)", ...)?;
```

### Correct
```rust
tx.execute("DELETE FROM memory_fts WHERE rowid=?1", ...)?;       // 趁 memories 仍是旧值，正确反算删除
tx.execute("UPDATE memories SET content=?2 WHERE id=?1", ...)?;  // 再改正文
tx.execute("INSERT INTO memory_fts(rowid,content) VALUES(?1,?2)", ...)?;
```

`invalidate_memory` 只设 `valid_to`、不改 content，DELETE 顺序无所谓。

**Tests Required**：插入 "apple" → 搜得到；`update_memory_content` 改 "banana" → 搜 "apple" 为空、搜 "banana" 非空（见 `db.rs::update_memory_content_refreshes_fts`）。

**FTS query 转义**：用户 query 进 `MATCH` 前必须转义为 phrase（内部 `"`→`""`，再用 `"..."` 包裹），否则 `* : ^` 或 `AND/OR/NEAR` 等被当查询语法报错、被 `unwrap_or_default` 吞成空召回（见 `search_by_text_handles_special_chars`）。

---

## 3. 嵌入服务：可配置 + 不可用降级

**Contracts（`~/.jarvis/config.json`）**：

| key | 必需 | 说明 |
|-----|------|------|
| `embeddingBaseUrl` | 否 | 嵌入端点，缺省回退 LLM 提供商 URL |
| `embeddingModel` | 否 | 嵌入模型名，缺省 `text-embedding-3-small` |

**关键约束**：部分服务商（如 **DeepSeek**）**没有 `/embeddings` 端点**。嵌入不可用时必须降级，不能让记忆整体失效。

**降级契约**：
1. `embed()` 遇 `404` / `model_not_found` → 置位全局 `EMBEDDING_DISABLED`，之后直接短路返回 Err（不再发 HTTP），避免每轮对话白等往返阻塞首字。
2. 写入侧：嵌入为 `None` 时走 `insert_memory_fts_only`（只写 memories+FTS，跳过 vec），记忆仍可被关键词检索。
3. 检索侧：无 query 嵌入则跳过向量、仅 FTS。
4. 首次不可用 `warn_unavailable_once` 提示一次，引导配置 `embeddingBaseUrl`/`embeddingModel`（如指向本地 Ollama）。

**Validation & Error Matrix**：

| 条件 | 行为 |
|------|------|
| 凭证未配 | Err（**不**禁用，用户可能后续配置） |
| 404 / model_not_found | 置位禁用 + 短路 |
| 网络超时 | Err（**不**禁用，可能临时故障） |

---

## 4. 记忆合并去重（`store_fact_sync`）

按向量距离决策（向量可用时）：

| 距离 | 动作 |
|------|------|
| `< 0.3`（几乎重复） | 用新表述 `update_memory_content` 刷新 |
| `[0.3, 0.85)`（相似但可能不同） | **新增，不覆盖** |
| 无相似 / 无向量 | 新增（无向量走 FTS-only） |

**Why 不覆盖**：中等相似 ≠ 同一事实，覆盖会误删——如「喜欢 Python」被「喜欢 Java」顶掉。

**Tests Required**：相似但不同的两条事实（距离落 `[0.3,0.85)`）写入后 `count_active()==2`（见 `extractor.rs::store_similar_but_different_facts_not_overwritten`）。
