// 通用 MCP（Model Context Protocol）stdio client 管理器。
//
// 这是 jenkins-deploy 任务 PR1 的交付：一个**独立**的 MCP client 核心，
// 还不接 agent / tools / pending_actions（那是 PR2/PR3）。职责：
//   1. 读 ~/.jarvis/mcp-servers.json（多个 MCP server 配置）
//   2. 用 rmcp 的 child-process transport spawn stdio 子进程（spawn 时注入 env）
//   3. 走 initialize 握手（rmcp 的 serve API 自动完成，见下）
//   4. 暴露 list_tools / call_tool
//   5. 把 RunningService 长期持有在共享状态里，保证子进程不被回收
//
// ---- rmcp 1.7.0 API 备忘（已对着 crate 源码核实，区别于调研里的 0.x 草稿）----
//   - 起会话：`().serve(transport).await` —— `()` 实现了 `ClientHandler`，
//     `ClientHandler` 又 blanket-impl 了 `Service<RoleClient>`，`ServiceExt::serve`
//     在内部完成 initialize 请求 + initialized 通知，返回 `RunningService<RoleClient, ()>`。
//     （0.x 调研担心要手动 initialize —— 不用，serve 返回 Ok 即握手成功。）
//   - transport：`rmcp::transport::TokioChildProcess::new(impl Into<CommandWrap>)`，
//     `tokio::process::Command` 可直接 `.into()`。默认 stdin/stdout piped、
//     stderr inherit（子进程 stderr 直接打到 Jarvis 控制台，方便诊断）。
//   - 句柄：`RunningService<RoleClient, ()>` Deref 到 `Peer<RoleClient>`，
//     所以可直接 `running.list_tools(..)` / `running.call_tool(..)`。
//     `Peer` 是 `Clone + Send + Sync`（内部就是 mpsc sender + Arc），整个
//     `RunningService` 也 `Send + Sync`，可安心塞进 tokio 共享状态。
//   - list：`list_tools(Option<PaginatedRequestParams>)` 返回 `ListToolsResult`，
//     或便捷的 `list_all_tools()` 翻页拿全量 `Vec<Tool>`。
//   - call：`call_tool(CallToolRequestParams { name, arguments, .. })`，
//     `arguments: Option<serde_json::Map<String, Value>>`，返回 `CallToolResult`。
//   - 关停：`RunningService::cancel(self).await` 显式优雅关停（杀子进程）；
//     drop 也会异步关闭，但显式 cancel 更干净。
//
// ---- jenkins-mcp 接入注意（已读其 src/index.ts 源码核实）----
//   - 启动命令：`node <jenkins-mcp>/dist/index.js`（需先 `npm run build`）。
//   - 若 **没有** 任何 Jenkins env（JENKINS_ENV_*/JENKINS_TEST_*/...），
//     server 的 parseEnvironments() 直接 throw 退出 —— 连 list_tools 冒烟都得
//     在 spawn 时塞至少一组 dummy JENKINS_ENV_TEST_URL/USERNAME/TOKEN，否则握手失败。
//   - 工具失败走 `Ok(CallToolResult { is_error: Some(true), .. })`（文本块里带
//     错误信息），不是 Rust `Err`。两层都要处理。

#![allow(dead_code)] // PR1 独立交付，部分 API 待 PR2/PR3 接入 agent 时才被调用

use std::collections::HashMap;
use std::sync::Arc;

use once_cell::sync::Lazy;
use rmcp::model::{CallToolRequestParams, CallToolResult, Tool};
use rmcp::service::{RoleClient, RunningService};
use rmcp::transport::TokioChildProcess;
use rmcp::ServiceExt;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tokio::process::Command;
use tokio::sync::Mutex;

use crate::settings::jarvis_dir;

/// 单次 MCP 请求（list_tools / call_tool）的硬超时。
///
/// rmcp 默认 `PeerRequestOptions::no_options()` 不带 timeout：子进程若**活着但不回**
/// （卡死、死循环），`list_all_tools().await` / `call_tool().await` 会**永久挂起**。
/// 这两个调用都发生在 agent loop 内（loop 启动列工具 + 每次 MCP 调用前重新分类），
/// 一旦挂死整个对话就卡住。故在管理器侧统一包一层 `tokio::time::timeout` 兜底，
/// 超时按普通错误返回，让 agent 退化（少这个 server 的工具）而非冻结。
const MCP_REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

