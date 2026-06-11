// 对话式发版：prepare-deploy（查找项目匹配，返回中间结果）+ confirm-deploy（真正触发）。
//
// 镜像 effort_logging 的 prepare → 用户确认 → 真写入 模式。发版是高危写操作，
// 红线：agent 只能调 prepare-deploy（提案）；confirm-deploy 只能经 tool_execute
// （前端确认卡片）或 channels 确认流触达，绝不在 agent loop 内被调。
//
// 环境（test/prod）必须显式，绝不默认——直接堵死 jenkins-mcp 的
// `envs[0]` 误发坑（environment 不传时它默认走第一个配置的环境）。

use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::sync::OnceLock;
use tauri::Emitter;

// ============================================================================
// 发版预设配置：~/.jarvis/deploy-presets.json
// ============================================================================
//
// 数据模型：jenkinsUrl（全局）+ credentials 列表，每个 credential 包含
// name / token / projects（job→alias 映射）。
// 账号 token 走 keychain 占位（`keychain:jenkins-<name>-token`），
// 本文件只管「Jenkins 地址 + 凭据 + 项目映射」。

/// 单个项目：job 名 + 用户可见别名。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProjectEntry {
    pub job: String,
    pub alias: String,
}

/// 单个凭据条目：一个 Jenkins 账号 + 其下的项目列表。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CredentialEntry {
    pub name: String,
    /// keychain 占位或明文 token（读取时原样；保存时由 deploy_config.rs 处理）。
    #[serde(default)]
    pub token: String,
    #[serde(default)]
    pub projects: Vec<ProjectEntry>,
}

/// deploy-presets.json 根结构。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeployConfig {
    pub jenkins_url: String,
    #[serde(default)]
    pub credentials: Vec<CredentialEntry>,
}

/// 配置文件路径：~/.jarvis/deploy-presets.json
pub(crate) fn deploy_presets_path() -> std::path::PathBuf {
    crate::settings::jarvis_dir().join("deploy-presets.json")
}

// ============================================================================
// 全局 AppHandle：供轮询任务 emit 事件到前端
// ============================================================================

static APP_HANDLE: OnceLock<tauri::AppHandle> = OnceLock::new();

/// 在 app setup 阶段调用一次，存储 AppHandle 供后续轮询任务使用。
pub(crate) fn init_app_handle(app: tauri::AppHandle) {
    let _ = APP_HANDLE.set(app);
}

/// 读 deploy-presets.json。
///
/// 与 mcp-servers.json 宽容缺省语义**不同**：没配文件就发不了版（发版无预设
/// 毫无意义），故文件不存在直接报错，引导用户去配。坏 JSON 也显式报错。
pub(crate) fn load_deploy_config() -> Result<DeployConfig, String> {
    let path = deploy_presets_path();
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err("未配置发版项目，请先在 ~/.jarvis/deploy-presets.json 配置".to_string());
        }
        Err(e) => return Err(format!("读取 deploy-presets.json 失败: {}", e)),
    };
    serde_json::from_str(&content).map_err(|e| format!("解析 deploy-presets.json 失败: {}", e))
}

// ============================================================================
// prepare-deploy：查找项目匹配，返回需要参数选择的中间结果
// ============================================================================

#[derive(Debug, Deserialize)]
struct PrepareDeployInput {
    #[serde(default)]
    project: String,
    #[serde(default)]
    environment: String,
    #[serde(default)]
    branch: Option<String>,
    /// 构建参数（agent 从 get_job_info 拿到、用户确认后回传）。**带上它**就进入
    /// 「生成确认卡片」分支；不带则返回 needsParameters 引导 agent 先去拉参数。
    #[serde(default)]
    parameters: Option<Map<String, Value>>,
}

