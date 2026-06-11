# MCP Client 接入（rmcp stdio）

> 通用 MCP（Model Context Protocol）stdio client 的接入规范与合约。
> 来源：jenkins-deploy PR1，`src-tauri/src/mcp_client.rs`。本篇只覆盖 **client 接入**；
> 工具注入 agent、动态安全分类、确认流（`mcp__<server>__<tool>` 路由 / pending action）属 PR2/PR3，另文。

---

## 1. Scope / Trigger

- **触发**：infra 集成 —— spawn 外部子进程、注入凭据 env、跨进程 JSON-RPC 合约。属强制 code-spec 深度。
- **crate**：`rmcp = { version = "1", features = ["client", "transport-child-process"] }`（当前解析 **1.7.0**；pre-1.0 的 0.x 草稿 API 已过时，勿照抄）。
- **模块**：`src-tauri/src/mcp_client.rs`（独立，不依赖 agent/tools）。

---

## 2. Signatures

```rust
// 配置模型（serde camelCase）
pub struct McpServerConfig { pub command: String, pub args: Vec<String>,
                             pub env: HashMap<String,String>, pub enabled: bool /*默认 true*/ }
pub struct McpServersConfig { pub servers: HashMap<String, McpServerConfig> }

pub fn mcp_servers_config_path() -> PathBuf;                 // ~/.jarvis/mcp-servers.json
pub fn load_mcp_servers_config() -> Result<McpServersConfig, String>;

#[derive(Clone, Default)]
pub struct McpClientManager { /* Arc<Mutex<HashMap<String, RunningService<RoleClient,()>>>> */ }
impl McpClientManager {
    pub async fn spawn_server(&self, id: &str, cfg: &McpServerConfig) -> Result<(), String>; // 幂等
    pub async fn spawn_all_from_config(&self) -> Result<Vec<String>, String>;   // 返回成功启动的 id
    pub async fn list_tools(&self, id: &str) -> Result<Vec<Tool>, String>;       // list_all_tools 翻页
    pub async fn call_tool(&self, id: &str, tool: &str,
                           args: Option<Map<String,Value>>) -> Result<CallToolResult, String>;
    pub async fn connected_ids(&self) -> Vec<String>;
    pub async fn shutdown_server(&self, id: &str) -> Result<(), String>;
    pub async fn shutdown_all(&self);
}
pub fn first_text(result: &CallToolResult) -> Option<String>;   // 取第一个 text 块
```

### rmcp 1.7 client 范式（已对 crate 源码核实）

```rust
let transport = TokioChildProcess::new(cmd)?;   // tokio::process::Command 直接 .into()
let running = ().serve(transport).await?;        // () impl ClientHandler；serve 自动跑 initialize 握手
// running: RunningService<RoleClient,()>，Deref 到 Peer<RoleClient>，可直接 running.list_all_tools()/.call_tool()
let tools = running.list_all_tools().await?;     // Vec<Tool>
running.cancel().await?;                          // 优雅关停（消费 self，杀子进程）
```

---

## 3. Contracts

### 3.1 配置文件 `~/.jarvis/mcp-servers.json`

```json
{ "servers": {
    "jenkins": { "command": "node",
                 "args": ["D:\\coding\\my-mcp-servers\\jenkins-mcp\\dist\\index.js"],
                 "env": { "JENKINS_ENV_TEST_URL": "...", "JENKINS_ENV_TEST_USERNAME": "...",
                          "JENKINS_ENV_TEST_TOKEN": "..." },
                 "enabled": true } } }
```

- `enabled` 缺省 `true`；`args`/`env` 缺省空。`env` 在 spawn 时注入子进程（PR3 改成从 keychain 取后注入，配置不落明文）。

### 3.2 env 注入（含 keychain 解析）

`McpServerConfig.env` 的每一项经 `tokio::process::Command::env(k,v)` 注入。**凭据只走 env，spawn 后不可再读**——这是给 MCP server 传密钥的正确通道。