/// 已连接的 MCP server 句柄类型别名。
///
/// 持有 `RunningService` 本体（而非只留 `Peer`）—— 一旦 drop，rmcp 会关闭
/// transport 并杀掉子进程。所以管理器必须把它存活在 map 里。
type Running = RunningService<RoleClient, ()>;

// ============================================================================
// 配置模型：~/.jarvis/mcp-servers.json
// ============================================================================
//
// PR1 用最小形态，够 spawn 即可。PR2 加了 `toolPolicy`（动态安全分类）；
// 账号/项目参数预设仍留给 PR3，避免过度设计。

/// 单个 MCP server 的 spawn 配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerConfig {
    /// 可执行命令，如 "node"。
    pub command: String,
    /// 命令参数，如 ["D:\\...\\jenkins-mcp\\dist\\index.js"]。
    #[serde(default)]
    pub args: Vec<String>,
    /// spawn 时注入子进程的环境变量（Jenkins 凭据等敏感值走这里，
    /// PR3 会改成从 keychain 取后注入；PR1 直接读配置里的明文/占位）。
    #[serde(default)]
    pub env: HashMap<String, String>,
    /// 是否启用。默认 true；置 false 则 spawn_all 跳过。
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// 工具安全策略（PR2 新增）：工具名 → 级别（"auto"/"confirm"/"blocked"）。
    /// 支持 "*" 通配兜底。最高优先级，覆盖 annotations。
    /// 如 Jenkins：`{"trigger_build":"confirm","cancel_build":"confirm","*":"auto"}`。
    #[serde(default)]
    pub tool_policy: HashMap<String, String>,
}

fn default_true() -> bool {
    true
}

/// mcp-servers.json 根结构：server id → 配置。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServersConfig {
    #[serde(default)]
    pub servers: HashMap<String, McpServerConfig>,
}

/// 配置文件路径：~/.jarvis/mcp-servers.json
pub fn mcp_servers_config_path() -> std::path::PathBuf {
    jarvis_dir().join("mcp-servers.json")
}

/// 读 ~/.jarvis/mcp-servers.json。文件不存在 → 返回空配置（不报错，
/// 表示"还没配任何 MCP server"，与 settings::load_raw_config 的宽容语义一致）。
/// 文件存在但解析失败 → 报错（坏配置应当显式暴露，而非静默吞掉）。
pub fn load_mcp_servers_config() -> Result<McpServersConfig, String> {
    let path = mcp_servers_config_path();
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(McpServersConfig::default());
        }
        Err(e) => return Err(format!("读取 {} 失败: {}", path.display(), e)),
    };
    serde_json::from_str(&content)
        .map_err(|e| format!("解析 {} 失败: {}", path.display(), e))
}

// ============================================================================
// 动态工具安全分类（PR2）
// ============================================================================
//
// 运行时发现的 MCP 工具无法靠硬编码白名单管控，需要通用机制判定 agent 能否直调。
// 每个工具定一个级别，决定它在 agent loop 里的待遇：
//   - auto：纯只读、安全，agent 可在 loop 内直接调。
//   - confirm（默认）：写/高危操作，agent 不能直接执行，必须走用户确认（PR3 实现确认流；
//     PR2 只负责"拦下、不执行"）。
//   - blocked：彻底禁止，agent 永远不能调（危险工具熔断）。
//
// 级别从三处来源定，**就高不就低**（strictest-wins，冲突取最严）：
//   1. 用户在 mcp-servers.json 的 toolPolicy 显式标注（最高优先，含 "*" 通配）。
//   2. MCP annotations（server 提供时）：readOnlyHint=true 倾向 auto、destructiveHint=true 倾向 confirm。
//   3. 兜底：无 policy 无 annotations → 一律 confirm，绝不默认 auto。
//
// 理由：贴合 Jarvis「写操作永不自动 + 用户确认」铁律；不依赖 server 是否给 annotations
// （jenkins-mcp 老 SDK 没有 annotations，其工具全落到 toolPolicy 或默认 confirm）。

/// 工具安全级别。严格程度 blocked > confirm > auto（数值越大越严）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ToolLevel {
    /// agent 可在 loop 内直接调用（纯只读工具）。
    Auto,
    /// agent 不能直接执行，需用户确认（写操作默认级别）。
    Confirm,
    /// agent 永远不能调用（熔断）。
    Blocked,
}

