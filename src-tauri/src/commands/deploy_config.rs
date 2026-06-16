// 发版配置设置页后端：读写 deploy-presets.json + mcp-servers.json + token 落密钥链
// + 保存后重启 jenkins-mcp + 测试连接。
//
// 这是 jenkins-deploy 任务「设置页后端」PR：让前端能在 UI 里读写发版配置，
// 不必再手改两个 JSON、token 也有了安全入口。前端 UI 是下一个 PR，本模块只做后端。
//
// ---- 两个配置文件、两个消费者（直接复用，勿另起炉灶）----
//   - ~/.jarvis/deploy-presets.json：{jenkinsUrl, credentials:[{name,username,token,projects}]}。
//     由 tools/deploy.rs 的 load_deploy_config() 消费（对话式发版的预设来源）。
//   - ~/.jarvis/mcp-servers.json：{servers:{jenkins:{command,args,env,enabled}}}。
//     由 mcp_client.rs 消费（spawn MCP 子进程；env 里 `keychain:` 前缀的值在 spawn
//     时经 resolve_env_value 从密钥链解出）。路径函数 mcp_servers_config_path() 已存在。
//
// ---- 桥接策略 ----
//   deploy-presets.json 是发版配置的唯一真相来源。保存时，本模块同时更新
//   mcp-servers.json 的 Jenkins env（从 credentials 派生 JENKINS_ENV_* 三件套），
//   让 mcp_client.rs 能正常 spawn jenkins-mcp。mcp_client.rs 本身不需要改动。
//
// ---- jenkins-mcp env 模型（已读其 src/index.ts 核实）----
//   每个环境要三件套 JENKINS_ENV_<NAME>_URL / _USERNAME / _TOKEN（三者齐全才生效），
//   生成的环境名 = <NAME>.toLowerCase()。这个小写名必须等于 trigger_build 的
//   environment 参数。故约定：凭据名一律小写（hasToken/test_connection 用它），
//   写进 env key 时大写。
//
// ---- token 安全 ----
//   token 绝不返回明文、绝不落明文配置。deploy-presets.json 里只写占位
//   `keychain:jenkins-<name>-token`，真值经 secret_set 存进 OS 密钥链
//   （与 LLM apiKey 同一套 settings::secret_* 机制）。
//   spawn 时 mcp_client::resolve_env_value 才把占位解成真值注入子进程。

use serde::Deserialize;
use serde_json::{json, Value};

use crate::mcp_client::{load_mcp_servers_config, mcp_servers_config_path, McpServerConfig};

/// jenkins-mcp 在用户本机的默认入口（机器相关，故可配；仅当配置里没有时兜底）。
const DEFAULT_JENKINS_MCP_PATH: &str = r"D:\coding\my-mcp-servers\jenkins-mcp\dist\index.js";

/// mcp-servers.json 里 Jenkins server 的固定 id。
const JENKINS_SERVER_ID: &str = "jenkins";

// ============================================================================
// deploy-presets.json 路径（与 tools/deploy.rs 的 deploy_presets_path 同址）
// ============================================================================
//
// tools/deploy.rs 把 deploy_presets_path() 设为 pub(crate)，这里复用同址实现，
// 两处都从 settings::jarvis_dir() 拼，路径一致，是同一个文件。

fn deploy_presets_path() -> std::path::PathBuf {
    crate::settings::jarvis_dir().join("deploy-presets.json")
}

// ============================================================================
// 输入模型（前端 PR2 依赖的 JSON 契约）
// ============================================================================

/// 单个项目入参。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectInput {
    job: String,
    alias: String,
}

/// 单个凭据入参。
///
/// `username` 是 Jenkins 账号名——API 鉴权是 `username:token` 基本认证，必需；
/// jenkins-mcp 的 parseEnvironments 要求 url/username/token 三者非空才注册环境。
/// `token` 仅在用户新填/改了密码时才带（缺省/空 = 不动已有密钥，绝不覆盖成空）。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CredentialInput {
    name: String,
    #[serde(default)]
    username: String,
    #[serde(default)]
    token: Option<String>,
    #[serde(default)]
    projects: Vec<ProjectInput>,
}

