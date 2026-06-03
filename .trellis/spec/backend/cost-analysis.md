# Cost Analysis

> 项目成本分析 —— 数据流、聚合 key、加班拆分、渠道注册。

---

## 1. 数据流

```
帆软 BI 工时明细(reportIndex=1)
  → Rust finereport_get_efforts_raw(begin, end, None, true, projectName, userStatus)
  → 按 project_name 精确兜底
  → 按 employee(中文名) 聚合 Σitem_hours + distinct task_name
  → 按 (employee, date) 聚合用每日工时阈值拆分正常/加班
  → × 时薪(cost-rates.json) = 成本
```

**完全不调禅道 effort API**（避免 N+1 打崩禅道）。

---

## 2. 聚合 Key —— 员工中文名

帆软明细无禅道 `account`，**一律以员工中文名 `employee` 为 key**。

| 存储 | Key | 说明 |
|------|-----|------|
| `cost-rates.json` | 员工中文名 | 如 `"张三": { hourlyRate: 100 }` |
| `MemberCost.account` | 员工中文名 | 从帆软 `employee` 字段直接取 |
| `MemberCost.display_name` | 覆盖名 | cost-rates.json 的 `displayName`（非空且≠employee）> employee |

---

## 3. 加班拆分规则

```rust
// 每日阈值 = 设置里 workSchedule.periods Σ(end-start)/60，缺省 8h
let threshold = daily_work_hours_from_config();

// 拆分
let is_workday = chinese_holiday::chinese_holiday(&date).is_workday();
let (normal, overtime) = if is_workday {
    (hours.min(threshold), (hours - threshold).max(0.0))
} else {
    (0.0, hours)
};
```

**不变式**：每人 `总工时 == normal + overtime`（同源，无补差）。

---

## 4. `project_cost_summary_inner` 签名

```rust
pub async fn project_cost_summary_inner(
    project_name: &str,
    include_overtime: bool,        // true=拆分正常/加班
    start_date: Option<&str>,      // "YYYY-MM-DD"，缺省→本月1号
    end_date: Option<&str>,        // "YYYY-MM-DD"，缺省→今天
    include_resigned: bool,        // true=含离职（USER_STATUS=""）
) -> Result<CostSummaryResult, String>
```

- 前端 invoke 命令 `project_cost_summary` 同参数，`Option<bool/String>`。
- Tauri command 注册在 `lib.rs`。

---

## 5. 渠道工具注册（关键！）

机器人渠道（QQ/Telegram）接新工具必须在 **4 个点同步注册**：

| 序号 | 位置 | 注册内容 |
|------|------|----------|
| ① | `chat_agent.rs::DEFAULT_AGENT_TOOLS` | 加工具名 |
| ② | `chat_agent.rs::tool_schema()` | 加描述 + JSON Schema |
| ③ | `message_handler.rs::allowed_channel_tools()` | 加工具名 |
| ④ | `message_handler.rs::should_use_agent_tools()` | 加触发关键词 |

缺少任何一处都会导致渠道 agent 拿不到工具或走错路由。

### 成本工具渠道清单

```rust
// allowed_channel_tools() 必须包含：
"cost_report_preview",  // 轻量预览：拉团队成员列表
"cost_report",          // 完整成本报告：含条形图、人均、汇总

// should_use_agent_tools()  keywords 必须包含：
"成本",  // 触发词，用户说"查成本"才能进 agent 模式
```

---

## 6. 两步确认流程

```
用户说"查 XX 项目成本"
  → agent 调 cost_report_preview 拉成员列表
  → agent 展示"你要查的是 XX 项目吗？成员有：张三、李四…"
  → 用户确认
  → agent 调 cost_report(projectName, range/startDate/endDate, includeOvertime, includeResigned)
  → 返回带条形图的文本报告
```

系统 prompt 已在 `chat_agent.rs::default_system_prompt()` 第 6 条写明此流程，禁止跳过预览直接查成本。

---

## 7. `cost_report` 工具参数

```rust
// 来自 cost_report.rs 的 resolve_range 逻辑：
// 1. startDate + endDate 同时给 → 自定义区间（优先级最高）
// 2. range 有值 → 按档计算
// 3. 都缺省 → 本月

// 支持档位：
//   "thisMonth"    → 本月 1 号 ~ 本月最后一天
//   "thisQuarter"  → 本季度 1 号 ~ 季度末
//   "halfYear"     → 近半年（含当前月往前 6 个月 1 号 ~ 今天）
//   "thisYear"     → 今年 1 月 1 号 ~ 12 月 31 号
```
