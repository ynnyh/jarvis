# 修复代码审查发现的成本与记忆模块问题

## Goal

对昨日（2026-06-03）新增的「Agent 记忆系统」与重构的「项目成本分析」做完整代码审查后，按优先级修复发现的正确性 bug、逻辑缺陷与可维护性问题。审查已完成，本任务为修复执行。

## What I already know（审查结论，已逐条核实关键项）

### 成本模块
- **C1 🔴 加班拆分整条失效**：`CostApp.vue:166` invoke 里 `include_overtime` 误写成 snake_case，后端命令 `project_cost_summary`（`cost_rates.rs:361-368`）无 `rename_all`、Tauri v2 默认 camelCase → 收不到 → `unwrap_or(false)` → 永不拆分 → 前端 `hasOvertime` 恒 false。**已亲自核实**。一行修复：`includeOvertime`。
- **C2 🟡 halfYear 口径不一致**：前端 `CostApp.vue:74-77` 用「精确 6 个月前同一天」，渠道 `cost_report.rs:31-39` 用「往前 5 个月 1 号」（spec §7）。同一用户两路径数字不同。
- **C3 🟡 自定义区间不校验 start>end**：`cost_rates.rs:213-220` / `CostApp.vue` 两日期框不互约束，颠倒填 → 帆软空结果、只提示「暂无数据」。`FineReportSection.vue:266` 有此校验，成本路径漏了。
- **C4 🟡 加班阈值跨午夜班次静默丢弃**：`cost_rates.rs:139-143` `if e>s` 丢弃夜班时段 → 阈值偏小 → 加班高估。
- **C5 🟡 节假日跨年失准**：`cost_rates.rs:280-283` 依赖 `chinese_holiday` 内置表，超覆盖年份判错；解析失败 `unwrap_or(true)` 把节假日加班误算正常工时。
- **C6 🔵 preview 全周期超时**：`cost_rates.rs:74-85` `cost_report_preview` 拉 2020-01-01~今天全部门，大项目易撞 60s 超时（两步确认第一步就挂）。
- **C7 🔵 fmt_money 死分支**：`cost_report.rs:136-139` `>=1000` 与 `else` 输出相同。**已核实**。
- **C8 🔵 item_hours f32 精度**：`html_parser.rs` 解析成 f32 再转 f64，建议源头 f64。
- **C9 🔵 get_task_works 死代码**：`zentao.rs:551-640` 旧禅道工作日志方案残留，现行数据流不用。

### 记忆模块
- **M1 🔴 提取阻塞命令返回十几秒**：`chat.rs:177-203` 注释称「异步」实为顺序 await（全文件无 spawn，**已核实**），提取 LLM（15s）+ 逐条嵌入（30s/条串行）跑完才 return。应 `tauri::async_runtime::spawn` 真正异步化。
- **M2 🔴 FTS 外部内容表更新顺序腐化索引**：`db.rs:196-213` 先 UPDATE memories.content 再 DELETE memory_fts，FTS5 按新内容反算删除致旧 token 残留。应改顺序或用 `'delete'` 命令。
- **M3 🔴 DeepSeek 等无 /embeddings 端点 → 长期记忆整层失效**：`embedding.rs:64` 模型名硬编码 `text-embedding-3-small`，embed 失败则写入/检索全跳过。需让 embeddingModel 可配置 + 探测/提示。**修法待定（见 Open Questions）**。
- **M4 🟡 Core Memory 层无写入路径（死代码）**：`db.rs:120-133` core_set/core_delete 全仓除测试零调用（**已核实**），core_memory 表永远空。
- **M5 🟡 has_system 时记忆被算后丢弃**：`chat.rs:138-144`。
- **M6 🟡 query 嵌入阻塞首字**：`chat.rs:118` 主 LLM 前先 await 嵌入。
- **M7 🟡 按距离覆盖记忆误删不同事实**：`extractor.rs:56-67` distance∈[0.3,0.85) 即覆盖旧记忆。
- **M8 🟡 三表写入非事务**：`db.rs:149-177`/`190-215` 中途失败状态脱节。
- **M9 🟡 FTS query 未转义**：`db.rs:282-293` 特殊字符致 search_by_text 抛错被 unwrap_or_default 吞。
- **M10 🔵 嵌入串行应并发**：`chat.rs:186-190`。
- **M11 🔵 相似度/阈值与归一化耦合**：`db.rs:334` (1-dist) + extractor 阈值，vec0 默认 L2，embed 未归一化。
- **M12 🔵 register_vec_extension 重复注册**：`db.rs:17-23` 用 Once。
- **M13 🔵 每轮提取无节流**：`chat.rs:178` double LLM 成本。
- **M14 🔵 run() Result 摆设**：`lib.rs:30` 改 Result 但仍 expect panic、恒 Ok。
- **M15 🔵 无记忆管理命令**：用户无法查看/清除被记住内容。

### 系统性
- 前后端参数命名靠 camelCase↔snake_case 约定、无编译期保证（C1 即此类），值得类型化 invoke wrapper。

## Decision（ADR-lite）