/// deploy_config_save 的入参。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SaveInput {
    /// jenkins-mcp 入口路径；缺省沿用现有 args[0]，再缺省用默认路径。
    #[serde(default)]
    jenkins_mcp_path: Option<String>,
    /// Jenkins 服务器地址（全局）。
    jenkins_url: String,
    /// 凭据列表（每个凭据 = 一个 Jenkins 账号 + 其下的项目列表）。
    #[serde(default)]
    credentials: Vec<CredentialInput>,
}

// ============================================================================
// 命令 1：deploy_config_get —— 合成只读视图（token 绝不返回明文）
// ============================================================================

/// 读两文件，合成发版配置的只读视图返回给前端。
///
/// 两文件缺失都按空处理（不报错——「还没配」是正常态）。token 只回 hasToken 布尔，
/// 绝不回明文。
#[tauri::command]
pub fn deploy_config_get() -> Result<Value, String> {
    // mcp-servers.json：宽容缺省（load_mcp_servers_config 文件不存在即 Ok(空)）。
    let mcp_cfg = load_mcp_servers_config()?;
    let jenkins = mcp_cfg.servers.get(JENKINS_SERVER_ID);

    let jenkins_mcp_path = jenkins
        .and_then(|c| c.args.first())
        .map(|s| s.to_string())
        .unwrap_or_else(|| DEFAULT_JENKINS_MCP_PATH.to_string());

    // deploy-presets.json：读新结构（文件缺失/坏 JSON 都按空）。
    let (jenkins_url, credentials_view) = load_deploy_presets_view();

    Ok(json!({
        "jenkinsMcpPath": jenkins_mcp_path,
        "jenkinsUrl": jenkins_url,
        "credentials": credentials_view,
    }))
}

/// 读 deploy-presets.json 并返回 (jenkinsUrl, credentials 只读视图)。
///
/// 文件不存在/坏 JSON → 空值（设置页打不开不该因为坏配置，用户正要来修）。
fn load_deploy_presets_view() -> (String, Vec<Value>) {
    let content = match std::fs::read_to_string(deploy_presets_path()) {
        Ok(c) => c,
        Err(_) => return (String::new(), Vec::new()),
    };
    let value: Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return (String::new(), Vec::new()),
    };

    let jenkins_url = value
        .get("jenkinsUrl")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let credentials = value
        .get("credentials")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let credentials_view: Vec<Value> = credentials
        .iter()
        .map(|cred| {
            let name = cred
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let username = cred
                .get("username")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let account = token_keychain_account(&name);
            let has_token = crate::settings::secret_get(&account).is_some();
            let projects = cred
                .get("projects")
                .cloned()
                .unwrap_or_else(|| json!([]));
            json!({
                "name": name,
                "username": username,
                "hasToken": has_token,
                "projects": projects,
            })
        })
        .collect();

    (jenkins_url, credentials_view)
}

// ============================================================================
// 命令 2：deploy_config_save —— 写两文件 + token 落密钥链 + 重启 jenkins-mcp
// ============================================================================