PR3 起，每个 env 值在注入前过 `resolve_env_value(v)`（通用机制，任何 server 适用）：

```rust
// mcp_client.rs —— spawn_running 的 env 循环
let resolved = resolve_env_value(v)?;   // 解析失败即 spawn 错误，spawn_all_from_config 记录后跳过
cmd.env(k, resolved);

fn resolve_env_value(v: &str) -> Result<String, String> {
    match v.strip_prefix("keychain:") {                       // "keychain:jenkins-test-token"
        Some(key) => crate::settings::secret_get(key.trim())  // OS 密钥链取
            .ok_or_else(|| format!("env 引用的 keychain 密钥 '{}' 不存在", key.trim())),
        None => Ok(v.to_string()),                            // 字面值原样
    }
}
```

- `"JENKINS_ENV_TEST_TOKEN": "keychain:jenkins-test-token"` → spawn 时才从密钥链解出注入，**token 不落明文配置**（满足 DoD）。
- **密钥不存在/为空 → `Err`**，绝不静默注入空值（否则 server 拿空 token 静默失败、难排查）。`secret_get` 本身已把空白 token 映射为 `None`。
- 非 `keychain:` 前缀（含恰好 `"keychain"` 无冒号）一律按字面值。
- 发版预设（项目/环境/参数）走另一份配置，见 [mcp-deploy-confirm.md](./mcp-deploy-confirm.md)。

### 3.3 工具结果两层语义

| 层 | 形态 |
|----|------|
| 协议/传输错误（子进程死、方法不存在、JSON-RPC 错） | `call_tool` 返回 `Err(String)` |
| 工具自身失败 | `Ok(CallToolResult { is_error: Some(true), content:[text] })` —— **不是** Rust `Err` |

调用方（PR2/3）必须显式查 `is_error`，再用 `first_text()` 取消息。

---

## 4. Validation & Error Matrix

| 条件 | 行为 |
|------|------|
| 配置文件不存在 | `load_mcp_servers_config` → `Ok(空配置)`（语义：还没配 server，不报错） |
| 配置文件存在但 JSON 坏 | → `Err("解析 ... 失败")`（坏配置显式暴露） |
| `spawn_server` 子进程起不来 / 握手失败 | → `Err("启动 MCP server '<id>' 失败: ...")` |
| **server 无任何凭据 env**（jenkins-mcp 等） | server `parseEnvironments()` 直接 throw 退出 → **握手失败 → Err**。连 `list_tools` 冒烟都要塞 dummy env |
| `spawn_all_from_config` 部分 server 失败 | 失败项 `eprintln!("[mcp_client] ...")` 记录后跳过；**全失败且有错**才返回 `Err`，否则 `Ok(已起列表)` |
| `list_tools`/`call_tool` 指定未连接的 id | → `Err("MCP server '<id>' 未连接")` |

> **非致命错误一律 `eprintln!("[mcp_client] ...")`**（与 `channels/*` 日志风格一致），不得 `let _ =` 静默吞掉——否则配错的 server 静默缺席、难排查。

---

## 5. Good / Base / Bad 用例

- **Good**：配置 1 个 jenkins server（带 env）→ `spawn_all_from_config` → `list_tools("jenkins")` 得 8 个工具。
- **Base**：无配置文件 → `spawn_all_from_config` 返回 `Ok([])`，app 正常启动（无 MCP server）。
- **Bad**：jenkins 配了但漏 env → spawn 报 `Err`，其它 server 不受影响、照常起。

---

## 6. Tests Required（`mcp_client::tests`）

- `config_parses_minimal_shape` / `config_enabled_defaults_true_and_can_be_false` / `empty_config_is_default` —— 断言 serde 缺省值与解析。
- `smoke_jenkins_list_tools`（`#[ignore]`，手动 `--ignored` 跑）—— spawn 真 jenkins-mcp，**断言恰好 8 个工具名**（list_environments/list_jobs/get_job_info/trigger_build/get_build_status/get_build_log/cancel_build/test_connection）。前置：jenkins-mcp 已 `npm run build`、本机有 `node`、塞 dummy `JENKINS_ENV_TEST_*`。
  ```
  cargo test --lib mcp_client::tests::smoke_jenkins_list_tools -- --ignored --nocapture
  ```