impl ToolLevel {
    /// 从配置字符串解析。无法识别 → None（交给调用方走更低优先级来源）。
    fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(ToolLevel::Auto),
            "confirm" => Some(ToolLevel::Confirm),
            "blocked" => Some(ToolLevel::Blocked),
            _ => None,
        }
    }

    /// agent 是否可以在 loop 内直接执行此级别的工具。
    pub fn is_auto(self) -> bool {
        matches!(self, ToolLevel::Auto)
    }
}

/// 从 toolPolicy 查某个工具的显式级别：先精确匹配工具名，再退到 "*" 通配。
/// 值无法解析为合法级别时按"未标注"处理（返回 None）。
fn policy_level(policy: &HashMap<String, String>, tool: &str) -> Option<ToolLevel> {
    policy
        .get(tool)
        .or_else(|| policy.get("*"))
        .and_then(|s| ToolLevel::parse(s))
}

/// 从 MCP annotations 推断级别（仅作参考，且只在 toolPolicy 未命中时采信）。
///   - readOnlyHint=true → Auto 倾向
///   - destructiveHint=true → Confirm 倾向
/// 两者都没有有意义信号 → None（落到默认 confirm）。
fn annotation_level(tool: &Tool) -> Option<ToolLevel> {
    let ann = tool.annotations.as_ref()?;
    // destructive 优先：只读但破坏性的极端情况下取严。
    if ann.destructive_hint == Some(true) {
        return Some(ToolLevel::Confirm);
    }
    if ann.read_only_hint == Some(true) {
        return Some(ToolLevel::Auto);
    }
    None
}

/// 综合判定一个 MCP 工具的安全级别。严格"就高不就低"：
///   1. toolPolicy 显式标注（最高优先，直接采用，不再被 annotations 放宽）。
///   2. 否则采信 annotations（若 server 给了）。
///   3. 否则兜底 **Confirm**（绝不默认 Auto）。
///
/// 注：当 toolPolicy 把某工具标成 auto，而 annotations 说 destructive 时，仍以
/// toolPolicy 为准（用户显式标注代表知情授权，是最高优先级，符合 PRD「用户可主动把
/// 只读工具降 auto」）。annotations 只在用户没标注时兜底收严。
pub fn classify_tool(policy: &HashMap<String, String>, tool: &Tool) -> ToolLevel {
    if let Some(level) = policy_level(policy, tool.name.as_ref()) {
        return level;
    }
    if let Some(level) = annotation_level(tool) {
        return level;
    }
    ToolLevel::Confirm
}

// ============================================================================
// 全局 McpClientManager 单例（PR2）
// ============================================================================
//
// agent loop 从多处入口被触达（Tauri 命令、channels、chat_tool 递归），且这些路径都
// 不串 Tauri State。沿用本仓库的「模块级全局」惯例（如 settings::CONFIG_WRITE_LOCK），
// 用 once_cell::Lazy 持有一个全局 manager。`McpClientManager` 内部是 Arc<Mutex>，
// Clone 廉价，全局只是给各处一个共同句柄。

static GLOBAL_MANAGER: Lazy<McpClientManager> = Lazy::new(McpClientManager::new);

/// 取全局 MCP client 管理器。app 启动时 spawn_all_from_config，之后各处共享。
pub fn manager() -> &'static McpClientManager {
    &GLOBAL_MANAGER
}

// ============================================================================
// McpClientManager：spawn + 持有 + list_tools + call_tool
// ============================================================================

/// 通用 MCP client 管理器。
///
/// 用 `tokio::sync::Mutex`（不是 std）守护 server map，因为所有读写都在
/// 持锁期间 `await`（spawn / list / call 都是异步）。`Arc` 包一层方便后续
/// PR2 放进 Tauri 共享状态多处 clone 持有。
#[derive(Clone, Default)]
pub struct McpClientManager {
    servers: Arc<Mutex<HashMap<String, Running>>>,
}