/// 保存发版配置：写 deploy-presets.json + mcp-servers.json，token 落密钥链，
/// 重启 jenkins-mcp，并把重启结果回报前端。
///
/// 顺序：先校验 → 落密钥链 → 写两文件 → 重启。配置写盘后即便重启失败也属正常
/// （坏 token/路径下次修对再存即可），故重启错误返回给前端但配置已持久化。
#[tauri::command]
pub async fn deploy_config_save(input: Value) -> Result<(), String> {
    // Tauri v2 invoke 可能把整个 { input: {...} } 传进来，也可能是直接 {...}。
    // 如果顶层只有 "input" 一个 key 且其值是对象，就解包一层。
    let actual = match input.as_object() {
        Some(obj) if obj.len() == 1 => {
            if let Some(inner) = obj.get("input") {
                if inner.is_object() { inner.clone() } else { input }
            } else {
                input
            }
        }
        _ => input,
    };
    let parsed: SaveInput =
        serde_json::from_value(actual).map_err(|e| format!("发版配置入参错误: {}", e))?;

    // 1. 校验每个凭据名（字母数字/-、非空）+ 项目别名非空。
    for c in &parsed.credentials {
        validate_credential_name(&c.name)?;
    }
    validate_project_aliases(&parsed.credentials)?;

    // 2. 读现有 mcp-servers.json（保留 jenkins 以外的其它 server 不动）。
    let mut mcp_cfg = load_mcp_servers_config()?;
    let old_jenkins = mcp_cfg.servers.get(JENKINS_SERVER_ID).cloned();

    // 3. 解析 jenkinsMcpPath：入参优先 → 现有 args[0] → 默认路径。
    let jenkins_mcp_path = resolve_jenkins_mcp_path(
        parsed.jenkins_mcp_path.as_deref(),
        old_jenkins.as_ref(),
    );

    // 4. token 落密钥链：仅对带非空 token 的凭据 secret_set；token 缺省/空 → 不动该 key
    //    的已有密钥（保留），绝不覆盖成空。这一步在写盘前做，密钥链是 token 的唯一真值来源。
    for c in &parsed.credentials {
        if let Some(token) = c.token.as_deref() {
            let token = token.trim();
            if !token.is_empty() {
                let account = token_keychain_account(&c.name);
                crate::settings::secret_set(&account, token)?;
            }
        }
    }

    // 5. 被删掉的凭据：从密钥链清掉其 secret（用 secret_clear），避免遗留无主密钥。
    //    对比来源：旧 deploy-presets.json 的 credentials。
    if let Ok(old_content) = std::fs::read_to_string(deploy_presets_path()) {
        if let Ok(old_value) = serde_json::from_str::<Value>(&old_content) {
            let old_names = credential_names_from_json(&old_value);
            let new_names: std::collections::HashSet<String> = parsed
                .credentials
                .iter()
                .map(|c| c.name.trim().to_lowercase())
                .collect();
            for old_name in old_names {
                if !new_names.contains(&old_name) {
                    let account = token_keychain_account(&old_name);
                    if let Err(e) = crate::settings::secret_clear(&account) {
                        tracing::error!(target: "deploy_config", "清理已删除凭据的密钥 '{}' 失败: {}", account, e);
                    }
                }
            }
        }
    }

    // 6. 重建 jenkins server：保留 enabled（沿用旧值，缺省 true）与旧 toolPolicy
    //    （设置页不管 toolPolicy，但绝不能在保存时把用户已有的策略悄悄抹掉），
    //    其余按新凭据重写。
    let jenkins_cfg = build_jenkins_server_config(
        &jenkins_mcp_path,
        &parsed.jenkins_url,
        &parsed.credentials,
        old_jenkins.as_ref().map(|c| c.enabled).unwrap_or(true),
        old_jenkins
            .as_ref()
            .map(|c| c.tool_policy.clone())
            .unwrap_or_default(),
    );
    mcp_cfg
        .servers
        .insert(JENKINS_SERVER_ID.to_string(), jenkins_cfg.clone());

    // 7. 写 deploy-presets.json：新结构（jenkinsUrl + credentials 含 keychain: 占位），原子写。
    let presets = build_deploy_presets_json(&parsed.jenkins_url, &parsed.credentials);
    write_json_atomic(&deploy_presets_path(), &presets)?;

    // 8. 写 mcp-servers.json，原子写。
    let mcp_value = serde_json::to_value(&mcp_cfg)
        .map_err(|e| format!("序列化 mcp-servers.json 失败: {}", e))?;
    write_json_atomic(&mcp_servers_config_path(), &mcp_value)?;

    // 9. 重启 jenkins-mcp：先关（没在跑就忽略），再起。spawn 失败（坏 token/路径/握手失败）
    //    把错误返回给前端，让用户立刻知道——此时配置已写盘，属正常。
    let mgr = crate::mcp_client::manager();
    if let Err(e) = mgr.shutdown_server(JENKINS_SERVER_ID).await {
        tracing::error!(target: "deploy_config", "重启前关停 jenkins 失败（可能未在运行）: {}", e);
    }
    mgr.spawn_server(JENKINS_SERVER_ID, &jenkins_cfg).await?;

    Ok(())
}

// ============================================================================
// 命令 3：deploy_test_connection —— 直接打 Jenkins /api/json 验证当前填写的凭据
// ============================================================================

