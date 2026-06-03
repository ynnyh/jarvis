# Agent 记忆系统

## Goal

为 Jarvis Agent 添加持久化记忆能力，实现跨会话上下文保持。Agent 能记住用户偏好、项目知识、历史决策，无需每次对话重新解释背景。

## Requirements

### 三层记忆架构

* **Core Memory**（常驻 prompt）：用户画像、偏好、活跃上下文。始终注入 system prompt，约 500-2000 tokens
* **Working Memory**：近期对话上下文，现有滑动窗口机制已有，不新增
* **Long-term Memory**：持久事实和知识，向量 + FTS 混合检索，按需注入

### 记忆提取

* **时机**：每轮用户-助手对话结束后，异步提取（不阻塞主流程）
* **方式**：LLM 调用提取关键事实，与已有记忆比较后决定 新增/合并/更新/过期
* **提取 prompt**：设计专门的提取 prompt，从对话中抽取出结构化事实

### 记忆检索与注入

* 用户发消息时，从 Long-term Memory 中检索 top-K 相关记忆
* 混合检索：向量相似度（sqlite-vec）+ FTS5 全文搜索，结果融合排序
* 检索到的记忆 + Core Memory 动态拼入 system prompt

### 记忆合并去重

* 新事实与已有记忆比较（向量相似度阈值）
* 相似度过高 → 合并更新
* 矛盾信息 → 旧记忆设 valid_to 过期，插入新记忆
* 独立新事实 → 直接插入

### 存储

* SQLite（rusqlite）+ sqlite-vec 向量扩展
* 数据库文件：`~/.jarvis/memory.db`
* 嵌入模型：all-MiniLM-L6-v2（384维），本地 ONNX 运行（onnxruntime crate）
* 记忆带时间戳（created_at, updated_at, valid_from, valid_to）

## Acceptance Criteria

* [ ] 跨会话记忆保持：关闭聊天重开后，Agent 仍记得之前讨论的关键事实
* [ ] 记忆自动提取：对话结束后异步提取事实并持久化
* [ ] 记忆检索相关：新对话中，能检索到与当前话题相关的历史记忆
* [ ] 系统 prompt 动态化：prompt 中包含 Core Memory 和检索到的相关记忆
* [ ] 提取不阻塞：记忆提取异步进行，不影响聊天响应速度
* [ ] 不影响现有功能：现有聊天、工具、频道功能无回归

## Definition of Done

* Rust 模块 `src-tauri/src/memory/` 实现完整
* SQLite schema + sqlite-vec 向量索引
* ONNX 嵌入模型集成
* Tauri commands 暴露（如有前端交互需要）
* 单元测试覆盖提取/检索/合并核心逻辑
* CHANGELOG 更新

## Decision (ADR-lite)

**Context**：需要为 Agent 选择记忆系统架构
**Decision**：三层记忆 + SQLite + 本地 ONNX 嵌入 + 实时异步提取
**Consequences**：
  - 新增依赖：rusqlite, sqlite-vec, onnxruntime（约增加 5-10MB 二进制体积）
  - 首次启动需下载嵌入模型文件（~90MB）
  - 每次 LLM 对话额外一次提取调用（增加 token 消耗）
  - 换来跨会话连续性 + 离线可用 + 检索相关性好

## Out of Scope

* 知识图谱（Neo4j 等）
* 过程记忆（学习到的技能/行为模式）
* 多用户记忆隔离
* 频道会话（Telegram/QQ）记忆（后续跟进）
* 记忆管理 UI（V1 无前端界面）
* 记忆导入/导出
* 嵌入模型训练/微调

## Technical Notes

* 关键文件：`src-tauri/src/chat_agent.rs`（agent loop + system prompt）、`src-tauri/src/conversations.rs`（对话存储）、`src-tauri/src/llm.rs`（LLM 客户端）
* 存储：`~/.jarvis/` 目录，无数据库，JSON 文件
* 需要引入 SQLite 依赖（rusqlite + sqlite-vec 扩展）
* 嵌入模型：all-MiniLM-L6-v2（384维，本地 ONNX）
* 对齐 GitHub 高赞项目：Mem0（提取+合并去重）、Letta（三层架构）、Zep（时序感知）