impl McpClientManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// 按 server id spawn 一个 stdio MCP 子进程并完成 initialize 握手。
    ///
    /// 已存在同名连接 → 直接返回（幂等，不重复 spawn）。
    pub async fn spawn_server(&self, id: &str, cfg: &McpServerConfig) -> Result<(), String> {
        {
            let map = self.servers.lock().await;
            if map.contains_key(id) {
                return Ok(());
            }
        }

        let running = spawn_running(cfg)
            .await
            .map_err(|e| format!("启动 MCP server '{}' 失败: {}", id, e))?;

        let mut map = self.servers.lock().await;
        // 双检：持锁 spawn 期间可能有并发 spawn 同 id 抢先插入。后到的关掉自己。
        if map.contains_key(id) {
            drop(map);
            if let Err(e) = running.cancel().await {
                // cancel 失败不致命（Drop 仍会异步关停子进程），但记一笔便于排查。
                eprintln!("[mcp_client] 关停重复 spawn 的 '{}' 失败: {}", id, e);
            }
            return Ok(());
        }
        map.insert(id.to_string(), running);
        Ok(())
    }

    /// 读配置并 spawn 所有 enabled 的 server。返回成功 spawn 的 server id 列表。
    /// 单个 server 失败不阻断其它（逐个打日志，但至少把能起的起起来）。
    pub async fn spawn_all_from_config(&self) -> Result<Vec<String>, String> {
        let cfg = load_mcp_servers_config()?;
        let mut started = Vec::new();
        let mut errors = Vec::new();
        for (id, server_cfg) in &cfg.servers {
            if !server_cfg.enabled {
                continue;
            }
            match self.spawn_server(id, server_cfg).await {
                Ok(()) => started.push(id.clone()),
                Err(e) => {
                    // 单个失败不阻断其它，但必须可见（与 channels/* 的日志风格一致），
                    // 否则配错的 server 会静默缺席、难排查。
                    eprintln!("[mcp_client] {}", e);
                    errors.push(e);
                }
            }
        }
        if started.is_empty() && !errors.is_empty() {
            return Err(errors.join("; "));
        }
        Ok(started)
    }

    /// 列出某个已连接 server 的全部工具（自动翻页）。
    pub async fn list_tools(&self, id: &str) -> Result<Vec<Tool>, String> {
        // 只在持锁期间 clone 出 Peer（Clone 廉价：内部就是 mpsc sender + Arc），
        // 随即释放锁再做 stdio 往返——绝不持锁 await，否则一个卡死的 server
        // 会把整个管理器锁死（后续所有 list/call/discover 全堵）。
        let peer = {
            let map = self.servers.lock().await;
            map.get(id)
                .ok_or_else(|| format!("MCP server '{}' 未连接", id))?
                .peer()
                .clone()
        };
        // 超时兜底：活着但不回的子进程不会让 list_all_tools 永久挂起。
        match tokio::time::timeout(MCP_REQUEST_TIMEOUT, peer.list_all_tools()).await {
            Ok(r) => r.map_err(|e| format!("list_tools('{}') 失败: {}", id, e)),
            Err(_) => Err(format!(
                "list_tools('{}') 超时（{}s 未响应）",
                id,
                MCP_REQUEST_TIMEOUT.as_secs()
            )),
        }
    }

    /// 调用某个已连接 server 的某个工具。
    ///
    /// 注意两层错误：
    ///   - 协议/传输层错误（子进程死了、方法不存在等）→ 本函数返回 `Err(String)`。
    ///   - 工具自身失败 → 返回 `Ok(CallToolResult { is_error: Some(true), .. })`，
    ///     调用方需自己检查 `is_error`（jenkins-mcp 失败就是这么回的）。
    pub async fn call_tool(
        &self,
        id: &str,
        tool_name: &str,
        arguments: Option<Map<String, Value>>,
    ) -> Result<CallToolResult, String> {
        // 同 list_tools：持锁只为 clone Peer，stdio 往返不持锁，并加超时兜底。
        let peer = {
            let map = self.servers.lock().await;
            map.get(id)
                .ok_or_else(|| format!("MCP server '{}' 未连接", id))?
                .peer()
                .clone()
        };
        // CallToolRequestParams 是 #[non_exhaustive]，跨 crate 不能用结构体字面量，
        // 走 new()/with_arguments() builder。
        let mut params = CallToolRequestParams::new(tool_name.to_string());
        if let Some(args) = arguments {
            params = params.with_arguments(args);
        }
        match tokio::time::timeout(MCP_REQUEST_TIMEOUT, peer.call_tool(params)).await {
            Ok(r) => r.map_err(|e| format!("call_tool('{}/{}') 失败: {}", id, tool_name, e)),
            Err(_) => Err(format!(
                "call_tool('{}/{}') 超时（{}s 未响应）",
                id,
                tool_name,
                MCP_REQUEST_TIMEOUT.as_secs()
            )),
        }
    }

    /// 当前已连接的 server id 列表。
    pub async fn connected_ids(&self) -> Vec<String> {
        let map = self.servers.lock().await;
        map.keys().cloned().collect()
    }

    /// 聚合所有已连接 server 的工具，给 agent 用。
    ///
    /// 对每个连接的 server：读其在 mcp-servers.json 里的 toolPolicy，list_tools，
    /// 再逐工具 classify_tool 定级。返回 `(server_id, Tool, ToolLevel)` 列表，
    /// 供 agent 一次性构建工具定义 + 分类查询表（避免每次调用都读配置/列工具）。
    ///
    /// 单个 server 列工具失败不阻断其它（打日志后跳过）。没有任何已连接 server →
    /// 返回空 Vec（不报错，agent 退化为只用 native 工具）。
    pub async fn discover_for_agent(&self) -> Vec<(String, Tool, ToolLevel)> {
        let ids = self.connected_ids().await;
        if ids.is_empty() {
            return Vec::new();
        }
        // 读配置拿各 server 的 toolPolicy。配置读不出来时退化为空策略（全落默认 confirm）。
        let cfg = load_mcp_servers_config().unwrap_or_default();
        let mut out = Vec::new();
        for id in ids {
            let tools = match self.list_tools(&id).await {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("[mcp_client] {}", e);
                    continue;
                }
            };
            let policy = cfg
                .servers
                .get(&id)
                .map(|c| c.tool_policy.clone())
                .unwrap_or_default();
            for tool in tools {
                let level = classify_tool(&policy, &tool);
                out.push((id.clone(), tool, level));
            }
        }
        out
    }

    /// 关停并移除某个 server（优雅 cancel → 杀子进程）。
    pub async fn shutdown_server(&self, id: &str) -> Result<(), String> {
        let running = {
            let mut map = self.servers.lock().await;
            map.remove(id)
        };
        if let Some(running) = running {
            running
                .cancel()
                .await
                .map_err(|e| format!("关停 MCP server '{}' 失败: {}", id, e))?;
        }
        Ok(())
    }

    /// 关停全部 server。
    pub async fn shutdown_all(&self) {
        let drained: Vec<(String, Running)> = {
            let mut map = self.servers.lock().await;
            map.drain().collect()
        };
        for (id, running) in drained {
            if let Err(e) = running.cancel().await {
                eprintln!("[mcp_client] 关停 '{}' 失败: {}", id, e);
            }
        }
    }
}