pub(crate) async fn prepare_deploy(input: Value) -> Result<Value, String> {
    let parsed: PrepareDeployInput =
        serde_json::from_value(input).map_err(|e| format!("prepare-deploy 入参错误: {}", e))?;

    // 唯一副作用是读磁盘配置；两个分支都是纯函数，单测直接打（不碰磁盘）。
    let config = load_deploy_config()?;
    match parsed.parameters {
        // 带了参数 → 生成发版确认卡片（前端据此渲染带确认按钮的卡片）。
        Some(params) => build_deploy_card(
            &config,
            &parsed.project,
            &parsed.environment,
            parsed.branch.as_deref(),
            params,
        ),
        // 没带参数 → 返回中间结果，引导 agent 先调 get_job_info 拉参数。
        None => build_deploy_lookup(
            &config,
            &parsed.project,
            &parsed.environment,
            parsed.branch.as_deref(),
        ),
    }
}

/// 在所有 credentials 的 projects 里按 alias 匹配，返回 (job 名, 凭据名)。
fn find_job_by_alias(config: &DeployConfig, alias: &str) -> Option<(String, String)> {
    for cred in &config.credentials {
        for proj in &cred.projects {
            if proj.alias == alias {
                return Some((proj.job.clone(), cred.name.clone()));
            }
        }
    }
    None
}

/// 纯函数：按 alias 匹配项目，返回「需要参数选择」的中间结果（引导 agent 去 get_job_info）。
///
/// 拆出来既让 prepare_deploy 只剩「读配置 + 分发」，也让单测能不碰磁盘直接验证真实代码路径。
fn build_deploy_lookup(
    config: &DeployConfig,
    project: &str,
    environment: &str,
    branch: Option<&str>,
) -> Result<Value, String> {
    let project = project.trim();
    let environment = environment.trim();
    if project.is_empty() {
        return Err("必须指定发版项目".to_string());
    }
    // 环境绝不省略、绝不默认——堵死 envs[0] 误发。
    if environment.is_empty() {
        return Err("必须显式指定环境（test/prod），不能省略".to_string());
    }

    let (job, credential_name) =
        find_job_by_alias(config, project).ok_or_else(|| format!("未配置项目 {}", project))?;
    let branch = branch.map(str::trim).filter(|s| !s.is_empty());

    Ok(json!({
        "needsParameters": true,
        "job": job,
        "credentialName": credential_name,
        "jenkinsUrl": config.jenkins_url,
        "project": project,
        "environment": environment,
        "branch": branch,
        "message": "已匹配到项目和凭据，请确认构建参数后再执行。"
    }))
}

/// 纯函数：参数齐全后生成「发版确认卡片」中间结果。
///
/// 前端 `pendingWriteFromToolMessage` 认 `prepare-deploy` 且 `kind=="mcp-deploy"` 才渲染
/// **带确认按钮的卡片**；`payload` 就是 confirm-deploy 需要的 `{server, tool, args}`。
/// 用户点确认 → confirm-deploy → trigger_build。这是「文字说确认不算数、必须点按钮」的来源。
fn build_deploy_card(
    config: &DeployConfig,
    project: &str,
    environment: &str,
    branch: Option<&str>,
    parameters: Map<String, Value>,
) -> Result<Value, String> {
    let project = project.trim();
    let environment = environment.trim();
    if project.is_empty() {
        return Err("必须指定发版项目".to_string());
    }
    if environment.is_empty() {
        return Err("必须显式指定环境（test/prod），不能省略".to_string());
    }

    let (job, _credential_name) =
        find_job_by_alias(config, project).ok_or_else(|| format!("未配置项目 {}", project))?;
    let branch = branch.map(str::trim).filter(|s| !s.is_empty());

    // 组 trigger_build 的 args（confirm-deploy 会原样转给 mcp trigger_build）。
    let mut args = Map::new();
    args.insert("jobName".to_string(), json!(job));
    if let Some(b) = branch {
        args.insert("branch".to_string(), json!(b));
    }
    if !parameters.is_empty() {
        args.insert("parameters".to_string(), Value::Object(parameters.clone()));
    }

    // 人读 summary：项目/环境/Job/分支 + 关键参数，供卡片展示给用户核对。
    let mut summary = format!("项目 {} ｜ 环境 {} ｜ Job {}", project, environment, job);
    if let Some(b) = branch {
        summary.push_str(&format!(" ｜ 分支 {}", b));
    }
    if !parameters.is_empty() {
        let kvs: Vec<String> = parameters
            .iter()
            .map(|(k, v)| format!("{}={}", k, value_to_plain(v)))
            .collect();
        summary.push_str(&format!(" ｜ 参数 {}", kvs.join(", ")));
    }

    Ok(json!({
        "pendingWrite": true,
        "kind": "mcp-deploy",
        "summary": summary,
        "payload": {
            "server": "jenkins",
            "tool": "trigger_build",
            "args": args,
        }
    }))
}

