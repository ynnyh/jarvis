# FineReport Integration

> FineReport (帆软决策平台) 接入规范与合约。

---

## 1. 客户端构建 —— Timeout 分级

`FineReportClient` 有两个构造函数：

| 方法 | 超时 | 用途 |
|------|------|------|
| `new(base_url, account, password)` | 10s | 日报、chat 查询（数据量小，超时即报错定位快） |
| `new_with_timeout(base_url, account, password, secs)` | 自定义 | 成本/团队人员等大报表（all_people + 长周期 → 60s） |

```rust
// 日报路径（10s）
let client = FineReportClient::new(cred.base_url, cred.account, cred.password)?;

// 成本分析路径（60s）
let client = FineReportClient::new_with_timeout(cred.base_url, cred.account, cred.password, 60)?;
```

**规则**：`finereport_get_efforts`（日报/chat）走 10s；`finereport_get_efforts_raw`（成本/团队人员）走 60s。不得混用。

---

## 2. `submit_filter` 参数合约

```rust
pub async fn submit_filter(
    &self,
    jwt: &str,
    session_id: &str,
    begin: &str,        // "YYYY-MM-DD 00:00:00"，自动补时间边界
    end: &str,          // "YYYY-MM-DD 23:59:59"
    real_name: &str,    // 空→[]（全部），有值→["姓名"]（JSON 数组）
    project_name: &str, // 空→不限项目，有值→PJ_NAME 粗筛
    user_status: &str,  // "0"=仅在职，""=全部（含离职）
) -> Result<(), String>
```

### 2.1 时间边界补全

FR 的 `workdate` 是 `datetime` 列，纯 `yyyy-MM-dd` 会被解为 `00:00:00`，导致 `>= begin AND <= end` 同日变零宽区间。

```rust
let begin_full = format!("{} 00:00:00", begin);
let end_full   = format!("{} 23:59:59", end);
```

### 2.2 REAL_NAME 必须是 JSON 数组

FR 多选下拉控件期望真正的 JSON 数组 `["姓名"]`，**不是**字符串化的 `"[\"姓名\"]"`。

```rust
let real_name_field = if real_name.is_empty() { json!([]) } else { json!([real_name]) };
```

### 2.3 PJ_NAME 仅粗筛

FR 层按项目名过滤，Rust 端仍需 `record.project_name == project_name` 精确兜底（FR SQL 可能含模糊匹配或大小写问题）。

---

## 3. 报表调用链

```
login() → JWT
  → open_report_and_get_session(jwt, viewlet) → sessionID (UUID)
  → generate_cid(&sessionID) → cid (客户端生成的 32-hex#ms#8-hex token)
  → submit_filter(jwt, sessionID, begin, end, realName, projectName, userStatus)
  → fetch_report_html(jwt, sessionID, cid, reportIndex)
      reportIndex=0 → 禅道工时汇总
      reportIndex=1 → 禅道工时任务完成明细 (EffortRecord)
```

### 关键约束

1. **cid 不在 HTML 里**——客户端用 `MD5(sessionID + ts + seed)` 生成 32-hex 串，FR 服务端第一次见到时建立 `(sessionID, cid)` → state 映射。
2. **sessionID** 从 HTML 抠——优先 `FR.SessionMgr.register('<uuid>', ...)`，兜底 `currentSessionID = '<uuid>'`，再兜底任意 UUID。
3. JWT 缓存在 `~/.jarvis/finereport.json`，剩 <30 分钟自动续期。

---

## 4. `finereport_get_efforts_raw` 合约

内部函数（非 tauri command），供成本/团队人员模块调用。

```rust
pub async fn finereport_get_efforts_raw(
    begin: String,
    end: String,
    real_name: Option<String>,  // None → all_people? 全部门 : 回退 config
    all_people: bool,           // true=不传 realName 过滤
    project_name: Option<String>, // None/""=不限项目
    user_status: &str,          // "0"=在职，""=含离职
) -> Result<Vec<EffortRecord>, String>
```

- `all_people=true` 时 `real_name` 被忽略（传空串），拉全部门数据。
- 内部固定用 60s 超时 `new_with_timeout`。
