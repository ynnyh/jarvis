// 对话式发版：prepare-deploy（生成建议，无副作用）+ confirm-deploy（真正触发）。
//
// 镜像 effort_logging 的 prepare → 用户确认 → 真写入 模式。发版是高危写操作，
// 红线：agent 只能调 prepare-deploy（提案）；confirm-deploy 只能经 tool_execute
// （前端确认卡片）或 channels 确认流触达，绝不在 agent loop 内被调。
//
// 环境（test/prod）必须显式匹配预设，绝不默认第一个——直接堵死 jenkins-mcp 的
// `envs[0]` 误发坑（environment 不传时它默认走第一个配置的环境）。

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::{json, Map, Value};

// ============================================================================
// 发版预设配置：~/.jarvis/deploy-presets.json
// ============================================================================
//
// 数据模型：项目（别名）→ 环境表（test/prod）→ { job, jenkinsEnvironment, params }。
// 账号(token)/baseUrl 不在这里——那些走 mcp-servers.json 的 env（keychain 注入），
// 本文件只管「发哪个项目的哪个环境、带什么构建参数」。

/// 单个环境的发版预设。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EnvironmentPreset {
    /// Jenkins job 名或 jenkins-mcp 别名（别名走 JENKINS_ALIAS_* 解析）。
    job: String,
    /// 传给 trigger_build 的 environment 名（对应 JENKINS_ENV_<NAME>_*）。
    /// 必须显式——jenkins-mcp 不传则默认 envs[0]，有误发风险。
    jenkins_environment: String,
    /// 构建参数预设（Jenkins build params 都是字符串）。用 BTreeMap 保证
    /// summary 里参数顺序稳定，便于人核对、便于测试。
    #[serde(default)]
    params: BTreeMap<String, String>,
}

/// 单个项目：环境名（test/prod）→ 环境预设。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectPreset {
    #[serde(default)]
    environments: BTreeMap<String, EnvironmentPreset>,
}

/// deploy-presets.json 根结构。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeployConfig {
    /// 调哪个 mcp-servers.json 里的 server id，缺省 "jenkins"。
    #[serde(default = "default_server")]
    server: String,
    #[serde(default)]
    projects: BTreeMap<String, ProjectPreset>,
}

fn default_server() -> String {
    "jenkins".to_string()
}

/// 配置文件路径：~/.jarvis/deploy-presets.json
fn deploy_presets_path() -> std::path::PathBuf {
    crate::settings::jarvis_dir().join("deploy-presets.json")
}