/// 测试连接：用**表单当前填写**的 url + username + token 直接 GET Jenkins `/api/json`
/// 做 Basic 认证验证凭据，**不需先保存、不依赖 spawn jenkins-mcp**——契合「填好 → 测通 → 再保存」
/// 的用户习惯（旧实现要先保存再测、且测的是已保存的旧值，导致改了 token 不保存就测不通）。
///
/// token 留空时回退取 keychain 里该账号（`name`）已存的 token（用于复测已保存的账号）。
#[tauri::command]
pub async fn deploy_test_connection(
    name: String,
    url: String,
    username: String,
    token: Option<String>,
) -> Result<Value, String> {
    let url = url.trim().trim_end_matches('/');
    if url.is_empty() {
        return Err("请先填写 Jenkins 地址".to_string());
    }
    let username = username.trim();
    if username.is_empty() {
        return Err("请先填写用户名".to_string());
    }

    // token：表单填了就用表单的；没填则回退取 keychain 已存的（测已保存账号）。
    let token = match token.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(t) => t.to_string(),
        None => {
            let account = token_keychain_account(&name);
            crate::settings::secret_get(&account)
                .ok_or_else(|| "请填写 token（该账号还没保存过 token）".to_string())?
        }
    };

    // 直接 GET {url}/api/json 做 Basic 认证（与 jenkins-mcp 的 testConnection 同款），
    // 不经 MCP spawn——测的就是用户此刻填的凭据，所见即所测。
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {}", e))?;
    let resp = client
        .get(format!("{}/api/json", url))
        .query(&[("tree", "nodeName")])
        .basic_auth(username, Some(&token))
        .send()
        .await
        .map_err(|e| format!("无法连接 {}：{}", url, e))?;

    let status = resp.status();
    if status.is_success() {
        let version = resp
            .headers()
            .get("x-jenkins")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let detail = if version.is_empty() {
            "连接成功 ✓".to_string()
        } else {
            format!("连接成功 ✓（Jenkins {}）", version)
        };
        Ok(json!({ "ok": true, "detail": detail, "jenkinsUrl": url }))
    } else if status.as_u16() == 401 {
        Err("认证失败：用户名或 token 不对".to_string())
    } else if status.as_u16() == 403 {
        Err("无权限（403）：账号权限不足".to_string())
    } else {
        Err(format!("连接失败：HTTP {}", status.as_u16()))
    }
}

// ============================================================================
// 纯函数（不碰磁盘/密钥链，可直接单测真实代码路径）
// ============================================================================

/// 凭据逻辑名 → 密钥链 account：`jenkins-<name小写>-token`。
///
/// 必须与 deploy-presets.json 里写的占位 `keychain:jenkins-<name小写>-token` 对应，
/// 这样 spawn 时 resolve_env_value 才能从同一 account 取到真值。
fn token_keychain_account(name: &str) -> String {
    format!("jenkins-{}-token", name.trim().to_lowercase())
}

/// 凭据逻辑名 → env key 前缀里的大写段：`JENKINS_ENV_<NAME大写>_`。
fn env_key_prefix(name: &str) -> String {
    format!("JENKINS_ENV_{}_", name.trim().to_uppercase())
}

/// 校验凭据名：非空，且仅含字母/数字/`-`。
fn validate_credential_name(name: &str) -> Result<(), String> {
    let n = name.trim();
    if n.is_empty() {
        return Err("凭据名不能为空".to_string());
    }
    if !n.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err(format!("凭据名 '{}' 只能包含字母、数字和连字符(-)", n));
    }
    Ok(())
}

/// 校验所有项目别名非空。每个项目都必须有别名（产品要求：对话发版按别名指项目）。
/// 前端已把 job 为空的空行过滤掉，故这里只校验 alias；为空即报错并指明 job。
fn validate_project_aliases(credentials: &[CredentialInput]) -> Result<(), String> {
    for c in credentials {
        for p in &c.projects {
            if p.alias.trim().is_empty() {
                return Err(format!("项目「{}」的别名不能为空", p.job.trim()));
            }
        }
    }
    Ok(())
}

/// 入参 path 优先 → 现有 jenkins server 的 args[0] → 默认路径。
fn resolve_jenkins_mcp_path(input: Option<&str>, old: Option<&McpServerConfig>) -> String {
    if let Some(p) = input.map(str::trim).filter(|s| !s.is_empty()) {
        return p.to_string();
    }
    if let Some(existing) = old.and_then(|c| c.args.first()).map(|s| s.trim()) {
        if !existing.is_empty() {
            return existing.to_string();
        }
    }
    DEFAULT_JENKINS_MCP_PATH.to_string()
}