---

## 7. Wrong vs Correct

### 7.1 `CallToolRequestParams` 是 `#[non_exhaustive]`

```rust
// Wrong —— 跨 crate 用结构体字面量，编译报 E0639
let p = CallToolRequestParams { name: tool.into(), arguments: args };
// Correct —— 走 builder
let mut p = CallToolRequestParams::new(tool.to_string());
if let Some(a) = args { p = p.with_arguments(a); }
```

### 7.2 必须长持有 `RunningService`，否则子进程被回收

```rust
// Wrong —— 只留 Peer / 让 RunningService 出作用域 drop → transport 关闭 → 子进程被杀
let peer = ().serve(transport).await?.peer().clone();   // running 被 drop！
// Correct —— 把 owned RunningService 存进 manager 的 map，存活期间子进程才在
map.insert(id, running);   // 关停用 running.cancel().await
```

### 7.3 工具失败不会变成 `Err`

```rust
// Wrong —— 以为 Ok 就是成功
let r = mgr.call_tool(id, "trigger_build", args).await?;   // r 可能 is_error==Some(true)
use_result(r);
// Correct
let r = mgr.call_tool(id, "trigger_build", args).await?;
if r.is_error == Some(true) { return Err(first_text(&r).unwrap_or_default()); }
```

---

## 增量更新（2026-06-11）

### spawn：Windows 隐藏控制台窗口

`spawn_running` 构建 `tokio::process::Command` 后，Windows 下加 `CREATE_NO_WINDOW`：

```rust
#[cfg(windows)]
{
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW); // tokio Command 的 inherent 方法，无需 use std trait
}
```

- 原因：Jarvis 是 GUI 应用，spawn 控制台子系统程序（node.exe）默认每次弹一个控制台黑框（保存配置触发重启 server 时尤其扰民）。`stdin/stdout` 管道不受影响。

### call_tool：僵尸条目自愈（transport-dead → 重连重试一次）

子进程可能 spawn 后悄悄退出（崩溃/被杀），但 `RunningService` 仍留在 map——`connected_ids` 只看 key、**不探活**，导致后续调用一直撞死进程报 `Transport closed`，卡到重启整个 app。

新契约：`call_tool` 首次失败若是**传输已断**（`ServiceError::TransportClosed | TransportSend`，见 `is_transport_dead`），则**剔除死条目（`shutdown_server`）→ 按 mcp-servers.json 重新 spawn → 重试一次**；重试仍失败如实返回，绝不无限重连。工具自身失败（`is_error`）**不触发**自愈。

| 条件 | 行为 |
|---|---|
| call 命中 `TransportClosed` / `TransportSend` | 剔除 + 重 spawn + 重试一次 |
| 重连后仍传输错 | `Err("...重连后传输仍不可用...")` |
| 配置中已无该 server | `Err("...无法重连")` |
| 协议错 / 超时 / 工具 `is_error` | **不**自愈（如实返回 / 走 §3.3 两层语义） |

> **Wrong**：`connected_ids` 报在线就直接 call → 死进程上反复 `Transport closed`。
> **Correct**：`call_tool` 内置「死了就重连重试一次」，对调用方透明（实现为 `call_tool_once` + `is_transport_dead` 分类）。

---

## Design Decision：最小配置模型

PR1 只建模 `command/args/env/enabled`。PR2 加了 `toolPolicy`（动态安全分类，见
[mcp-agent-integration.md](./mcp-agent-integration.md)）；账号·项目参数预设仍随 PR3 的确认流一起加，
避免过度设计。`McpClientManager` 用 `Arc<Mutex<..>>` 包裹是为放进共享状态多处 clone 持有铺路
（PR2 已落地为 `once_cell::Lazy` 全局单例 `mcp_client::manager()`）。