/// 读 deploy-presets.json。
///
/// 与 mcp-servers.json 宽容缺省语义**不同**：没配文件就发不了版（发版无预设
/// 毫无意义），故文件不存在直接报错，引导用户去配。坏 JSON 也显式报错。
fn load_deploy_config() -> Result<DeployConfig, String> {
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
// prepare-deploy：生成待确认建议（agent 可调，无副作用）
// ============================================================================

#[derive(Debug, Deserialize)]
struct PrepareDeployInput {
    #[serde(default)]
    project: String,
    #[serde(default)]
    environment: String,
    #[serde(default)]
    branch: Option<String>,
}

pub(crate) async fn prepare_deploy(input: Value) -> Result<Value, String> {
    let parsed: PrepareDeployInput =
        serde_json::from_value(input).map_err(|e| format!("prepare-deploy 入参错误: {}", e))?;

    // 唯一的副作用是读磁盘配置；核心逻辑全在纯函数 build_deploy_proposal 里，
    // 单测直接打它（不依赖真 ~/.jarvis/deploy-presets.json），测的就是上线代码本身。
    let config = load_deploy_config()?;
    build_deploy_proposal(
        &config,
        &parsed.project,
        &parsed.environment,
        parsed.branch.as_deref(),
    )
}

/// 纯函数：从已加载的配置 + 入参生成「待确认发版建议」。无任何 IO/副作用。
///
/// 拆出来既让 prepare_deploy 只剩「读配置 + 调本函数」，也让单测能不碰磁盘直接验证
/// 真实代码路径（环境校验、project/env 查表、payload 形态、summary 回显）。
fn build_deploy_proposal(
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
    // 环境绝不省略、绝不默认——这是堵死 envs[0] 误发的核心约束。
    if environment.is_empty() {
        return Err("必须显式指定环境（test/prod），不能省略".to_string());
    }

    let project_preset = config
        .projects
        .get(project)
        .ok_or_else(|| format!("未配置项目 {}", project))?;

    let env_preset = project_preset
        .environments
        .get(environment)
        .ok_or_else(|| format!("项目 {} 未配置 {} 环境", project, environment))?;

    // 构建 trigger_build 入参。jenkins-mcp 的 trigger_build inputSchema（已读其
    // src/index.ts 核实）：{ jobName, branch?, parameters?(嵌套对象), environment? }。
    // 注意参数 **嵌套在 parameters 子对象**，不是平铺；job 字段名是 jobName。
    // 关键：分支放进 parameters.branch（小写），绝不走顶层 branch ——jenkins-mcp 会把
    // 顶层 branch 映射成大写 BRANCH 构建参，这些 job 要的是小写 branch，传错就发错分支。
    let mut parameters = Map::new();
    for (k, v) in &env_preset.params {
        parameters.insert(k.clone(), Value::String(v.clone()));
    }
    // branch 覆盖：用户显式传 branch 时覆盖预设的 params.branch。
    let branch = branch.map(str::trim).filter(|s| !s.is_empty());
    if let Some(b) = branch {
        parameters.insert("branch".to_string(), Value::String(b.to_string()));
    }

    let mut args = Map::new();
    args.insert("jobName".to_string(), Value::String(env_preset.job.clone()));
    args.insert(
        "environment".to_string(),
        Value::String(env_preset.jenkins_environment.clone()),
    );
    args.insert("parameters".to_string(), Value::Object(parameters.clone()));

    // summary 给用户在确认卡片上核对：环境（test/prod）和分支必须醒目。
    let branch_display = parameters
        .get("branch")
        .and_then(|v| v.as_str())
        .unwrap_or("(预设默认)");
    let params_line = parameters
        .iter()
        .map(|(k, v)| format!("{}={}", k, v.as_str().unwrap_or_default()))
        .collect::<Vec<_>>()
        .join(", ");
    let summary = format!(
        "项目: {}\n环境: {}\n分支: {}\nJob: {}\n参数: {}",
        project, environment, branch_display, env_preset.job, params_line
    );

    Ok(json!({
        "pendingWrite": true,
        "kind": "mcp-deploy",
        "payload": {
            "server": config.server,
            "tool": "trigger_build",
            "args": Value::Object(args),
        },
        "summary": summary,
        "message": "已准备发版建议，请用户确认后再执行。",
    }))
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
    let arguments: Option<Map<String, Value>> = match &parsed.args {
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
        // 最小 projects/environments 形态；server 缺省为 "jenkins"。
        let cfg = config_from_json(
            r#"{
                "projects": {
                    "人资管理端": {
                        "environments": {
                            "test": {
                                "job": "example-access-web",
                                "jenkinsEnvironment": "test",
                                "params": { "branch": "dev" }
                            }
                        }
                    }
                }
            }"#,
        );
        assert_eq!(cfg.server, "jenkins");
        let proj = cfg.projects.get("人资管理端").expect("项目");
        let env = proj.environments.get("test").expect("test 环境");
        assert_eq!(env.job, "example-access-web");
        assert_eq!(env.jenkins_environment, "test");
        assert_eq!(env.params.get("branch").map(String::as_str), Some("dev"));
    }

    #[test]
    fn deploy_config_server_override() {
        let cfg = config_from_json(r#"{ "server": "my-jenkins", "projects": {} }"#);
        assert_eq!(cfg.server, "my-jenkins");
    }

    fn happy_config_json() -> &'static str {
        r#"{
            "projects": {
                "人资管理端": {
                    "environments": {
                        "test": {
                            "job": "example-access-web-test",
                            "jenkinsEnvironment": "test",
                            "params": {
                                "branch": "dev",
                                "node_version": "nodejs-18.14.2",
                                "server_ip": "192.0.2.23",
                                "CLEAN_DEPLOY": "false"
                            }
                        },
                        "prod": {
                            "job": "example-access-web-prod",
                            "jenkinsEnvironment": "prod",
                            "params": { "branch": "prod", "server_ip": "192.0.2.162" }
                        }
                    }
                }
            }
        }"#
    }

    #[test]
    fn prepare_deploy_requires_explicit_environment() {
        let cfg = config_from_json(happy_config_json());
        // 环境为空 → Err。
        let e = build_deploy_proposal(&cfg, "人资管理端", "", None).unwrap_err();
        assert!(e.contains("显式指定环境"), "实得: {}", e);
        // 未知项目 → Err。
        let e = build_deploy_proposal(&cfg, "不存在的项目", "test", None).unwrap_err();
        assert!(e.contains("未配置项目"), "实得: {}", e);
        // 项目存在但无该环境 → Err。
        let e = build_deploy_proposal(&cfg, "人资管理端", "staging", None).unwrap_err();
        assert!(e.contains("未配置 staging 环境"), "实得: {}", e);
    }

    #[test]
    fn prepare_deploy_happy_path() {
        let cfg = config_from_json(happy_config_json());
        let out = build_deploy_proposal(&cfg, "人资管理端", "test", None).expect("应成功");

        assert_eq!(out["kind"], "mcp-deploy");
        assert_eq!(out["pendingWrite"], true);
        let payload = &out["payload"];
        assert_eq!(payload["server"], "jenkins");
        assert_eq!(payload["tool"], "trigger_build");

        let args = &payload["args"];
        assert_eq!(args["jobName"], "example-access-web-test");
        // environment 必须 == jenkinsEnvironment（而非项目环境键），且显式。
        assert_eq!(args["environment"], "test");
        // 参数嵌套在 parameters 子对象。
        assert_eq!(args["parameters"]["branch"], "dev");
        assert_eq!(args["parameters"]["node_version"], "nodejs-18.14.2");
        assert_eq!(args["parameters"]["server_ip"], "192.0.2.23");
        assert_eq!(args["parameters"]["CLEAN_DEPLOY"], "false");
        // 红线：分支只能在 parameters.branch（小写），绝不能冒出顶层 branch
        // ——否则 jenkins-mcp 会把顶层 branch 映射成大写 BRANCH，发错分支。
        assert!(args.get("branch").is_none(), "args 不应有顶层 branch: {}", args);

        // summary 必须含环境与分支（人核对的安全点）。
        let summary = out["summary"].as_str().unwrap();
        assert!(summary.contains("环境: test"), "summary 缺环境: {}", summary);
        assert!(summary.contains("分支: dev"), "summary 缺分支: {}", summary);
    }

    #[test]
    fn prepare_deploy_branch_override() {
        let cfg = config_from_json(happy_config_json());
        // 显式传 branch 覆盖预设的 dev。
        let out =
            build_deploy_proposal(&cfg, "人资管理端", "test", Some("feature/x")).expect("应成功");
        assert_eq!(out["payload"]["args"]["parameters"]["branch"], "feature/x");
        let summary = out["summary"].as_str().unwrap();
        assert!(summary.contains("分支: feature/x"), "实得: {}", summary);

        // prod 环境回显 prod。
        let out = build_deploy_proposal(&cfg, "人资管理端", "prod", None).expect("应成功");
        assert_eq!(out["payload"]["args"]["environment"], "prod");
        let summary = out["summary"].as_str().unwrap();
        assert!(summary.contains("环境: prod"), "实得: {}", summary);
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