/// 用凭据列表重建 jenkins server 的 spawn 配置。
///
/// env 对每个凭据 c 写三件套（token 只写 `keychain:` 占位，真值在密钥链）：
///   JENKINS_ENV_<大写>_URL / _USERNAME / _TOKEN
/// 三者必须齐全（jenkins-mcp 的 parseEnvironments 要求 url/username/token 都非空，
/// 否则跳过该环境；全跳过则它启动即抛「未配置 Jenkins 连接」并退出）。
fn build_jenkins_server_config(
    jenkins_mcp_path: &str,
    jenkins_url: &str,
    credentials: &[CredentialInput],
    enabled: bool,
    tool_policy: std::collections::HashMap<String, String>,
) -> McpServerConfig {
    let mut env = std::collections::HashMap::new();
    for c in credentials {
        let prefix = env_key_prefix(&c.name);
        // URL 使用全局 jenkinsUrl（新结构不再 per-credential 配 URL）。
        env.insert(format!("{}URL", prefix), jenkins_url.trim().to_string());
        // username 是 Jenkins 账号名，必需——空了 jenkins-mcp 会跳过这个环境。
        env.insert(format!("{}USERNAME", prefix), c.username.trim().to_string());
        // token 永不落明文：env 里只放 keychain 占位，真值在密钥链。
        env.insert(
            format!("{}TOKEN", prefix),
            format!("keychain:{}", token_keychain_account(&c.name)),
        );
    }

    // toolPolicy：用户没显式配（空）→ 给安全默认。只读工具 auto（agent 可直接调，如
    // get_job_info / list_jobs / get_build_status——发版流程靠它读构建参数），只有写操作
    // trigger_build / cancel_build 需确认。否则空策略会被分类器「默认 confirm」兜底，把
    // 只读工具也拦死，agent 连读参数都不行。用户已显式配则尊重，不覆盖。
    let tool_policy = if tool_policy.is_empty() {
        default_jenkins_tool_policy()
    } else {
        tool_policy
    };

    McpServerConfig {
        command: "node".to_string(),
        args: vec![jenkins_mcp_path.to_string()],
        env,
        enabled,
        tool_policy,
    }
}

/// jenkins 的安全默认 toolPolicy：写操作需确认，其余（只读）auto。
///
/// 对应动态工具分类的「显式策略」来源（最高优先，见 mcp-agent-integration.md）。
/// 杜绝「无策略无注解 → 全 confirm」把 get_job_info 等只读工具也拦死、发版流程读不到参数。
fn default_jenkins_tool_policy() -> std::collections::HashMap<String, String> {
    let mut p = std::collections::HashMap::new();
    p.insert("trigger_build".to_string(), "confirm".to_string());
    p.insert("cancel_build".to_string(), "confirm".to_string());
    p.insert("*".to_string(), "auto".to_string());
    p
}

/// 组装 deploy-presets.json 的根 JSON：`{jenkinsUrl, credentials}`。
///
/// credentials 里 token 字段写 `keychain:` 占位（真值已在密钥链）；
/// 未填 token 的凭据也写占位（沿用已有密钥）。保证文件总能被
/// tools/deploy.rs 解析。
fn build_deploy_presets_json(jenkins_url: &str, credentials: &[CredentialInput]) -> Value {
    let creds: Vec<Value> = credentials
        .iter()
        .map(|c| {
            json!({
                "name": c.name.trim(),
                "username": c.username.trim(),
                "token": format!("keychain:{}", token_keychain_account(&c.name)),
                "projects": c.projects.iter().map(|p| json!({
                    "job": p.job,
                    "alias": p.alias,
                })).collect::<Vec<_>>(),
            })
        })
        .collect();
    json!({
        "jenkinsUrl": jenkins_url.trim(),
        "credentials": creds,
    })
}

/// 从旧 deploy-presets.json 的 credentials 数组提取凭据名集合（去重、小写）。
fn credential_names_from_json(value: &Value) -> Vec<String> {
    let mut names = std::collections::BTreeSet::new();
    if let Some(creds) = value.get("credentials").and_then(|v| v.as_array()) {
        for cred in creds {
            if let Some(name) = cred.get("name").and_then(|v| v.as_str()) {
                let n = name.trim().to_lowercase();
                if !n.is_empty() {
                    names.insert(n);
                }
            }
        }
    }
    names.into_iter().collect()
}