/// 根据配置构造 `tokio::process::Command`（注入 env），包进 rmcp transport，
/// `serve` 起会话（含 initialize 握手），返回长期持有的 `RunningService`。
async fn spawn_running(cfg: &McpServerConfig) -> Result<Running, String> {
    let mut cmd = Command::new(&cfg.command);
    cmd.args(&cfg.args);
    for (k, v) in &cfg.env {
        // 每个 env 值先过 resolve_env_value：`keychain:` 前缀的从 OS 密钥链取，
        // 让 Jenkins token 等敏感值不落明文配置（满足 DoD「token 不出现在明文配置」）。
        // 解析失败（引用的密钥不存在）→ 直接返回 spawn 错误，由 spawn_all_from_config
        // eprintln 记录后跳过该 server，绝不静默 let _ = 吞掉。
        let resolved = resolve_env_value(v)?;
        cmd.env(k, resolved);
    }

    let transport =
        TokioChildProcess::new(cmd).map_err(|e| format!("spawn 子进程失败: {}", e))?;

    // `().serve(transport)` 会自动跑 initialize 握手；返回 Ok 即握手成功、可用。
    ()
        .serve(transport)
        .await
        .map_err(|e| format!("MCP initialize 握手失败: {}", e))
}

/// 解析 env 值：`keychain:<key>` 形态从 OS 密钥链取对应密钥；其余原样返回。
///
/// 通用机制——任何 MCP server 都能把敏感值（token/密码）放进密钥链，配置里只写
/// `"JENKINS_ENV_TEST_TOKEN": "keychain:jenkins-test-token"`，spawn 时才解出注入子进程。
/// 引用的密钥不存在/为空 → Err（绝不静默注入空值，否则 server 拿空 token 静默失败、难排查）。
fn resolve_env_value(v: &str) -> Result<String, String> {
    match v.strip_prefix("keychain:") {
        Some(key) => {
            let key = key.trim();
            crate::settings::secret_get(key)
                .ok_or_else(|| format!("env 引用的 keychain 密钥 '{}' 不存在", key))
        }
        None => Ok(v.to_string()),
    }
}

