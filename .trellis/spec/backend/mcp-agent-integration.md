# MCP 工具接入 agent + 动态安全分类

> MCP 工具如何注入 chat agent、按 `mcp__<server>__<tool>` 路由、并被动态安全分类管控。
> 来源：jenkins-deploy PR2，`src-tauri/src/chat_agent.rs` + `src-tauri/src/mcp_client.rs`。
> 前置：transport/client 合约见 [mcp-client.md](./mcp-client.md)。确认流（pending action）属 PR3，另文。

---

## 1. Scope / Trigger

- **触发**：把运行时发现的 MCP 工具接进既有 agent loop——跨「工具发现 / LLM 工具定义 / 执行路由 / 安全门禁」多个边界，属强制 code-spec 深度。
- **关键约束**：发版是高危写操作，**安全第一**。运行时发现的工具无法靠硬编码白名单管控，需通用机制判定 agent 能否直调。

---

## 2. 全局 manager 单例

agent loop 从多处入口被触达（Tauri 命令、channels、`chat_tool` 递归），这些路径都**不串 Tauri State**。沿用本仓库「模块级全局」惯例（如 `settings::CONFIG_WRITE_LOCK`）：

```rust
// mcp_client.rs
static GLOBAL_MANAGER: Lazy<McpClientManager> = Lazy::new(McpClientManager::new);
pub fn manager() -> &'static McpClientManager;   // 各处共享同一句柄
```

- **spawn 时机**：app 启动（`lib.rs` setup）后台 `tauri::async_runtime::spawn` 调 `manager().spawn_all_from_config()`。没配文件 → `Ok([])`，正常启动（无 MCP server，agent 退化为纯 native 工具，**不报错**）。
- `McpClientManager` 内部 `Arc<Mutex>`，`Clone` 廉价；全局只是给各处一个共同句柄。

---

## 3. 命名空间：`mcp__<server>__<tool>`

发现的 MCP 工具注入 agent 时统一加 `mcp__<server>__<tool>` 前缀，避免与 native 工具名/跨 server 工具名冲突。

```rust
// mcp_client.rs
pub const MCP_TOOL_PREFIX: &str = "mcp__";
pub fn namespaced_tool_name(server: &str, tool: &str) -> String;          // "mcp__jenkins__list_jobs"
pub fn parse_namespaced_tool_name(name: &str) -> Option<(String, String)>; // 拆回 (server, tool)
```

- 用**首个** `__` 切出 server，剩余整体为 tool 名（server id 一般是简单 id 如 `jenkins`；tool 名理论上可含 `__`，故不能用 `rsplit`）。
- 非 `mcp__` 前缀 / server 或 tool 空 → `None`。

---

## 4. 工具注入 agent

`build_tool_definitions`（sync，纯 native）保留；新增 async 包装一次性合并 MCP 工具：

```rust
// chat_agent.rs
async fn build_all_tool_definitions(allowed: &[String]) -> Vec<ToolDefinition> {
    let mut out = build_tool_definitions(allowed);               // native（allowed + 内联 schema）
    for (server, tool, _level) in manager().discover_for_agent().await {
        out.push(mcp_tool_to_definition(&server, &tool));        // + MCP（命名空间）
    }
    out
}
```

- **sync→async**：因列 MCP 工具是跨进程 `list_tools`（async）。两个 loop 入口（`run_agent` / `run_agent_streaming`）都在 async 上下文，loop 启动时调一次。
- **rmcp `Tool` → `ToolDefinition`** 字段映射（已对 rmcp 1.7 `Tool` 源码核实）：

| agent 字段 | 来源 | 备注 |
|---|---|---|
| `function.name` | `namespaced_tool_name(server, tool.name)` | `Tool.name: Cow<str>` → `.as_ref()` |
| `function.description` | `tool.description.map(\|d\| d.to_string())` | `Tool.description: Option<Cow>`，缺省空串（agent 字段非 Option） |
| `function.parameters` | `tool.schema_as_json_value()` | `Tool.input_schema: Arc<JsonObject>` → `Value::Object(..)`，正是 JSON Schema |