/// 把 JSON 值渲染成 summary 里的纯文本（字符串去引号，其余按 JSON 文本）。
fn value_to_plain(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

// ============================================================================
// confirm-deploy：真正触发构建（NOT agent 可调；经 tool_execute / 确认流触达）
// ============================================================================

#[derive(Debug, Deserialize)]
struct ConfirmDeployInput {
    #[serde(default)]
    server: String,
    #[serde(default)]
    tool: String,
    #[serde(default)]
    args: Value,
}

pub(crate) async fn confirm_deploy(input: Value) -> Result<Value, String> {
    let parsed: ConfirmDeployInput =
        serde_json::from_value(input).map_err(|e| format!("confirm-deploy 入参错误: {}", e))?;

    let server = if parsed.server.trim().is_empty() {
        "jenkins".to_string()
    } else {
        parsed.server.trim().to_string()
    };

    // 纵深防御：confirm-deploy 只允许 trigger_build，不能退化成「调任意 MCP 工具」后门。
    if parsed.tool != "trigger_build" {
        return Err("confirm-deploy 只允许 trigger_build".to_string());
    }

    // args 必须是 JSON 对象，转成 call_tool 期望的 Option<Map>。
    let arguments: Option<serde_json::Map<String, Value>> = match &parsed.args {
        Value::Object(map) => Some(map.clone()),
        Value::Null => None,
        _ => return Err("confirm-deploy 的 args 必须是 JSON 对象".to_string()),
    };

    let result = crate::mcp_client::manager()
        .call_tool(&server, &parsed.tool, arguments)
        .await;

    // 两层错误（见 mcp-client.md §3.3）：
    //   传输/协议层错 → call_tool 返回 Err（直接传播）。
    //   工具自身失败 → Ok(is_error==Some(true))，取 first_text 当错误回。
    match result {
        Err(e) => {
            append_deploy_audit(false, &server, &parsed.tool, &parsed.args, None, Some(&e));
            Err(e)
        }
        Ok(call_result) => {
            let text = crate::mcp_client::first_text(&call_result).unwrap_or_default();
            if call_result.is_error == Some(true) {
                let err = if text.is_empty() {
                    "trigger_build 执行失败".to_string()
                } else {
                    text
                };
                append_deploy_audit(false, &server, &parsed.tool, &parsed.args, None, Some(&err));
                return Err(err);
            }
            append_deploy_audit(true, &server, &parsed.tool, &parsed.args, Some(&text), None);
            // 尽力从结果文本里捞 queueId / buildNumber；捞不到也无妨，raw 一定带上。
            let (queue_id, build_number) = parse_build_identifiers(&text);

            // 启动后台轮询构建状态（PR4）。触发后按间隔查 get_build_status，
            // 通过 Tauri 事件推送到前端更新卡片。不阻塞 agent loop。
            if let Some(app_handle) = APP_HANDLE.get().cloned() {
                // jobName 从 args 里取；buildNumber 优先从触发结果取，没有就从 args 取。
                let job_name = parsed.args.get("jobName")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                // buildNumber 只认**真正的构建号**：trigger_build 通常只返回 queueId（≠ buildNumber），
                // 绝不能拿 queueId 当 buildNumber 去查（jenkins 会 404 → 前端「构建状态查询出错」）。
                // 没有真 buildNumber 时不传，让 get_build_status 取 lastBuild（触发后最新构建即本次）。
                let poll_build_number = build_number
                    .clone()
                    .or_else(|| parsed.args.get("buildNumber").cloned());
                if !job_name.is_empty() {
                    tokio::spawn(start_build_polling(
                        app_handle,
                        server.clone(),
                        job_name,
                        poll_build_number,
                        DEFAULT_POLL_INTERVAL_SECS,
                        DEFAULT_TIMEOUT_SECS,
                    ));
                }
            }

            Ok(json!({
                "ok": true,
                "queueId": queue_id,
                "buildNumber": build_number,
                "raw": text,
            }))
        }
    }
}

/// 尽力从 trigger_build 返回文本里解析 queueId / buildNumber。
///
/// jenkins-mcp 的 trigger_build 返回 `{ success, message, queueId, branch, parameters }`
/// 的 JSON 文本（queueId 一般有，buildNumber 触发阶段通常没有）。先按 JSON 解析；
/// 解析不出就返回 (None, None)，调用方靠 raw 兜底。
fn parse_build_identifiers(text: &str) -> (Option<Value>, Option<Value>) {
    let Ok(v) = serde_json::from_str::<Value>(text) else {
        return (None, None);
    };
    let queue_id = v.get("queueId").cloned().filter(|x| !x.is_null());
    let build_number = v
        .get("buildNumber")
        .or_else(|| v.get("number"))
        .cloned()
        .filter(|x| !x.is_null());
    (queue_id, build_number)
}

// ============================================================================
// 构建结果轮询（PR4）
// ============================================================================

/// 默认轮询间隔（秒）。
const DEFAULT_POLL_INTERVAL_SECS: u64 = 60;
/// 默认轮询超时（秒）。
const DEFAULT_TIMEOUT_SECS: u64 = 15 * 60;

/// 后台轮询构建状态，通过 Tauri 事件推送到前端。
///
/// 由 confirm_deploy 在触发成功后 tokio::spawn，不阻塞 agent loop。
/// 三种终态停止轮询：SUCCESS / FAILURE / ABORTED。超时 15 分钟后也停止。
async fn start_build_polling(
    app_handle: tauri::AppHandle,
    server: String,
    job_name: String,
    build_number: Option<Value>,
    poll_interval_secs: u64,
    timeout_secs: u64,
) {
    let timeout = std::time::Duration::from_secs(timeout_secs);
    let interval = std::time::Duration::from_secs(poll_interval_secs);
    let started = std::time::Instant::now();

    // 等几秒让 Jenkins 分配 buildNumber（触发后可能有延迟）。
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;

    // 首次尝试：用 args 里的 buildNumber（如果有的话）。
    // 没有 buildNumber 时 get_build_status 可能失败，循环会重试。
    let mut current_build_number = build_number;

    loop {
        if started.elapsed() > timeout {
            let _ = app_handle.emit(
                "build-status",
                json!({
                    "jobName": job_name,
                    "buildNumber": current_build_number,
                    "status": "timeout",
                    "result": null,
                    "log": null,
                    "url": null,
                }),
            );
            return;
        }

        // 构造 get_build_status 参数。
        let mut args_map = Map::new();
        args_map.insert("jobName".to_string(), json!(job_name));
        if let Some(ref bn) = current_build_number {
            args_map.insert("buildNumber".to_string(), bn.clone());
        }

        let call_result = match crate::mcp_client::manager()
            .call_tool(&server, "get_build_status", Some(args_map))
            .await
        {
            Ok(r) => r,
            Err(e) => {
                // 网络/协议错误：如果还没有 buildNumber，等一会重试；
                // 有 buildNumber 了还报错，emit error 后停止。
                eprintln!("[deploy] 轮询构建状态失败: {}", e);
                if current_build_number.is_some() {
                    let _ = app_handle.emit(
                        "build-status",
                        json!({
                            "jobName": job_name,
                            "buildNumber": current_build_number,
                            "status": "error",
                            "result": null,
                            "log": null,
                            "url": null,
                        }),
                    );
                    return;
                }
                // 没 buildNumber，等一下再试。
                tokio::time::sleep(interval).await;
                continue;
            }
        };

        let text = crate::mcp_client::first_text(&call_result).unwrap_or_default();

        // 工具自身报错（如 buildNumber 不存在）：同上，分情况处理。
        if call_result.is_error == Some(true) {
            eprintln!("[deploy] get_build_status 工具错误: {}", text);
            if current_build_number.is_some() {
                let _ = app_handle.emit(
                    "build-status",
                    json!({
                        "jobName": job_name,
                        "buildNumber": current_build_number,
                        "status": "error",
                        "result": null,
                        "log": null,
                        "url": null,
                    }),
                );
                return;
            }
            tokio::time::sleep(interval).await;
            continue;
        }

        // 解析 JSON 响应。
        let status_json: Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(_) => {
                eprintln!("[deploy] get_build_status 返回非 JSON: {}", text);
                tokio::time::sleep(interval).await;
                continue;
            }
        };

        // 如果之前没有 buildNumber，从返回结果里捞一个。
        if current_build_number.is_none() {
            let bn = status_json
                .get("buildNumber")
                .or_else(|| status_json.get("number"))
                .cloned()
                .filter(|x| !x.is_null());
            if bn.is_some() {
                current_build_number = bn;
            }
        }

        let building = status_json
            .get("building")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let result = status_json
            .get("result")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let url = status_json
            .get("url")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        if building {
            // 还在构建中，emit building 状态，继续等。
            let _ = app_handle.emit(
                "build-status",
                json!({
                    "jobName": job_name,
                    "buildNumber": current_build_number,
                    "status": "building",
                    "result": null,
                    "log": null,
                    "url": url,
                }),
            );
            tokio::time::sleep(interval).await;
            continue;
        }

        // building == false：终态。
        match result {
            "SUCCESS" => {
                let _ = app_handle.emit(
                    "build-status",
                    json!({
                        "jobName": job_name,
                        "buildNumber": current_build_number,
                        "status": "success",
                        "result": "SUCCESS",
                        "log": null,
                        "url": url,
                    }),
                );
                return;
            }
            "FAILURE" => {
                // 失败：拉日志尾巴。
                let log_tail = fetch_build_log_tail(&server, &job_name, current_build_number.as_ref(), 30).await;
                let _ = app_handle.emit(
                    "build-status",
                    json!({
                        "jobName": job_name,
                        "buildNumber": current_build_number,
                        "status": "failure",
                        "result": "FAILURE",
                        "log": log_tail,
                        "url": url,
                    }),
                );
                return;
            }
            "ABORTED" => {
                let _ = app_handle.emit(
                    "build-status",
                    json!({
                        "jobName": job_name,
                        "buildNumber": current_build_number,
                        "status": "aborted",
                        "result": "ABORTED",
                        "log": null,
                        "url": url,
                    }),
                );
                return;
            }
            _ => {
                // 其它终态（如 NOT_BUILT 等），当 aborted 处理。
                let _ = app_handle.emit(
                    "build-status",
                    json!({
                        "jobName": job_name,
                        "buildNumber": current_build_number,
                        "status": "aborted",
                        "result": result,
                        "log": null,
                        "url": url,
                    }),
                );
                return;
            }
        }
    }
}