**Context**：审查发现 24 项问题；本地实测默认 LLM = DeepSeek（`deepseek-v4-flash`）、`embeddingBaseUrl` 未配，DeepSeek 无 `/embeddings` 端点 → 长期记忆当前完全失效（M3 实锤）。
**Decision**：
- 范围 = 全部 🔴 严重 + 🟡 重要 + 低成本 🔵 清理；**不含** M15（记忆管理 UI，新功能另开）。
- M3 方案 = 新增 `embeddingModel`/`embeddingBaseUrl` 可配置 + 嵌入不可用时降级为纯 FTS（仍写入、仍按关键词检索）+ 一次性提示。
**Consequences**：批次较大（~20 项），分三批实现并各自验证；记忆写入逻辑需支持「无向量」路径（FTS-only）。

## Requirements（本批）

### 成本
- C1：`CostApp.vue:166` `include_overtime` → `includeOvertime`。
- C2：前端 `halfYear` 对齐 spec §7 口径（往前 5 个月 1 号 ~ 今天）。
- C3：自定义区间 `start>end` 入口校验（后端 `project_cost_summary_inner` + 前端 runQuery）。
- C4：`daily_work_hours_from_config` 跨午夜班次（`e<=s` 按 +1440 分钟）。
- C5：节假日超 `chinese_holiday` 覆盖范围时提示；复核 `unwrap_or(true)` 兜底方向。
- C6：`cost_report_preview` 限定窗口（如近 1 年），不走 2020~今天全周期。
- C7：删 `fmt_money` 等价死分支。
- C8：`html_parser` `item_hours` 源头解析为 `f64`。
- C9：清理 `get_task_works` 死代码（或显式标注废弃）。

### 记忆
- M1：提取改 `tauri::async_runtime::spawn` 真异步，emit Done 后立即返回（db 需 Arc 共享）。
- M2：`update_memory_content` 改 content 前先删 FTS（或用 `'delete'` 命令带旧内容）。
- M3：嵌入可配置 + 不可用降级纯 FTS（写入允许无向量）+ 一次性提示。
- M4：Core Memory 接入写入路径（高 importance 事实同步写 core）或移除死代码 —— 实现时定。
- M5：`has_system` 时把记忆拼入已有 system（或跳过计算省请求）。
- M6：query 嵌入与主 LLM 调用并发 / 失败快速短路。
- M7：`store_fact_sync` 改为仅极近（<0.3）才合并、否则新增。
- M8：`insert_memory`/`update_memory_content` 用 `conn.transaction()` 包裹三表写入。
- M9：`search_by_text` 的 query 转义为 phrase（`"..."`）。
- M10：多条 fact 嵌入 `join_all` 并发。
- M12：`register_vec_extension` 用 `std::sync::Once`。
- M14：`run()` 去掉摆设 `Result`（或把 `.expect` 改 `?`）。

## Acceptance Criteria

- [ ] C1：大窗勾选「含加班」后正常/加班拆分列正确显示。
- [ ] C3：颠倒日期区间给出明确错误提示，不静默空结果。
- [ ] M1：`chat_send_stream` 在 emit Done 后立即返回，提取在后台进行。
- [ ] M2：更新记忆内容后旧 token 不残留（旧内容搜不到、新内容搜得到）——加单测。
- [ ] M3：当前 DeepSeek 配置下长期记忆能写入并按关键词（FTS）检索；嵌入不可用有一次性提示。
- [ ] M7：相似但不同的事实（如「喜欢 Python」vs「喜欢 Java」）不互相覆盖——加单测。
- [ ] cargo check / 前端 tsc 通过，记忆 db 单测通过。

## Definition of Done

- 相关单测补充/更新（记忆 db、cost resolve_range 等纯函数）。
- cargo check / 前端 tsc 通过。
- 行为变化处更新注释/spec。
- 全链路中文 commit。

## Out of Scope

- M15 记忆管理 UI 面板（新功能，另开任务）。
- C8 `item_hours` 改 f64：**跳过**——会波及 `effort_report.rs` 多处类型，且工时值（0.5/1.0/8.0 等）在 f32 下本就精确、`cost_rates.rs` 已 `as f64` 累加，实际精度损失为零，回归面 > 收益。
- M11 向量归一化、M13 提取节流：本批未做（价值低 / 非必需）。
- 前后端类型化 invoke wrapper（系统性改造，另议）。

## 实现结果（2026-06-04）

全部完成：C1-C7、C9（C8 跳过）；M1-M10、M12、M14。
- M3：嵌入 model 可配置 + 404 后置位禁用短路（M6 协同）+ 不可用时 FTS-only 降级写入 + 一次性提示。
- M4：移除从未接线的 core key-value 子系统，`build_core_prompt` 改取 longterm top-importance（≥0.8）作常驻画像。
- M7：相似但非极近（距离 [0.3,0.85)）不再覆盖，改为新增。

验证：`cargo check` ✓、`cargo test memory` 18 passed ✓（含 M2/M7/M9 回归用例）、`vite build` ✓。
待人工：CostApp「含加班」端到端需真实 Tauri + 帆软环境点验。

## Technical Notes

- 审查依据 spec：`.trellis/spec/backend/cost-analysis.md`、`fine-report-integration.md`。
- 后端命令注册：`lib.rs` invoke_handler。
- 记忆状态：`MemoryState { db: Option<Mutex<MemoryDb>> }`，spawn 异步化需考虑 Arc 共享。
- 修复优先级表（用户已认可）：C1 → M1 → M3 → M2 → C6/C2。
