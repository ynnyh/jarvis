# 项目成本分析功能

## 目标

从禅道拉取项目任务和工时数据，结合人员时薪，计算项目实际人力成本。区分正常工时与加班工时，用户可选择是否纳入加班。

## 需求确认

### 工时计算规则
- **正常工时**：工作日每天 ≤8h 的部分，双休日不计
- **加班工时**：工作日 >8h 的部分 + 非工作日全部工时
- 默认仅统计正常工时，用户勾选"含加班"后合并计算
- 工作日/节假日规则复用现有 `WorkDaysSection` 配置

### 人员时薪配置
- 从禅道拉取所有项目的参与人员列表（去重），展示在设置页
- 用户为每人填入时薪，存本地 `~/.jarvis/cost-rates.json`
- 配置入口放在禅道设置页旁边或作为子 tab

### 时间范围
- 默认：项目全生命周期（从最早到最新工时记录）
- 快捷档：本月、本季度、近半年、今年
- 自由选择：起止日期

### 展示
- 桌面端（Phase 1）：小人右键「项目成本」入口 → 弹出窗口
- 频道端（Phase 2）：机器人查询命令

## 验收标准

- [ ] 设置页新增人员时薪配置：自动列出禅道所有参与人、批量编辑时薪
- [ ] 右键菜单「项目成本」入口，点击弹出成本分析窗口
- [ ] 成本窗口：选项目 + 选时间范围 + 是否含加班 checkbox
- [ ] 结果按项目汇总：每人正常工时、加班工时、时薪、小计 → 项目总成本
- [ ] 默认用项目全生命周期时间范围，也支持月/季度/自定义

## 数据流

```
设置页拉禅道人员 → 存 cost-rates.json（每人时薪）
                         ↓
用户点「项目成本」→ 选项目 + 时间范围 + 含加班
                         ↓
Rust 拉禅道任务列表（按项目筛选）
  → get_efforts 拉每人每日工时
  → 按工作日规则拆分 normalHours / overtimeHours
  → normalHours × 时薪 = 正常成本
  → overtimeHours × 时薪 = 加班成本（勾选时计入）
                         ↓
                  输出汇总表
```

## 实现计划

### Phase 1：桌面端（核心）
- 新增 `~/.jarvis/cost-rates.json` 读写
- 新增 Rust 命令：`cost_rates_load` / `cost_rates_save` / `project_cost_summary`
- 设置页新增「人员时薪」section
- 右键新增「项目成本」→ 成本展示窗口（项目选择 + 时间范围 + 含加班开关 + 汇总表）

### Phase 2：频道端
- 机器人查询命令：「查成本」→ 返回项目成本汇总

## 技术节点

- `cost-rates.json`：`{ "username": { hourlyRate: 100, displayName: "张三" } }`
- 时薪配置 UI：从禅道项目拉人员列表，用表格编辑
- 成本计算：复用 FineReport 的 `daily_hours` + 工作日判断逻辑
- 成本窗口：新建 Tauri 子窗口或复用现有浮层模式

---

## 重构方案 v2（2026-06-03 确认）⚠️ 已作废——禅道 OpenAPI 的 `/api.php/v1/tasks/{id}/work` 在本司禅道版本 404，且逐任务拉 = N+1 会打崩禅道。改用帆软，见下方 **v3**。

> 背景：v1 实现把 `team[].consumed` 当主数据源、effort 仅在勾「含加班」时才逐任务拉，导致：
> ①禅道 team/effort 默认只返回 account 拿不到中文名；②无法做时间范围筛选（team consumed 无日期）；
> ③assignee 兜底逻辑漏算（多任务负责人只算首个任务）；④加班拆分与基础工时两个数据源对不齐，靠补差硬凑、
> `正常+加班 ≠ 工时`；⑤每日阈值写死 8h，未复用设置里的工时时段。本次按 PRD 原始数据流（effort 为核心）整体重构。

