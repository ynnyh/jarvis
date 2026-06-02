# 批量写工时 + 今日计划改造

## Goal

将今日复盘中的写工时流程从"逐个任务分别写"改造为"所有提交汇总在一个大窗口里，一键批量写入"；同时改造今日计划，使其为批量写工时提供工时估算基线，并放开任务选择限制（支持关键字搜索和 ID 添加）。

## Requirements

### 今日计划改造

- [ ] 每条任务增加工时估算输入框
- [ ] 打开时自动加载禅道候选任务（现有逻辑）
- [ ] 支持按任务 ID 手动添加（调用现有 `get_task(id)` 验证）
- [ ] 支持关键字搜索禅道任务（尝试更广的接口，备选方案见技术决策）
- [ ] 支持添加自定义任务（非禅道任务，用于计划但不写入工时）
- [ ] 支持添加事务类任务（晨会、评审等常规项）
- [ ] 底部显示预计合计工时 vs 8h 进度条
- [ ] 保存后计划持久化，供批量写工时窗口读取

### 批量写工时窗口

- [ ] 独立大窗口（约 900x700），从今日复盘顶部按钮打开
- [ ] 自动加载当天所有提交，按任务分组展示
- [ ] 读取今日计划的估算工时作为默认值
- [ ] 每组可编辑工时（按比例预填，用户可调）和工作内容（自动拼合 commit title）
- [ ] 有计划但无 commit 的任务标 ⚠️ 提醒
- [ ] 未关联提交展示在下方，支持下拉搜索选择任务归属
- [ ] 支持添加事务类工作项（可搜索禅道任务）
- [ ] 展示当天已写工时（来自帆软）
- [ ] 底部合计进度条（X/8h）和"一键写入"按钮
- [ ] 写入时显示进度（N/M 已完成），写入后出结果汇总（成功/失败列表）

### 衔接

- [ ] 今日计划的估算工时作为批量写工时的默认值
- [ ] 有计划但无 commit 的任务标 ⚠️ 提醒

## Acceptance Criteria

- [ ] 今日计划可搜索到未来日期的禅道任务并选中
- [ ] 今日计划支持直接输入任务 ID 添加
- [ ] 今日计划每条任务可设估算工时，底部实时合计
- [ ] 批量写窗口打开后，所有今日提交按任务分组展示
- [ ] 批量写默认工时来自今日计划估算（有计划时）或按 commit 比例分配 8h（无计划时）
- [ ] 一键写入成功调用禅道接口，工时正确写入
- [ ] 已写工时可展示（从帆软查询）
- [ ] 写入失败的任务单独列出，不影响其他任务写入
- [ ] 写入有幂等保护（复用现有 clientRequestId 机制）

## Definition of Done

- Lint / typecheck 通过
- 现有功能不受影响（今日复盘、单个写工时、晚间提醒等）
- 批量写入有审计日志

## Out of Scope

- 禅道以外的工时系统支持
- 移动端适配
- 拖拽排序（条目按计划 + commit 数量自动排序即可）

## Technical Approach

### 窗口

BatchWrite 作为一个独立 Tauri webview 窗口，沿袭 writeHours/todayPlan 的窗口模式：
- `tauri.conf.json` 注册 `batchWrite` 窗口（900x700，居中，默认隐藏）
- `vite.config.ts` 添加 `batchWrite: resolve(__dirname, 'desktop/batchWrite.html')`
- 新建 `desktop/batchWrite.html`、`desktop/src/batchWrite-main.ts`、`desktop/src/BatchWriteApp.vue`
- `commands/window.rs` 添加 `batch_write_open` / `batch_write_close`
- `lib.rs` 注册命令 + 注册 CloseRequested 拦截

### 数据流

1. 打开时，前端调 `tool_execute('get_daily_review')` 获取今日数据
2. 前端调 `load_today_plan` 获取计划工时估算
3. 前端合并数据，用户编辑
4. 点击一键写入 → 前端循环调 `tool_execute('log-task-effort', ...)` 逐条写入
5. 每写完一条更新进度
6. 全部完成出汇总

### 任务搜索

见技术决策 #1

## Decision (ADR-lite)

**任务搜索方案**: 终选用方案 B——后端换更广接口（`my-task-assignedTo` 或 `task-browse-0`）一次拉全用户所有指派任务，前端做本地模糊搜索。ID 直加（调 `get_task(id)` 验证）兜底。

**批量写入**: 前端循环调 `tool_execute('log-task-effort', ...)` 逐条写入，复用现有幂等/审计机制。

**防重复写入**: 三层防护——
1. 写入成功后卡片变 "✅ 已写入"，不可编辑
2. 预检：写入前查 `write-back.log` 跳过已有 `clientRequestId` 的
3. 写制中按钮 disabled + 转圈

## Technical Notes

### 禅道搜索能力
- `my-work-task-assignedTo--id_desc.json` — 仅返回当前工作台任务（未来日期不可见）
- OpenAPI `GET /api.php/v1/tasks/{id}` — 可以查单任务
- 无原生搜索 API 可用
- 备选：`task-browse-{product}--id_desc-0-0-0-0-200.json` 可能返回更广范围

### 已有模式可复用
- 窗口生命周期：writeHours 的 payload 传参模式
- 幂等写入：clientRequestId 机制
- 审计日志：write-back.log
- 今日计划持久化：config.json `todayPlan` 字段

### 关键文件清单
**新建：**
- `desktop/src/BatchWriteApp.vue`
- `desktop/src/batchWrite-main.ts`
- `desktop/batchWrite.html`

**修改：**
- `desktop/src/TodayPlanApp.vue` — 加工时输入、搜索、自定义任务
- `desktop/src/components/ReviewWindow.vue` — 加"批量写工时"按钮
- `desktop/vite.config.ts` — 注册 batchWrite 入口
- `src-tauri/tauri.conf.json` — 注册 batchWrite 窗口
- `src-tauri/capabilities/default.json` — 权限
- `src-tauri/src/commands/window.rs` — 加 open/close 命令
- `src-tauri/src/lib.rs` — 注册命令
- `src-tauri/src/zentao.rs` — 加搜索方法（如需）
- `src-tauri/src/worklog.rs` — 补 loads/saves 增强

### 风险
- 禅道不同版本 browse 接口路径可能不一致，搜索功能可能需要兼容处理
- 批量写入中若某条失败，需决定继续还是中止（建议继续，汇总时列出失败项）