---

## 5. 动态安全分类（核心）

每个 MCP 工具定一个级别，决定 agent 在 loop 里的待遇：

```rust
// mcp_client.rs
pub enum ToolLevel { Auto, Confirm, Blocked }   // 严格程度 Blocked > Confirm > Auto
pub fn classify_tool(policy: &HashMap<String,String>, tool: &Tool) -> ToolLevel;
```

| 级别 | 语义 |
|---|---|
| `Auto` | 纯只读、安全，agent 可在 loop 内**直接调** |
| `Confirm`（默认） | 写/高危操作，agent **不能直接执行**，需用户确认（确认流是 PR3；PR2 只「拦下、不执行」） |
| `Blocked` | 彻底禁止，agent **永远不能调**（危险工具熔断） |

**级别三来源，就高不就低（strictest-wins，冲突取最严）：**

1. **用户 `toolPolicy`（最高优先）**：`mcp-servers.json` 里 server 级配置，工具名 → 级别，支持 `"*"` 通配兜底。
   - Jenkins：`{"trigger_build":"confirm","cancel_build":"confirm","*":"auto"}`。
   - 值无法解析为合法级别（`auto`/`confirm`/`blocked`）→ 按「未标注」处理，落下一来源。
   - ⚠️ **已知取舍**：`"*":"auto"` 会让该 server **未显式列出**的工具（含 server 日后新增的写工具，如 `delete_job`）一律落 `Auto`、可被 agent 直调。这是用户主动放宽的知情授权，不是漏洞；要更严就别配 `"*":"auto"`（缺省即兜底 `Confirm`），或把通配设为 `confirm` 再逐个降 `auto`。
2. **MCP annotations**（仅 toolPolicy 未命中时采信）：`destructiveHint=true`→`Confirm`、`readOnlyHint=true`→`Auto`（destructive 优先取严）；都无信号 → 落兜底。
3. **兜底**：无 policy 无 annotations → **一律 `Confirm`，绝不默认 `Auto`**。

> **toolPolicy 是最高优先，不会被 annotations「放宽」也不会被「收严」**：用户显式标注 = 知情授权（PRD「用户可主动把只读工具降 auto」）。annotations 只在用户没标注时兜底收严。jenkins-mcp 老 SDK **不发 annotations**，其工具全落到 toolPolicy 或默认 `Confirm`。

**配置形态**（`McpServerConfig` PR2 新增字段，serde camelCase）：

```json
{ "servers": { "jenkins": {
    "command": "node", "args": ["...dist/index.js"], "env": { /* 凭据 */ },
    "toolPolicy": { "trigger_build": "confirm", "cancel_build": "confirm", "*": "auto" }
} } }
```

---

## 6. 执行路由 + 门禁

`execute_tool_call` 在最前面分流：**`mcp__` 前缀 → MCP 路由；否则走原 native 路径**。

```rust
// chat_agent.rs::execute_tool_call
if parse_namespaced_tool_name(&name).is_some() {
    return execute_mcp_tool_call(call).await;   // MCP：不走 native allowed 白名单
}
// ... 原 AGENT_FORBIDDEN_TOOLS / allowed 白名单 / tools::dispatch 不变
```