/// 原子写 JSON（pretty）到指定路径。复用项目既有的 util::write_atomic。
fn write_json_atomic(path: &std::path::Path, value: &Value) -> Result<(), String> {
    let content = serde_json::to_string_pretty(value)
        .map_err(|e| format!("序列化 {} 失败: {}", path.display(), e))?;
    crate::util::write_atomic(path, &content)
        .map_err(|e| format!("写入 {} 失败: {}", path.display(), e))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cred(name: &str, username: &str, token: Option<&str>, projects: Vec<ProjectInput>) -> CredentialInput {
        CredentialInput {
            name: name.to_string(),
            username: username.to_string(),
            token: token.map(|t| t.to_string()),
            projects,
        }
    }

    fn proj(job: &str, alias: &str) -> ProjectInput {
        ProjectInput {
            job: job.to_string(),
            alias: alias.to_string(),
        }
    }

    // ---- 凭据名校验 ----

    #[test]
    fn validate_credential_name_rules() {
        assert!(validate_credential_name("test").is_ok());
        assert!(validate_credential_name("prod-2").is_ok());
        assert!(validate_credential_name("Test").is_ok()); // 大小写都收，逻辑层再小写
        // 空 / 含空格 / 含点 → Err。
        assert!(validate_credential_name("").is_err());
        assert!(validate_credential_name("  ").is_err());
        assert!(validate_credential_name("my env").is_err());
        assert!(validate_credential_name("a.b").is_err());
        assert!(validate_credential_name("中文").is_err());
    }

    // ---- 项目别名必填 ----

    #[test]
    fn validate_project_aliases_rejects_empty() {
        let creds = vec![cred("acct-1", "user", Some("tok"), vec![proj("job-a", "")])];
        let e = validate_project_aliases(&creds).unwrap_err();
        assert!(e.contains("别名不能为空"), "实得: {}", e);
        assert!(e.contains("job-a"), "错误应指明是哪个 job: {}", e);
        // 纯空白也算空。
        let creds = vec![cred("acct-1", "user", Some("tok"), vec![proj("job-b", "  ")])];
        assert!(validate_project_aliases(&creds).is_err());
    }

    #[test]
    fn validate_project_aliases_ok() {
        let creds = vec![cred(
            "acct-1",
            "user",
            Some("tok"),
            vec![proj("job-a", "质量系统"), proj("job-b", "人资")],
        )];
        assert!(validate_project_aliases(&creds).is_ok());
        // 无项目的账号也 OK。
        assert!(validate_project_aliases(&[cred("acct-2", "u", None, vec![])]).is_ok());
    }

    // ---- env key 大写 / 逻辑名小写 / 密钥链 account ----

    #[test]
    fn env_key_and_account_casing() {
        assert_eq!(env_key_prefix("Test"), "JENKINS_ENV_TEST_");
        assert_eq!(env_key_prefix("prod"), "JENKINS_ENV_PROD_");
        assert_eq!(token_keychain_account("Test"), "jenkins-test-token");
        assert_eq!(token_keychain_account("PROD"), "jenkins-prod-token");
    }

    // ---- credentials → mcp env map：大写 key、token 走 keychain 占位、URL 全局 ----

    #[test]
    fn build_jenkins_config_env_shape() {
        let creds = vec![
            cred("test", "deploy-bot", Some("secret-t"), vec![]),
            cred("Prod", "prod-user", None, vec![]),
        ];
        let cfg = build_jenkins_server_config(
            r"D:\x\index.js",
            "http://jenkins.example.internal:8080/",
            &creds,
            true,
            Default::default(),
        );

        assert_eq!(cfg.command, "node");
        assert_eq!(cfg.args, vec![r"D:\x\index.js".to_string()]);
        assert!(cfg.enabled);

        // test 凭据：三件套，URL 用全局 jenkinsUrl，USERNAME 用凭据账号名，TOKEN 走 keychain 占位。
        assert_eq!(
            cfg.env.get("JENKINS_ENV_TEST_URL").unwrap(),
            "http://jenkins.example.internal:8080/"
        );
        assert_eq!(cfg.env.get("JENKINS_ENV_TEST_USERNAME").unwrap(), "deploy-bot");
        assert_eq!(
            cfg.env.get("JENKINS_ENV_TEST_TOKEN").unwrap(),
            "keychain:jenkins-test-token"
        );
        // Prod（大写名）→ env key 大写 PROD，占位 account 小写 prod。
        assert_eq!(
            cfg.env.get("JENKINS_ENV_PROD_URL").unwrap(),
            "http://jenkins.example.internal:8080/"
        );
        assert_eq!(cfg.env.get("JENKINS_ENV_PROD_USERNAME").unwrap(), "prod-user");
        assert_eq!(
            cfg.env.get("JENKINS_ENV_PROD_TOKEN").unwrap(),
            "keychain:jenkins-prod-token"
        );
        // 没有别的 env 串味（只这两凭据 → 6 个 key）。
        assert_eq!(cfg.env.len(), 6);
    }

    // ---- toolPolicy：空 → 安全默认；显式 → 保留 ----

    #[test]
    fn build_jenkins_config_empty_policy_gets_safe_default() {
        let cfg = build_jenkins_server_config(
            r"D:\x\index.js",
            "http://x",
            &[cred("acct-1", "u", Some("t"), vec![])],
            true,
            Default::default(), // 空策略
        );
        // 写操作需确认，只读（*）放行——否则 get_job_info 等会被默认 confirm 拦死。
        assert_eq!(
            cfg.tool_policy.get("trigger_build").map(String::as_str),
            Some("confirm")
        );
        assert_eq!(
            cfg.tool_policy.get("cancel_build").map(String::as_str),
            Some("confirm")
        );
        assert_eq!(cfg.tool_policy.get("*").map(String::as_str), Some("auto"));
    }

    #[test]
    fn build_jenkins_config_explicit_policy_preserved() {
        // 用户显式把一切设成 confirm → 不被默认覆盖。
        let mut explicit = std::collections::HashMap::new();
        explicit.insert("*".to_string(), "confirm".to_string());
        let cfg = build_jenkins_server_config(r"D:\x\index.js", "http://x", &[], true, explicit);
        assert_eq!(cfg.tool_policy.get("*").map(String::as_str), Some("confirm"));
        assert!(!cfg.tool_policy.contains_key("trigger_build"));
    }

    // ---- deploy-presets.json 形态：{jenkinsUrl, credentials} ----

    #[test]
    fn build_deploy_presets_shape() {
        let creds = vec![cred(
            "主账号",
            "zhang-san",
            Some("tok"),
            vec![proj("example-quality-web", "质量系统")],
        )];
        let out = build_deploy_presets_json("http://jenkins.example.internal:8080/", &creds);
        assert_eq!(out["jenkinsUrl"], "http://jenkins.example.internal:8080/");
        let credentials = out["credentials"].as_array().unwrap();
        assert_eq!(credentials.len(), 1);
        assert_eq!(credentials[0]["name"], "主账号");
        assert_eq!(credentials[0]["username"], "zhang-san");
        assert_eq!(
            credentials[0]["token"],
            "keychain:jenkins-主账号-token"
        );
        let projects = credentials[0]["projects"].as_array().unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0]["job"], "example-quality-web");
        assert_eq!(projects[0]["alias"], "质量系统");
    }

    // 与 deploy.rs 的读端做最小往返：build 出的 presets 能被同款 serde 模型解析回来。
    #[test]
    fn deploy_presets_roundtrips_with_reader_shape() {
        // 镜像 tools/deploy.rs 的读端模型（只取关心的字段），验证写出的 JSON 可解析。
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct ProjEntry {
            job: String,
            alias: String,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct CredEntry {
            name: String,
            token: String,
            projects: Vec<ProjEntry>,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Reader {
            jenkins_url: String,
            credentials: Vec<CredEntry>,
        }

        let creds = vec![cred(
            "主账号",
            "zhang-san",
            Some("tok"),
            vec![
                proj("example-quality-web", "质量系统"),
                proj("example-access-web", "人资管理端"),
            ],
        )];
        let presets = build_deploy_presets_json("http://jenkins.example.internal:8080/", &creds);
        let text = serde_json::to_string(&presets).unwrap();
        let parsed: Reader = serde_json::from_str(&text).expect("deploy.rs 读端应能解析");
        assert_eq!(parsed.jenkins_url, "http://jenkins.example.internal:8080/");
        assert_eq!(parsed.credentials.len(), 1);
        let cred = &parsed.credentials[0];
        assert_eq!(cred.name, "主账号");
        assert_eq!(cred.token, "keychain:jenkins-主账号-token");
        assert_eq!(cred.projects.len(), 2);
        assert_eq!(cred.projects[0].job, "example-quality-web");
        assert_eq!(cred.projects[0].alias, "质量系统");
    }

    // ---- mcp-servers.json 往返：保留其它 server、jenkins 被重写 ----

    #[test]
    fn save_preserves_other_servers_and_rewrites_jenkins() {
        // 现有配置有 jenkins（旧 env）+ 一个无关 server "weather"。
        let existing_json = r#"{
            "servers": {
                "jenkins": {
                    "command": "node",
                    "args": ["D:\\old\\index.js"],
                    "env": { "JENKINS_ENV_OLD_URL": "http://old", "JENKINS_ENV_OLD_USERNAME": "x",
                             "JENKINS_ENV_OLD_TOKEN": "keychain:jenkins-old-token" },
                    "enabled": true,
                    "toolPolicy": { "trigger_build": "confirm", "*": "auto" }
                },
                "weather": { "command": "node", "args": ["weather.js"], "enabled": false }
            }
        }"#;
        let mut mcp_cfg: crate::mcp_client::McpServersConfig =
            serde_json::from_str(existing_json).unwrap();

        // 模拟 save 的核心：重建 jenkins 并 insert（保留 enabled）。
        let creds = vec![cred("test", "deploy-bot", Some("tok"), vec![])];
        let old = mcp_cfg.servers.get(JENKINS_SERVER_ID).cloned();
        let path = resolve_jenkins_mcp_path(None, old.as_ref());
        assert_eq!(path, r"D:\old\index.js");

        let jenkins_cfg = build_jenkins_server_config(
            &path,
            "http://jenkins.example.internal:8080/",
            &creds,
            old.as_ref().map(|c| c.enabled).unwrap_or(true),
            old.as_ref()
                .map(|c| c.tool_policy.clone())
                .unwrap_or_default(),
        );
        mcp_cfg
            .servers
            .insert(JENKINS_SERVER_ID.to_string(), jenkins_cfg);

        // 其它 server 原封不动。
        let weather = mcp_cfg.servers.get("weather").expect("weather 应保留");
        assert!(!weather.enabled);
        assert_eq!(weather.args, vec!["weather.js".to_string()]);

        // jenkins 被新 env 重写：旧 OLD 三件套没了，新 TEST 三件套在。
        let jenkins = mcp_cfg.servers.get("jenkins").unwrap();
        assert!(!jenkins.env.contains_key("JENKINS_ENV_OLD_URL"));
        assert_eq!(
            jenkins.env.get("JENKINS_ENV_TEST_URL").unwrap(),
            "http://jenkins.example.internal:8080/"
        );
        assert_eq!(
            jenkins.env.get("JENKINS_ENV_TEST_TOKEN").unwrap(),
            "keychain:jenkins-test-token"
        );
        // toolPolicy 必须沿用旧值（设置页不编辑它，但保存时绝不能抹掉用户的 auto 降级）。
        assert_eq!(
            jenkins.tool_policy.get("trigger_build").map(String::as_str),
            Some("confirm")
        );
        assert_eq!(jenkins.tool_policy.get("*").map(String::as_str), Some("auto"));
    }

    // ---- jenkinsMcpPath 解析优先级：入参 > 现有 args[0] > 默认 ----

    #[test]
    fn resolve_path_priority() {
        let old_json = r#"{ "command": "node", "args": ["D:\\old\\index.js"] }"#;
        let old: McpServerConfig = serde_json::from_str(old_json).unwrap();

        assert_eq!(
            resolve_jenkins_mcp_path(Some("D:\\new\\index.js"), Some(&old)),
            "D:\\new\\index.js"
        );
        assert_eq!(
            resolve_jenkins_mcp_path(Some("   "), Some(&old)),
            "D:\\old\\index.js"
        );
        assert_eq!(resolve_jenkins_mcp_path(None, Some(&old)), "D:\\old\\index.js");
        assert_eq!(
            resolve_jenkins_mcp_path(None, None),
            DEFAULT_JENKINS_MCP_PATH
        );
    }

    // ---- credential_names_from_json：从旧配置提取凭据名 ----

    #[test]
    fn credential_names_from_json_parses() {
        let value = json!({
            "jenkinsUrl": "http://x",
            "credentials": [
                { "name": "主账号", "token": "k:x", "projects": [] },
                { "name": "Prod", "token": "k:y", "projects": [] }
            ]
        });
        let names = credential_names_from_json(&value);
        assert_eq!(names, vec!["prod".to_string(), "主账号".to_string()]); // BTreeSet 排序
    }

    #[test]
    fn credential_names_from_json_empty() {
        let names = credential_names_from_json(&json!({}));
        assert!(names.is_empty());
    }
}