### 已确认决策
1. **数据源**：以**工作日志(effort)为唯一口径**全面重构，替代 team[].consumed 主源。
2. **时间范围**：本次补全（默认全周期 + 本月/本季/近半年/今年 + 自定义起止）。
3. **加班口径**：工作日判定用 `chinese_holiday`（法定，含调休补班）；每日正常工时阈值用 `WorkDaysSection` 的每日时段总和（`workSchedule.periods` Σ时长），缺省 8h。

### 后端 — `zentao.rs`
- 新增 `get_all_users(&self) -> Result<Vec<Value>, String>`：`GET api.php/v1/users`，复用 `ensure_token` + Token header。
  - 防御性解析响应：兼容 `{users:[...]}` / `{data:{users:[...]}}` / 顶层数组。
  - 每条取 `account` + 中文名（`realname`→`realName`→`name` 兜底）。
  - 分页：带 `?limit=1000`；若响应含 `page`/`total` 且未取全，循环翻页。
  - 失败返回 `Err`，调用方降级为空映射，**不得阻断成本计算**。

### 后端 — `cost_rates.rs`（重写 `project_cost_summary_inner`）
新签名（tauri command 同步加 camelCase `startDate`/`endDate`，均 `Option<String>`）：
```rust
pub async fn project_cost_summary_inner(
    project_name: &str,
    include_overtime: bool,
    start_date: Option<&str>,  // "YYYY-MM-DD"，含端点
    end_date: Option<&str>,    // "YYYY-MM-DD"，含端点
) -> Result<CostSummaryResult, String>
```
流程：
1. `tasks = get_all_project_tasks(project)`。
2. 筛 `consumed>0`（task 或 team 任一成员）的任务 id（沿用现有筛选逻辑）。
3. **并发拉 effort**（沿用 chunk=10 + `join_all`），每条提取：`account`、`date`、`consumed`(兼容 hours/effort/workHours)、所属 `task_id`。
4. **时间范围过滤**：保留 `start<=date<=end`（端点为 None 时不限该侧）。
5. `users = get_all_users()`（失败→空 map，打印 warning 不阻断）→ account→realname 权威映射。
6. **全部基于 effort 聚合**：
   - `hour_map`: account → 总工时(Σconsumed) + distinct `task_id` 集合（→ `task_count`）。
   - `daily_map`: (account,date) → Σconsumed。
   - 拆分：`threshold = daily_work_hours_from_config()`；工作日 `normal=min(h,threshold)`、`overtime=(h-threshold).max(0)`；非工作日 `normal=0`、`overtime=h`。
   - **不变式**：每人 `总工时 == normal+overtime`（同源，删除 v1 的补差逻辑）。
7. **中文名优先级**：cost-rates.json `displayName`(非空且≠account) > users `realname` > effort `realname` > `account`。
8. 成本：`cost=hours×rate`，`normal_cost`/`overtime_cost` 同。`include_overtime=false` 时 normal/overtime 字段保持现状（返回 `None`）。
9. **删除**：v1 的 team[].consumed 主聚合 + assignee 兜底（旧 137-167 段）、`fetch_overtime_breakdown` 的补差。team consumed 仅当某任务 effort 为空时作兜底（MVP：仅 `eprintln` 警告，不补值）。

新增辅助：
```rust
/// 读 ~/.jarvis/config.json 的 workSchedule.periods（[{start:"HH:MM", end:"HH:MM"}]），
/// Σ(end-start)/60 = 每日正常工时阈值；解析失败/为空 → 8.0。
fn daily_work_hours_from_config() -> f64
```
（用 `crate::settings::load_raw_config()`。）

`cost_team_members` / `cost_team_members_inner` 改造：返回带中文名，如
`Vec<MemberBrief { account: String, realname: String }>`（用 `get_all_users` 补名，空则 realname=account）。

### 后端 — `tools/cost_report.rs` + `chat_agent.rs`
- `cost_report_preview` 适配 `cost_team_members` 新返回（显示 realname）。
- `cost_report` 适配 `project_cost_summary_inner` 新签名（start/end 传 `None`=全周期）。
- 工具 schema 可选加 `startDate`/`endDate`（MVP 可暂不加，默认全周期）。