/// 拉构建日志尾巴。失败不致命，返回 None。
async fn fetch_build_log_tail(
    server: &str,
    job_name: &str,
    build_number: Option<&Value>,
    tail: u32,
) -> Option<String> {
    let mut args = Map::new();
    args.insert("jobName".to_string(), json!(job_name));
    if let Some(bn) = build_number {
        args.insert("buildNumber".to_string(), bn.clone());
    }
    args.insert("tail".to_string(), json!(tail));

    let result = crate::mcp_client::manager()
        .call_tool(server, "get_build_log", Some(args))
        .await
        .ok()?;
    crate::mcp_client::first_text(&result)
}

/// 发版审计日志：追加一行 JSONL 到 ~/.jarvis/write-back.log。
///
/// 发版是高危写操作，成功失败都必须可追溯（产品铁律）。复用 effort_logging 的
/// write-back.log 同一文件、同一 ts 前缀风格（这里保留本地小副本，避免把
/// effort_logging 的私有 append_audit_log 提成 pub 而扩大其暴露面）。
fn append_deploy_audit(
    ok: bool,
    server: &str,
    tool: &str,
    args: &Value,
    result: Option<&str>,
    error: Option<&str>,
) {
    let entry = json!({
        "ts": chrono::Utc::now().to_rfc3339(),
        "action": "mcp-deploy",
        "ok": ok,
        "server": server,
        "tool": tool,
        "args": args,
        "result": result,
        "error": error,
    });
    let path = crate::settings::jarvis_dir().join("write-back.log");
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let line = format!("{}\n", entry);
    if let Err(e) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut f| {
            use std::io::Write;
            f.write_all(line.as_bytes())
        })
    {
        // 审计写失败不致命（发版本身已完成/失败），但必须可见，绝不静默吞。
        eprintln!("[deploy] 写发版审计日志失败: {}", e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_from_json(s: &str) -> DeployConfig {
        serde_json::from_str(s).expect("解析测试配置")
    }

    #[test]
    fn deploy_config_parses_minimal() {
        // 最小形态：jenkinsUrl + 一个 credential 含一个 project。
        let cfg = config_from_json(
            r#"{
                "jenkinsUrl": "http://jenkins.example.internal:8080/",
                "credentials": [
                    {
                        "name": "主账号",
                        "token": "keychain:jenkins-main-token",
                        "projects": [
                            { "job": "example-access-web", "alias": "人资管理端" }
                        ]
                    }
                ]
            }"#,
        );
        assert_eq!(cfg.jenkins_url, "http://jenkins.example.internal:8080/");
        assert_eq!(cfg.credentials.len(), 1);
        let cred = &cfg.credentials[0];
        assert_eq!(cred.name, "主账号");
        assert_eq!(cred.token, "keychain:jenkins-main-token");
        assert_eq!(cred.projects.len(), 1);
        assert_eq!(cred.projects[0].job, "example-access-web");
        assert_eq!(cred.projects[0].alias, "人资管理端");
    }

    #[test]
    fn deploy_config_empty_credentials() {
        let cfg = config_from_json(r#"{ "jenkinsUrl": "http://x" }"#);
        assert_eq!(cfg.jenkins_url, "http://x");
        assert!(cfg.credentials.is_empty());
    }

    fn happy_config_json() -> &'static str {
        r#"{
            "jenkinsUrl": "http://jenkins.example.internal:8080/",
            "credentials": [
                {
                    "name": "主账号",
                    "token": "keychain:jenkins-main-token",
                    "projects": [
                        { "job": "example-access-web-test", "alias": "人资管理端" },
                        { "job": "example-quality-web", "alias": "质量系统" }
                    ]
                },
                {
                    "name": "prod账号",
                    "token": "keychain:jenkins-prod-token",
                    "projects": [
                        { "job": "example-access-web-prod", "alias": "人资管理端-prod" }
                    ]
                }
            ]
        }"#
    }

    #[test]
    fn prepare_deploy_requires_explicit_environment() {
        let cfg = config_from_json(happy_config_json());
        // 环境为空 → Err。
        let e = build_deploy_lookup(&cfg, "人资管理端", "", None).unwrap_err();
        assert!(e.contains("显式指定环境"), "实得: {}", e);
        // 未知项目 → Err。
        let e = build_deploy_lookup(&cfg, "不存在的项目", "test", None).unwrap_err();
        assert!(e.contains("未配置项目"), "实得: {}", e);
    }

    #[test]
    fn prepare_deploy_happy_path() {
        let cfg = config_from_json(happy_config_json());
        let out = build_deploy_lookup(&cfg, "人资管理端", "test", None).expect("应成功");

        assert_eq!(out["needsParameters"], true);
        assert_eq!(out["job"], "example-access-web-test");
        assert_eq!(out["credentialName"], "主账号");
        assert_eq!(out["jenkinsUrl"], "http://jenkins.example.internal:8080/");
        assert_eq!(out["project"], "人资管理端");
        assert_eq!(out["environment"], "test");
        assert!(out["branch"].is_null());
        assert!(out["message"].as_str().unwrap().contains("匹配"));
    }

    #[test]
    fn prepare_deploy_branch_provided() {
        let cfg = config_from_json(happy_config_json());
        let out = build_deploy_lookup(&cfg, "质量系统", "test", Some("feature/x")).expect("应成功");
        assert_eq!(out["job"], "example-quality-web");
        assert_eq!(out["branch"], "feature/x");
    }

    #[test]
    fn prepare_deploy_branch_whitespace_filtered() {
        let cfg = config_from_json(happy_config_json());
        let out = build_deploy_lookup(&cfg, "质量系统", "test", Some("  ")).expect("应成功");
        assert!(out["branch"].is_null(), "空白 branch 应被过滤: {}", out["branch"]);
    }

    #[test]
    fn prepare_deploy_cross_credential_lookup() {
        // "人资管理端-prod" 在第二个 credential 里。
        let cfg = config_from_json(happy_config_json());
        let out = build_deploy_lookup(&cfg, "人资管理端-prod", "prod", None).expect("应成功");
        assert_eq!(out["job"], "example-access-web-prod");
        assert_eq!(out["credentialName"], "prod账号");
    }

    #[test]
    fn prepare_deploy_card_with_params() {
        // 带 parameters → 生成发版确认卡片（kind=mcp-deploy + payload.trigger_build）。
        let cfg = config_from_json(happy_config_json());
        let mut params = Map::new();
        params.insert("node_version".to_string(), json!("nodejs-16.20.0"));
        params.insert("server_ip".to_string(), json!("192.0.2.21"));
        let out = build_deploy_card(&cfg, "质量系统", "test", Some("develop"), params).expect("应成功");

        assert_eq!(out["pendingWrite"], true);
        assert_eq!(out["kind"], "mcp-deploy");
        assert_eq!(out["payload"]["server"], "jenkins");
        assert_eq!(out["payload"]["tool"], "trigger_build");
        assert_eq!(out["payload"]["args"]["jobName"], "example-quality-web");
        assert_eq!(out["payload"]["args"]["branch"], "develop");
        assert_eq!(
            out["payload"]["args"]["parameters"]["server_ip"],
            "192.0.2.21"
        );
        let summary = out["summary"].as_str().unwrap();
        assert!(summary.contains("质量系统") && summary.contains("server_ip="), "summary: {}", summary);
    }

    #[test]
    fn prepare_deploy_card_unknown_project_errs() {
        let cfg = config_from_json(happy_config_json());
        let e = build_deploy_card(&cfg, "不存在", "test", None, Map::new()).unwrap_err();
        assert!(e.contains("未配置项目"), "实得: {}", e);
    }

    #[tokio::test]
    async fn confirm_deploy_rejects_non_trigger_tool() {
        // tool != trigger_build → 立即 Err，不发起任何 live MCP 调用。
        let payload = json!({
            "server": "jenkins",
            "tool": "cancel_build",
            "args": { "jobName": "x" }
        });
        let e = confirm_deploy(payload).await.unwrap_err();
        assert!(e.contains("只允许 trigger_build"), "实得: {}", e);
    }

    #[test]
    fn parse_build_identifiers_best_effort() {
        // jenkins-mcp 触发返回带 queueId。
        let (q, b) = parse_build_identifiers(r#"{"success":true,"queueId":42,"branch":"dev"}"#);
        assert_eq!(q, Some(json!(42)));
        assert_eq!(b, None);
        // 非 JSON 文本 → 都为 None（靠 raw 兜底）。
        let (q, b) = parse_build_identifiers("not json");
        assert!(q.is_none() && b.is_none());
    }
}