- **MCP 工具不走 native `allowed` 白名单**（那张表只管 native 工具），改由动态分类管控。这是有意的设计取舍：native 白名单是给手写工具用的，MCP 工具是运行时发现的，二者治理机制不同。
- `execute_mcp_tool_call` 流程：
  1. 拆 `(server, tool)`。
  2. `manager().discover_for_agent()` 取该工具**当前**分类（读 toolPolicy + annotations 定级）。**server 未连接 / 工具不存在 → 拦下**（绝不放行未知工具）。
  3. 门禁 `mcp_gate_refusal(level, name) -> Option<String>`（纯函数，便于单测）：`Auto`→`None`（放行）；`Confirm`/`Blocked`→`Some(拒绝理由)`，作为 tool-result error 回模型。
  4. `Auto` 才真正 `call_tool`：参数转 `Option<Map<String,Value>>`（空/`null`→`None`，非对象→报错）。
  5. **两层错误**（见 mcp-client.md §3.3）：传输错 → `call_tool` 返回 `Err`；工具失败 → `Ok(is_error==Some(true))`，用 `first_text` 取消息回模型。

> **为何执行时再 `discover_for_agent` 一次（而非复用 loop 启动时建的定义）**：门禁必须以**调用时**的分类为准，定义可能已过期。安全优先于省一次 stdio 往返（桌面单用户、调用量低，可接受）。

---

## 7. 安全红线（PR2 验收点）

- **`Confirm`/`Blocked` 一律不在 loop 内直接执行**——与既有 `AGENT_FORBIDDEN_TOOLS`（`log-task-effort`）同级红线。
- PRD 验收：agent 直调只读 `auto` 工具（`list_jobs`）放行；agent 调 `trigger_build` 被拦截**不执行**。
- **PR2 边界**：门禁 = 拦下、回模型「需确认」；**不**实现确认→pending action→执行闭环（那是 PR3）。

---

## 8. Tests Required

- `chat_agent::tests`（纯逻辑，无 live server）：
  - `auto_tool_is_allowed_in_loop` / `confirm_tool_is_refused_in_loop` / `blocked_tool_is_refused_in_loop`——门禁三态。
  - `mcp_tool_converts_to_namespaced_definition`——`Tool`→`ToolDefinition` 字段映射。
- `mcp_client::tests`（纯逻辑）：
  - 分类优先级：`classify_defaults_to_confirm_with_no_policy_no_annotations`、`classify_jenkins_tool_policy`、`classify_policy_overrides_annotations_both_directions`、`classify_annotations_when_no_policy`、`classify_invalid_policy_value_falls_through`。
  - 命名空间 round-trip / 拆分 / 拒绝畸形：`namespaced_name_round_trips`、`parse_namespaced_*`。
  - `tool_policy_parses_from_config_json`——toolPolicy serde。
- `chat_agent::tests::classify_live_jenkins_tools_with_prd_policy`（`#[ignore]`，手动 `--ignored`）：spawn 真 jenkins-mcp，用 PRD 策略对**真实发现**的工具定级——`trigger_build`→Confirm 且被 gate 拦、`list_jobs`→Auto 且放行。
  ```
  cargo test --lib chat_agent::tests::classify_live_jenkins_tools_with_prd_policy -- --ignored --nocapture
  ```

---

## 9. Wrong vs Correct

### 9.1 MCP 工具名不能用 `rsplit` 拆

```rust
// Wrong —— tool 名含 "__" 时 rsplit 把 tool 切碎
let (server, tool) = name.strip_prefix("mcp__")?.rsplit_once("__")?;
// Correct —— 用首个 "__" 切 server，剩余整体是 tool（server id 不含 "__"）
let (server, tool) = name.strip_prefix("mcp__")?.split_once("__")?;
```

### 9.2 默认级别绝不能是 Auto

```rust
// Wrong —— 无标注就放行 = 高危写工具可能被 agent 直接触发
let level = policy_level(policy, tool).unwrap_or(ToolLevel::Auto);
// Correct —— 兜底 Confirm，贴合「写操作永不自动」铁律
// classify_tool: policy → annotations → Confirm
```

### 9.3 门禁要在执行时查，不能只信 loop 启动时的定义

```rust
// Wrong —— 只在 build 定义时分类，执行时不再校验 → 定义过期则绕过门禁
// Correct —— execute_mcp_tool_call 内 discover_for_agent 重新定级再 gate
```