### 前端 — `CostApp.vue`
- 新增时间范围 UI：快捷档按钮（全周期/本月/本季/近半年/今年）+ 自定义起止 `<input type="date">`×2；选择后算出 start/end。
- `invoke('project_cost_summary', { projectName, include_overtime, startDate, endDate })`。
- 显示逻辑：后端已给真名，保留 `getDisplayName` 的本地覆盖编辑 + runQuery 回填。

### 前端 — `CostRatesSection.vue`
- `cost_team_members` 返回带 realname → 「姓名」列显示中文名（空则 account），填时薪时 `displayName` 存真名。

### 验收
- [ ] 不勾「含加班」也显示中文名（禅道用户表）。
- [ ] 每人「总工时 == 正常+加班」，合计无裂缝。
- [ ] 时间范围：全周期/本月/本季/近半年/今年/自定义 均生效。
- [ ] 每日加班阈值随设置里工时时段变化（非写死 8h）。
- [ ] 设置页时薪表、机器人「查成本」均显示中文名。
- [ ] `cargo build` + 前端 `tsc`/`vue-tsc` 通过。

---

## 重构方案 v3（2026-06-03 · 数据源改帆软，作废 v2）

> **为什么推翻 v2**：v2 走禅道 OpenAPI 逐任务拉 effort，实测 `GET /api.php/v1/tasks/{id}/work` 与 `/works` 在本司禅道版本都 404（端点不存在），且逐任务 = N+1（80 个有工时任务 → 上百次请求）会把禅道打崩。正解是复用公司已有的帆软 BI——这也正是本 PRD「技术节点」原写的"复用 FineReport 的 daily_hours + 工作日判断"。

### 数据源
帆软报表 `zentao/effort-report-example.cpt`（viewlet `DEFAULT_VIEWLET`）的 **reportIndex=1「禅道工时任务完成明细」**。
- 已有 `crate::fine_report::finereport_get_efforts_raw(begin, end, real_name, all_people)`（`fine_report/commands.rs:178`，注释已写明"`all_people=true` 拉全部门数据，成本分析用"）。
- `EffortRecord`（`fine_report/html_parser.rs`）字段齐全：`employee`(员工中文名)、`department`、`date`、`daily_total_hours`(当日总工时)、`item_hours`(单项工时)、`project_name`、`task_name`、`work_content`。
- 调用链 `login → open_report_and_get_session → submit_filter → fetch_report_html`，**固定几次 HTTP，与项目规模无关，完全不调禅道 effort**。

### 后端改动
1. **`fine_report`**：给 `submit_filter` 和 `finereport_get_efforts_raw` 增加 `project_name` 参数，填进 `PJ_NAME`（现写死 `""`）。
   - **不要破坏现有调用方**：`finereport_get_efforts`（日报/chat 用）继续传空 `project_name`。`submit_filter` 新增参数后，所有调用点同步传值（日报路径传 `""`）。
2. **`cost_rates.rs` 重写 `project_cost_summary_inner`**（签名保持 v2 的 `project_name / include_overtime / start_date / end_date`）：
   - 数据源换成 `finereport_get_efforts_raw(begin, end, None, true, project_name)`。
   - begin/end：`start_date`/`end_date` 缺省 → begin=`"2020-01-01"`、end=今天（全周期）。帆软报表必须有日期范围。
   - **聚合 key 改用 `employee`（中文名）**（帆软明细无禅道 account）。`MemberCost.account` 存 employee；`display_name` = cost-rates.json 覆盖值（非空且≠employee）否则 employee。
   - 仅保留 `project_name` 匹配的 record（帆软 PJ_NAME 已粗筛，Rust 端再按 `record.project_name == project_name` 精确过滤兜底）。
   - hours = Σ`item_hours`；task_count = distinct `task_name`；加班：按 (employee, date) 聚合 `item_hours`，工作日（`chinese_holiday`）阈值 `daily_work_hours_from_config()` 拆分，非工作日全算加班。**不变式仍是 总工时 == 正常+加班**。
   - 删除 v2 的禅道路径：不再调 `get_all_project_tasks` / `get_task_works` / `get_all_users`。