/// 从 `CallToolResult` 取第一个文本块（jenkins-mcp 每个结果就是单个 text 块）。
///
/// `CallToolResult.content: Vec<Content>`，`Content = Annotated<RawContent>`，
/// Deref 到 `RawContent`，故 `c.as_text()` 经 deref 命中 `RawContent::as_text()`，
/// 拿到 `&RawTextContent`（其 `.text: String`）。供 PR2/PR3 复用。
pub fn first_text(result: &CallToolResult) -> Option<String> {
    result
        .content
        .iter()
        .find_map(|c| c.as_text().map(|t| t.text.clone()))
}

// ============================================================================
// 命名空间工具名：mcp__<server>__<tool>（PR2）
// ============================================================================
//
// 发现的 MCP 工具注入 agent 时统一加 `mcp__<server>__<tool>` 前缀，避免与 native
// 工具名/跨 server 工具名冲突；agent loop 见到 `mcp__` 前缀即路由到 McpClientManager。
// server id 本身不含 "__"（mcp-servers.json 的 key 一般是简单 id 如 "jenkins"），
// 故用首个 "__" 切出 server、剩余整体为 tool 名（tool 名理论上可含 "__"）。

pub const MCP_TOOL_PREFIX: &str = "mcp__";

/// 拼出命名空间工具名：("jenkins","list_jobs") → "mcp__jenkins__list_jobs"。
pub fn namespaced_tool_name(server: &str, tool: &str) -> String {
    format!("{}{}__{}", MCP_TOOL_PREFIX, server, tool)
}

