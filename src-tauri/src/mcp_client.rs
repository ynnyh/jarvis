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

use rmcp::model::{CallToolRequestParams, CallToolResult, Tool};
use rmcp::service::{RoleClient, RunningService};
use rmcp::transport::TokioChildProcess;
use rmcp::ServiceExt;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tokio::process::Command;
use tokio::sync::Mutex;

use crate::settings::jarvis_dir;

/// 已连接的 MCP server 句柄类型别名。
///
/// 持有 `RunningService` 本体（而非只留 `Peer`）—— 一旦 drop，rmcp 会关闭
/// transport 并杀掉子进程。所以管理器必须把它存活在 map 里。
type Running = RunningService<RoleClient, ()>;

// ============================================================================
// 配置模型：~/.jarvis/mcp-servers.json
// ============================================================================
//
// PR1 先用最小形态，够 spawn 即可。`toolPolicy`（动态安全分类）、账号/项目
// 参数预设等留给 PR2/PR3，故意不在这里建模，避免过度设计。

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
        let map = self.servers.lock().await;
        let running = map
            .get(id)
            .ok_or_else(|| format!("MCP server '{}' 未连接", id))?;
        // RunningService Deref 到 Peer<RoleClient>，可直接调。
        running
            .list_all_tools()
            .await
            .map_err(|e| format!("list_tools('{}') 失败: {}", id, e))
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
        let map = self.servers.lock().await;
        let running = map
            .get(id)
            .ok_or_else(|| format!("MCP server '{}' 未连接", id))?;
        // CallToolRequestParams 是 #[non_exhaustive]，跨 crate 不能用结构体字面量，
        // 走 new()/with_arguments() builder。
        let mut params = CallToolRequestParams::new(tool_name.to_string());
        if let Some(args) = arguments {
            params = params.with_arguments(args);
        }
        running
            .call_tool(params)
            .await
            .map_err(|e| format!("call_tool('{}/{}') 失败: {}", id, tool_name, e))
    }

    /// 当前已连接的 server id 列表。
    pub async fn connected_ids(&self) -> Vec<String> {
        let map = self.servers.lock().await;
        map.keys().cloned().collect()
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
        cmd.env(k, v);
    }

    let transport =
        TokioChildProcess::new(cmd).map_err(|e| format!("spawn 子进程失败: {}", e))?;

    // `().serve(transport)` 会自动跑 initialize 握手；返回 Ok 即握手成功、可用。
    ()
        .serve(transport)
        .await
        .map_err(|e| format!("MCP initialize 握手失败: {}", e))
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