3. **`cost_team_members_inner`**（设置页人员）：改为调帆软（按项目、全周期）取 distinct `employee`，返回 `MemberBrief{ account: employee, realname: employee }`。
4. **`cost-rates.json`**：key 语义从 account → 员工中文名（设置页 + 成本窗口一致）。
5. `zentao.rs::get_all_users`（v2 新增）成本路径不再用，可删。

### 前端
- `CostApp.vue` / `CostRatesSection.vue`：时薪/中文名的 key 改用员工中文名；时间范围 UI、加班列等其余不变。
- 项目下拉：暂保留禅道 `list_projects`（轻量，1 次）选项目名，传给后端做 PJ_NAME 筛选。

### 默认决策（已与用户口头确认，如不对再纠正）
- 时薪表 / 成本聚合一律以**员工中文名**为 key。
- 项目筛选：选中项目名 → 帆软 `PJ_NAME` + Rust 端 `project_name` 精确兜底。
- 全周期默认 begin=`2020-01-01`。

### 验收
- [ ] 选项目、不勾加班：列出每人中文名 + 工时 + 成本，**0 次禅道 effort 调用**（日志无 `get_task_works`）。
- [ ] 勾加班：每人 总工时 == 正常 + 加班。
- [ ] 时间范围档（本月/本季/近半年/今年/自定义）生效（begin/end 传帆软）。
- [ ] 设置页人员列表来自帆软、显示中文名。
- [ ] `cargo build` + 前端 `vue-tsc --noEmit` 通过。

---

## v3 增补（2026-06-03 测试反馈：超时 + 员工状态）

> **现象**：实跑报 `error sending request`（拉明细 HTML `reportIndex=1` 时）。前面 login/open/submit 都成功，卡在 fetch。**根因**：默认全周期（2020~今）+ `all_people` 数据量太大，帆软 client 的 10s timeout 扛不住（用户确认）。

**改动**：
1. **默认时间范围改「本月」**：
   - 前端 `CostApp.vue` `rangePreset` 默认 `'thisMonth'`（原 `'all'`）。所有档（含「全周期」）+ 自定义保留，仅改默认值。
   - 后端 `project_cost_summary_inner` begin/end 缺省 → 本月 1 号 ~ 今天（原 `2020-01-01`）。
2. **员工状态（在职/离职）可选**：
   - `fine_report/client.rs::submit_filter` 的 `USER_STATUS`（现写死 `"0"`）参数化，新增 `user_status: &str`。
   - `finereport_get_efforts_raw` 加 `user_status` 参数透传；`finereport_get_efforts`（日报/chat 路径）传 `"0"`（在职，维持现状）。
   - `project_cost_summary` 命令加 `include_resigned: Option<bool>`：缺省/false → `USER_STATUS="0"`（仅在职）；true → `USER_STATUS=""`（全部含离职）。
   - 前端 `CostApp.vue` 加「含离职」checkbox（默认不勾），invoke 传 `includeResigned`。
   - ⚠️ **USER_STATUS 取值假设**：在职=`"0"`（已知）、全部/含离职=`""`（帆软空=不筛惯例，**待实测确认**；若不对，离职值由用户抓真实请求补）。
3. **timeout 分级**：成本 / 团队人员这类大报表查询走 **60s** 超时；日报维持 10s。
   - 实现建议：`FineReportClient` 加 `new_with_timeout(base, account, password, secs)`（原 `new` 内部调它传 10s），`finereport_get_efforts_raw` 用 60s 构建 client。不要动日报路径的 10s。
4. `cost_team_members_inner` 同步：传 `user_status="0"`（设置页列在职人员即可），其余不变。

**验收增补**：
- [ ] 成本窗口默认打开 = 本月范围，不超时、出数据。
- [ ] 「含离职」勾选能切换 在职 / 全部。
- [ ] 日报功能（`get_efforts` / `finereport_get_efforts`）不受影响（仍 10s、仍只在职）。