/// 把命名空间工具名拆回 (server, tool)。非 `mcp__` 前缀或缺第二段分隔符 → None。
pub fn parse_namespaced_tool_name(name: &str) -> Option<(String, String)> {
    let rest = name.strip_prefix(MCP_TOOL_PREFIX)?;
    let (server, tool) = rest.split_once("__")?;
    if server.is_empty() || tool.is_empty() {
        return None;
    }
    Some((server.to_string(), tool.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_parses_minimal_shape() {
        let json = r#"{
            "servers": {
                "jenkins": {
                    "command": "node",
                    "args": ["/path/to/jenkins-mcp/dist/index.js"],
                    "env": { "JENKINS_ENV_TEST_URL": "http://x" }
                }
            }
        }"#;
        let cfg: McpServersConfig = serde_json::from_str(json).unwrap();
        let jenkins = cfg.servers.get("jenkins").expect("jenkins server");
        assert_eq!(jenkins.command, "node");
        assert_eq!(jenkins.args.len(), 1);
        assert_eq!(
            jenkins.env.get("JENKINS_ENV_TEST_URL").map(String::as_str),
            Some("http://x")
        );
        // enabled 默认 true
        assert!(jenkins.enabled);
    }

    #[test]
    fn config_enabled_defaults_true_and_can_be_false() {
        let json = r#"{ "servers": { "a": { "command": "x", "enabled": false } } }"#;
        let cfg: McpServersConfig = serde_json::from_str(json).unwrap();
        assert!(!cfg.servers.get("a").unwrap().enabled);
        assert!(cfg.servers.get("a").unwrap().args.is_empty());
        assert!(cfg.servers.get("a").unwrap().env.is_empty());
    }

    #[test]
    fn empty_config_is_default() {
        let cfg: McpServersConfig = serde_json::from_str("{}").unwrap();
        assert!(cfg.servers.is_empty());
    }

    // ---- 命名空间工具名 round-trip / 拆分 ----

    #[test]
    fn namespaced_name_round_trips() {
        let n = namespaced_tool_name("jenkins", "list_jobs");
        assert_eq!(n, "mcp__jenkins__list_jobs");
        assert_eq!(
            parse_namespaced_tool_name(&n),
            Some(("jenkins".to_string(), "list_jobs".to_string()))
        );
    }

    #[test]
    fn parse_namespaced_handles_tool_name_with_double_underscore() {
        // server id 用首个 "__" 切出，剩余整体是 tool 名（可含 "__"）。
        assert_eq!(
            parse_namespaced_tool_name("mcp__jenkins__weird__tool"),
            Some(("jenkins".to_string(), "weird__tool".to_string()))
        );
    }

    #[test]
    fn parse_namespaced_rejects_non_mcp_and_malformed() {
        assert_eq!(parse_namespaced_tool_name("get_tasks"), None); // 非 mcp__ 前缀
        assert_eq!(parse_namespaced_tool_name("mcp__jenkins"), None); // 缺 tool 段
        assert_eq!(parse_namespaced_tool_name("mcp____list"), None); // server 空
        assert_eq!(parse_namespaced_tool_name("mcp__jenkins__"), None); // tool 空
    }

    // ---- 动态安全分类：toolPolicy > annotations > 默认 confirm，就高不就低 ----

    fn tool_named(name: &str) -> Tool {
        Tool::new(
            name.to_string(),
            "test".to_string(),
            Arc::new(Map::new()),
        )
    }

    fn tool_with_annotations(name: &str, read_only: Option<bool>, destructive: Option<bool>) -> Tool {
        let ann = rmcp::model::ToolAnnotations::from_raw(None, read_only, destructive, None, None);
        tool_named(name).with_annotations(ann)
    }

    fn policy(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn classify_defaults_to_confirm_with_no_policy_no_annotations() {
        // jenkins-mcp 工具就是这种：无 annotations、无 toolPolicy → 兜底 confirm，绝不 auto。
        let t = tool_named("trigger_build");
        assert_eq!(classify_tool(&HashMap::new(), &t), ToolLevel::Confirm);
    }

    #[test]
    fn classify_jenkins_tool_policy() {
        // PRD 给的 Jenkins 策略：写工具 confirm、其余 "*" auto。
        let pol = policy(&[
            ("trigger_build", "confirm"),
            ("cancel_build", "confirm"),
            ("*", "auto"),
        ]);
        assert_eq!(classify_tool(&pol, &tool_named("trigger_build")), ToolLevel::Confirm);
        assert_eq!(classify_tool(&pol, &tool_named("cancel_build")), ToolLevel::Confirm);
        // 只读工具命中 "*" 通配 → auto，agent 可直调。
        assert_eq!(classify_tool(&pol, &tool_named("list_jobs")), ToolLevel::Auto);
        assert_eq!(classify_tool(&pol, &tool_named("get_build_status")), ToolLevel::Auto);
    }

    #[test]
    fn classify_policy_overrides_annotations_both_directions() {
        // toolPolicy 是最高优先：即便 annotations 说 destructive，policy=auto 仍 auto
        // （用户显式标注 = 知情授权；PRD「用户可主动把只读工具降 auto」）。
        let t = tool_with_annotations("x", Some(false), Some(true));
        assert_eq!(classify_tool(&policy(&[("x", "auto")]), &t), ToolLevel::Auto);
        // 反向：annotations 说 read_only，但 policy=confirm → 仍 confirm（取严由 policy 定）。
        let t2 = tool_with_annotations("y", Some(true), None);
        assert_eq!(classify_tool(&policy(&[("y", "confirm")]), &t2), ToolLevel::Confirm);
        // policy=blocked 最严。
        let t3 = tool_with_annotations("z", Some(true), None);
        assert_eq!(classify_tool(&policy(&[("z", "blocked")]), &t3), ToolLevel::Blocked);
    }

    #[test]
    fn classify_annotations_when_no_policy() {
        // 无 policy 时采信 annotations：readOnlyHint → auto。
        let ro = tool_with_annotations("a", Some(true), None);
        assert_eq!(classify_tool(&HashMap::new(), &ro), ToolLevel::Auto);
        // destructiveHint → confirm（即便也标了 read_only，destructive 优先取严）。
        let de = tool_with_annotations("b", Some(true), Some(true));
        assert_eq!(classify_tool(&HashMap::new(), &de), ToolLevel::Confirm);
        // 有 annotations 但无 read_only/destructive 信号 → 仍兜底 confirm。
        let empty_ann = tool_with_annotations("c", None, None);
        assert_eq!(classify_tool(&HashMap::new(), &empty_ann), ToolLevel::Confirm);
    }

    #[test]
    fn classify_invalid_policy_value_falls_through() {
        // toolPolicy 写了无法识别的值 → 视为未标注，落到下一来源（这里无 annotations → confirm）。
        let pol = policy(&[("trigger_build", "garbage")]);
        assert_eq!(classify_tool(&pol, &tool_named("trigger_build")), ToolLevel::Confirm);
    }

    #[test]
    fn tool_policy_parses_from_config_json() {
        let json = r#"{
            "servers": {
                "jenkins": {
                    "command": "node",
                    "toolPolicy": { "trigger_build": "confirm", "*": "auto" }
                }
            }
        }"#;
        let cfg: McpServersConfig = serde_json::from_str(json).unwrap();
        let jenkins = cfg.servers.get("jenkins").unwrap();
        assert_eq!(jenkins.tool_policy.get("trigger_build").map(String::as_str), Some("confirm"));
        assert_eq!(jenkins.tool_policy.get("*").map(String::as_str), Some("auto"));
        // 没配 toolPolicy 的 server → 空 map（默认）。
        let json2 = r#"{ "servers": { "a": { "command": "x" } } }"#;
        let cfg2: McpServersConfig = serde_json::from_str(json2).unwrap();
        assert!(cfg2.servers.get("a").unwrap().tool_policy.is_empty());
    }

    // ---- keychain env 注入：resolve_env_value ----

    #[test]
    fn resolve_env_value_passes_through_literal() {
        // 无 keychain: 前缀的字面值原样返回（确定性，不碰密钥链）。
        assert_eq!(resolve_env_value("http://x.local").unwrap(), "http://x.local");
        assert_eq!(resolve_env_value("").unwrap(), "");
        // 仅 "keychain" 而非 "keychain:" 不触发前缀解析，按字面值处理。
        assert_eq!(resolve_env_value("keychain").unwrap(), "keychain");
    }

    #[test]
    fn resolve_env_value_missing_keychain_key_errs() {
        // keychain: 前缀但密钥不存在 → Err（绝不静默注入空值）。
        // 用极不可能存在的 account 名，确保走 NoEntry 分支、不依赖本机已存的密钥。
        let key = "jarvis-test-nonexistent-key-9f3a7b21";
        let err = resolve_env_value(&format!("keychain:{}", key)).unwrap_err();
        assert!(err.contains(key), "错误应点名缺失的密钥: {}", err);
        assert!(err.contains("不存在"), "实得: {}", err);
    }

    // 真·冒烟：spawn jenkins-mcp 并列出 8 个工具。
    //
    // 默认 #[ignore]，因为它依赖隔壁 jenkins-mcp 仓库已 `npm run build` 出
    // dist/index.js，且机器上有 node。本地核实跑：
    //   cd src-tauri
    //   cargo test --lib mcp_client::tests::smoke_jenkins_list_tools -- --ignored --nocapture
    #[tokio::test]
    #[ignore = "需要隔壁 jenkins-mcp 已 npm run build + 本机有 node；手动 --ignored 跑"]
    async fn smoke_jenkins_list_tools() {
        let index_js = r"D:\coding\my-mcp-servers\jenkins-mcp\dist\index.js";
        let mut env = HashMap::new();
        // dummy 值即可——list_tools 不会真打 Jenkins，但 server 没 env 会直接退出。
        env.insert(
            "JENKINS_ENV_TEST_URL".to_string(),
            "http://dummy.local".to_string(),
        );
        env.insert(
            "JENKINS_ENV_TEST_USERNAME".to_string(),
            "dummy".to_string(),
        );
        env.insert("JENKINS_ENV_TEST_TOKEN".to_string(), "dummy".to_string());

        let cfg = McpServerConfig {
            command: "node".to_string(),
            args: vec![index_js.to_string()],
            env,
            enabled: true,
            tool_policy: HashMap::new(),
        };

        let mgr = McpClientManager::new();
        mgr.spawn_server("jenkins", &cfg)
            .await
            .expect("spawn jenkins-mcp");

        let tools = mgr.list_tools("jenkins").await.expect("list tools");
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
        println!("[smoke] jenkins-mcp tools ({}): {:?}", names.len(), names);

        assert_eq!(names.len(), 8, "jenkins-mcp 应暴露 8 个工具，实得 {:?}", names);
        for expected in [
            "list_environments",
            "list_jobs",
            "get_job_info",
            "trigger_build",
            "get_build_status",
            "get_build_log",
            "cancel_build",
            "test_connection",
        ] {
            assert!(
                names.contains(&expected),
                "缺少工具 {}，实得 {:?}",
                expected,
                names
            );
        }

        mgr.shutdown_all().await;
    }
}
