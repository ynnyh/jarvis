// 发版配置设置页后端：读写 deploy-presets.json + mcp-servers.json + token 落密钥链
// + 保存后重启 jenkins-mcp + 测试连接。
//
// 这是 jenkins-deploy 任务「设置页后端」PR：让前端能在 UI 里读写发版配置，
// 不必再手改两个 JSON、token 也有了安全入口。前端 UI 是下一个 PR，本模块只做后端。
//
// ---- 两个配置文件、两个消费者（直接复用，勿另起炉灶）----
//   - ~/.jarvis/deploy-presets.json：{server:"jenkins", projects:{...}}。
//     由 tools/deploy.rs 的 load_deploy_config() 消费（对话式发版的预设来源）。
//   - ~/.jarvis/mcp-servers.json：{servers:{jenkins:{command,args,env,enabled}}}。
//     由 mcp_client.rs 消费（spawn MCP 子进程；env 里 `keychain:` 前缀的值在 spawn
//     时经 resolve_env_value 从密钥链解出）。路径函数 mcp_servers_config_path() 已存在。
//
// ---- jenkins-mcp env 模型（已读其 src/index.ts 核实）----
//   每个环境要三件套 JENKINS_ENV_<NAME>_URL / _USERNAME / _TOKEN（三者齐全才生效），
//   生成的环境名 = <NAME>.toLowerCase()。这个小写名必须等于 deploy-presets 里
//   environment 条目的 jenkinsEnvironment，也等于 trigger_build 的 environment 参数。
//   故约定：连接逻辑名一律小写（hasToken/test_connection 用它），写进 env key 时大写。
//
// ---- token 安全 ----
//   token 绝不返回明文、绝不落明文配置。env 里只写占位 `keychain:jenkins-<name>-token`，
//   真值经 secret_set 存进 OS 密钥链（与 LLM apiKey 同一套 settings::secret_* 机制）。
//   spawn 时 mcp_client::resolve_env_value 才把占位解成真值注入子进程。

use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::mcp_client::{load_mcp_servers_config, mcp_servers_config_path, McpServerConfig};

/// jenkins-mcp 在用户本机的默认入口（机器相关，故可配；仅当配置里没有时兜底）。
const DEFAULT_JENKINS_MCP_PATH: &str = r"D:\coding\my-mcp-servers\jenkins-mcp\dist\index.js";

/// mcp-servers.json 里 Jenkins server 的固定 id。
const JENKINS_SERVER_ID: &str = "jenkins";

// ============================================================================
// deploy-presets.json 路径（与 tools/deploy.rs 私有的 deploy_presets_path 同址）
// ============================================================================
//
// tools/deploy.rs 把 deploy_presets_path() 设为私有，故这里独立给一份同址实现，
// 避免把那边的私有函数提成 pub 而扩大暴露面（两处都从 settings::jarvis_dir() 拼，
// 路径一致，是同一个文件）。

fn deploy_presets_path() -> std::path::PathBuf {
    crate::settings::jarvis_dir().join("deploy-presets.json")
}

// ============================================================================
// 连接模型（前端 PR2 依赖的 JSON 契约）
// ============================================================================

/// 单个 Jenkins 连接（= 一个账号 + 一个环境）。
///
/// 逻辑名 `name` 是用户/前端用的标识；写进 mcp env key 时大写、查密钥链时小写。
/// `token` 仅在用户新填/改了密码时才带（缺省/空 = 不动已有密钥，绝不覆盖成空）。
#[derive(Debug, Clone, Deserialize)]
struct ConnectionInput {
    #[serde(default)]
    name: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    username: String,
    #[serde(default)]
    token: Option<String>,
}

/// deploy_config_save 的入参。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SaveInput {
    /// jenkins-mcp 入口路径；缺省沿用现有 args[0]，再缺省用默认路径。
    #[serde(default)]
    jenkins_mcp_path: Option<String>,
    #[serde(default)]
    connections: Vec<ConnectionInput>,
    /// 原样写进 deploy-presets.json 的 projects（结构由前端/deploy.rs 约定，这里不解构）。
    #[serde(default)]
    projects: Value,
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

    let server_configured = jenkins.is_some();

    // 从 env 反解连接列表；hasToken 现读密钥链（持有真值的唯一来源）。
    let connections = jenkins
        .map(|c| connections_view_from_env(&c.env))
        .unwrap_or_default();

    // deploy-presets.json：projects 原样透传（文件缺失/坏 JSON 都按空）。
    let projects = load_deploy_projects().unwrap_or_else(|| json!({}));

    Ok(json!({
        "jenkinsMcpPath": jenkins_mcp_path,
        "serverConfigured": server_configured,
        "connections": connections,
        "projects": projects,
    }))
}

/// 读 deploy-presets.json 的 projects 段。文件不存在/坏 JSON → None（按空处理）。
///
/// deploy_config_get 是只读视图，坏配置不该让设置页打不开（用户正要来修），故宽容；
/// 真正发版时 tools/deploy.rs::load_deploy_config 会对坏 JSON 报错，那里才是该严的地方。
fn load_deploy_projects() -> Option<Value> {
    let content = std::fs::read_to_string(deploy_presets_path()).ok()?;
    let value: Value = serde_json::from_str(&content).ok()?;
    value.get("projects").cloned()
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
    let parsed: SaveInput =
        serde_json::from_value(input).map_err(|e| format!("发版配置入参错误: {}", e))?;

    // 1. 校验每个连接名（字母数字/-、非空），并规范成小写逻辑名。
    //    name 必须能稳定映射到 jenkinsEnvironment，故先校验再用。
    for c in &parsed.connections {
        validate_connection_name(&c.name)?;
    }

    // 2. 读现有 mcp-servers.json（保留 jenkins 以外的其它 server 不动）。
    let mut mcp_cfg = load_mcp_servers_config()?;
    let old_jenkins = mcp_cfg.servers.get(JENKINS_SERVER_ID).cloned();

    // 3. 解析 jenkinsMcpPath：入参优先 → 现有 args[0] → 默认路径。
    let jenkins_mcp_path = resolve_jenkins_mcp_path(
        parsed.jenkins_mcp_path.as_deref(),
        old_jenkins.as_ref(),
    );

    // 4. token 落密钥链：仅对带非空 token 的连接 secret_set；token 缺省/空 → 不动该 key
    //    的已有密钥（保留），绝不覆盖成空。这一步在写盘前做，密钥链是 token 的唯一真值来源。
    for c in &parsed.connections {
        if let Some(token) = c.token.as_deref() {
            let token = token.trim();
            if !token.is_empty() {
                let account = token_keychain_account(&c.name);
                crate::settings::secret_set(&account, token)?;
            }
        }
    }

    // 5. 被删掉的连接：从 env 移除其三件套（下面重建 env 时自然不含）；其密钥链 secret
    //    一并清掉（用 secret_clear，settings 提供了删除接口），避免遗留无主密钥。
    if let Some(old) = &old_jenkins {
        let old_names = connection_names_from_env(&old.env);
        let new_names: std::collections::HashSet<String> = parsed
            .connections
            .iter()
            .map(|c| c.name.trim().to_lowercase())
            .collect();
        for old_name in old_names {
            if !new_names.contains(&old_name) {
                let account = token_keychain_account(&old_name);
                if let Err(e) = crate::settings::secret_clear(&account) {
                    // 清理失败不致命（密钥只是遗留，env 已不再引用），但留痕便于排查。
                    eprintln!("[deploy_config] 清理已删除连接的密钥 '{}' 失败: {}", account, e);
                }
            }
        }
    }

    // 6. 重建 jenkins server：保留 enabled（沿用旧值，缺省 true）与旧 toolPolicy
    //    （设置页不管 toolPolicy，但绝不能在保存时把用户已有的策略悄悄抹掉——
    //    抹掉会让 trigger_build 等退回默认 confirm 倒不危险，但会丢掉用户对只读工具
    //    的 auto 降级，故沿用旧值），其余按新连接重写。
    let jenkins_cfg = build_jenkins_server_config(
        &jenkins_mcp_path,
        &parsed.connections,
        old_jenkins.as_ref().map(|c| c.enabled).unwrap_or(true),
        old_jenkins
            .as_ref()
            .map(|c| c.tool_policy.clone())
            .unwrap_or_default(),
    );
    mcp_cfg
        .servers
        .insert(JENKINS_SERVER_ID.to_string(), jenkins_cfg.clone());

    // 7. 写 deploy-presets.json：{server:"jenkins", projects}，原子写。
    let presets = build_deploy_presets_json(&parsed.projects);
    write_json_atomic(&deploy_presets_path(), &presets)?;

    // 8. 写 mcp-servers.json，原子写。
    let mcp_value = serde_json::to_value(&mcp_cfg)
        .map_err(|e| format!("序列化 mcp-servers.json 失败: {}", e))?;
    write_json_atomic(&mcp_servers_config_path(), &mcp_value)?;

    // 9. 重启 jenkins-mcp：先关（没在跑就忽略），再起。spawn 失败（坏 token/路径/握手失败）
    //    把错误返回给前端，让用户立刻知道——此时配置已写盘，属正常。
    let mgr = crate::mcp_client::manager();
    if let Err(e) = mgr.shutdown_server(JENKINS_SERVER_ID).await {
        // 关停失败不阻断重启（可能本就没在跑）；留痕即可。
        eprintln!("[deploy_config] 重启前关停 jenkins 失败（可能未在运行）: {}", e);
    }
    mgr.spawn_server(JENKINS_SERVER_ID, &jenkins_cfg).await?;

    Ok(())
}

// ============================================================================
// 命令 3：deploy_test_connection —— 调 jenkins-mcp 的 test_connection
// ============================================================================

/// 测试某个连接：确保 jenkins 在跑，调 test_connection 工具，按两层错误回报。
#[tauri::command]
pub async fn deploy_test_connection(name: String) -> Result<Value, String> {
    let env_name = name.trim().to_lowercase();
    if env_name.is_empty() {
        return Err("必须指定要测试的连接名".to_string());
    }

    let mgr = crate::mcp_client::manager();

    // 确保 jenkins 在跑：未连接则先 spawn（spawn_server 幂等，已在跑直接返回）。
    let connected = mgr.connected_ids().await;
    if !connected.iter().any(|id| id == JENKINS_SERVER_ID) {
        let cfg = load_mcp_servers_config()?;
        match cfg.servers.get(JENKINS_SERVER_ID) {
            Some(jenkins_cfg) => mgr.spawn_server(JENKINS_SERVER_ID, jenkins_cfg).await?,
            None => return Err("尚未配置 jenkins 连接，请先保存发版配置".to_string()),
        }
    }

    // 调 test_connection，显式带小写 environment（杜绝 jenkins-mcp 默认 envs[0]）。
    let mut args = Map::new();
    args.insert("environment".to_string(), Value::String(env_name.clone()));
    let result = mgr
        .call_tool(JENKINS_SERVER_ID, "test_connection", Some(args))
        .await?;

    // 两层错误（见 mcp-client.md §3.3）：传输错已是上面的 `?`；工具自身失败 → is_error。
    let text = crate::mcp_client::first_text(&result).unwrap_or_default();
    if result.is_error == Some(true) {
        let err = if text.is_empty() {
            format!("连接 {} 测试失败", env_name)
        } else {
            text
        };
        return Err(err);
    }

    Ok(json!({ "ok": true, "detail": text }))
}

// ============================================================================
// 纯函数（不碰磁盘/密钥链，可直接单测真实代码路径）
// ============================================================================

/// 连接逻辑名 → 密钥链 account：`jenkins-<name小写>-token`。
///
/// 必须与 mcp env 里写的占位 `keychain:jenkins-<name小写>-token` 对应，
/// 这样 spawn 时 resolve_env_value 才能从同一 account 取到真值。
fn token_keychain_account(name: &str) -> String {
    format!("jenkins-{}-token", name.trim().to_lowercase())
}

/// 连接逻辑名 → env key 前缀里的大写段：`JENKINS_ENV_<NAME大写>_`。
fn env_key_prefix(name: &str) -> String {
    format!("JENKINS_ENV_{}_", name.trim().to_uppercase())
}

/// 校验连接名：非空，且仅含字母/数字/`-`。
///
/// 限定简单标识符是为了 env key 大写、逻辑名小写两套规则都稳定可逆，避免和
/// deploy-presets 里的 jenkinsEnvironment 对不上（含空格/点会破坏映射）。
fn validate_connection_name(name: &str) -> Result<(), String> {
    let n = name.trim();
    if n.is_empty() {
        return Err("连接名不能为空".to_string());
    }
    if !n.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err(format!("连接名 '{}' 只能包含字母、数字和连字符(-)", n));
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

/// 用连接列表重建 jenkins server 的 spawn 配置。
///
/// env 对每个连接 c 写三件套（token 只写 `keychain:` 占位，真值在密钥链）：
///   JENKINS_ENV_<大写>_URL / _USERNAME / _TOKEN
fn build_jenkins_server_config(
    jenkins_mcp_path: &str,
    connections: &[ConnectionInput],
    enabled: bool,
    tool_policy: std::collections::HashMap<String, String>,
) -> McpServerConfig {
    let mut env = std::collections::HashMap::new();
    for c in connections {
        let prefix = env_key_prefix(&c.name);
        env.insert(format!("{}URL", prefix), c.url.trim().to_string());
        env.insert(
            format!("{}USERNAME", prefix),
            c.username.trim().to_string(),
        );
        // token 永不落明文：env 里只放 keychain 占位，真值在密钥链。
        env.insert(
            format!("{}TOKEN", prefix),
            format!("keychain:{}", token_keychain_account(&c.name)),
        );
    }
    McpServerConfig {
        command: "node".to_string(),
        args: vec![jenkins_mcp_path.to_string()],
        env,
        enabled,
        // toolPolicy 由调用方传入（沿用旧配置）：设置页不编辑它，但保存时必须保留，
        // 否则用户对只读工具的 auto 降级会被悄悄抹掉。新建（无旧配置）时为空 map，
        // 即 trigger_build 等落默认 confirm，安全。
        tool_policy,
    }
}

/// 组装 deploy-presets.json 的根 JSON：`{server:"jenkins", projects}`。
///
/// projects 原样透传（前端给什么写什么）；缺省/非对象时落空对象，保证文件总能被
/// tools/deploy.rs 解析（其 projects 字段 `#[serde(default)]`）。
fn build_deploy_presets_json(projects: &Value) -> Value {
    let projects = if projects.is_object() {
        projects.clone()
    } else {
        json!({})
    };
    json!({
        "server": JENKINS_SERVER_ID,
        "projects": projects,
    })
}

/// 从 jenkins env 反解出连接逻辑名集合（去重、小写）。
///
/// 识别规则：凡 `JENKINS_ENV_<NAME>_URL` 形态的 key，取中段 <NAME> 小写为逻辑名。
/// 只认 _URL 后缀做锚点（每连接必有 URL），避免同一连接的三件套被数三次。
fn connection_names_from_env(env: &std::collections::HashMap<String, String>) -> Vec<String> {
    let mut names = std::collections::BTreeSet::new();
    for key in env.keys() {
        if let Some(mid) = key
            .strip_prefix("JENKINS_ENV_")
            .and_then(|r| r.strip_suffix("_URL"))
        {
            if !mid.is_empty() {
                names.insert(mid.to_lowercase());
            }
        }
    }
    names.into_iter().collect()
}

/// 从 jenkins env 反解出连接的只读视图列表（按逻辑名排序，hasToken 查密钥链）。
///
/// 注意：这里会读密钥链（secret_get 判 token 是否非空），故不是纯函数。把它和纯解析
/// 拆开是为了让纯解析（connection_names_from_env / env 取值）能脱离密钥链单测。
fn connections_view_from_env(env: &std::collections::HashMap<String, String>) -> Vec<Value> {
    let names = connection_names_from_env(env);
    names
        .into_iter()
        .map(|name| {
            let prefix = env_key_prefix(&name);
            let url = env
                .get(&format!("{}URL", prefix))
                .cloned()
                .unwrap_or_default();
            let username = env
                .get(&format!("{}USERNAME", prefix))
                .cloned()
                .unwrap_or_default();
            // hasToken：env 里的 TOKEN 值是 `keychain:<account>` 占位，解出 account 后查
            // 密钥链是否非空。直接对 account 调 secret_get（非空即 hasToken=true）。
            let account = token_keychain_account(&name);
            let has_token = crate::settings::secret_get(&account).is_some();
            json!({
                "name": name,
                "url": url,
                "username": username,
                "hasToken": has_token,
            })
        })
        .collect()
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
    use std::collections::BTreeMap;

    fn conn(name: &str, url: &str, user: &str, token: Option<&str>) -> ConnectionInput {
        ConnectionInput {
            name: name.to_string(),
            url: url.to_string(),
            username: user.to_string(),
            token: token.map(|t| t.to_string()),
        }
    }

    // ---- 连接名校验 ----

    #[test]
    fn validate_connection_name_rules() {
        assert!(validate_connection_name("test").is_ok());
        assert!(validate_connection_name("prod-2").is_ok());
        assert!(validate_connection_name("Test").is_ok()); // 大小写都收，逻辑层再小写
        // 空 / 含空格 / 含点 → Err。
        assert!(validate_connection_name("").is_err());
        assert!(validate_connection_name("  ").is_err());
        assert!(validate_connection_name("my env").is_err());
        assert!(validate_connection_name("a.b").is_err());
        assert!(validate_connection_name("中文").is_err());
    }

    // ---- env key 大写 / 逻辑名小写 / 密钥链 account ----

    #[test]
    fn env_key_and_account_casing() {
        // env key 段大写。
        assert_eq!(env_key_prefix("Test"), "JENKINS_ENV_TEST_");
        assert_eq!(env_key_prefix("prod"), "JENKINS_ENV_PROD_");
        // 密钥链 account 小写。
        assert_eq!(token_keychain_account("Test"), "jenkins-test-token");
        assert_eq!(token_keychain_account("PROD"), "jenkins-prod-token");
    }

    // ---- connections → mcp env map：大写 key、token 走 keychain 占位 ----

    #[test]
    fn build_jenkins_config_env_shape() {
        let conns = vec![
            conn("test", "http://t.local", "alice", Some("secret-t")),
            conn("Prod", "http://p.local", "bob", None),
        ];
        let cfg = build_jenkins_server_config(r"D:\x\index.js", &conns, true, Default::default());

        assert_eq!(cfg.command, "node");
        assert_eq!(cfg.args, vec![r"D:\x\index.js".to_string()]);
        assert!(cfg.enabled);

        // test 连接：三件套，URL/USERNAME 字面、TOKEN 走 keychain 占位（小写 account）。
        assert_eq!(cfg.env.get("JENKINS_ENV_TEST_URL").unwrap(), "http://t.local");
        assert_eq!(cfg.env.get("JENKINS_ENV_TEST_USERNAME").unwrap(), "alice");
        assert_eq!(
            cfg.env.get("JENKINS_ENV_TEST_TOKEN").unwrap(),
            "keychain:jenkins-test-token"
        );
        // Prod（大写名）→ env key 大写 PROD，占位 account 小写 prod。
        assert_eq!(cfg.env.get("JENKINS_ENV_PROD_URL").unwrap(), "http://p.local");
        assert_eq!(
            cfg.env.get("JENKINS_ENV_PROD_TOKEN").unwrap(),
            "keychain:jenkins-prod-token"
        );
        // 没有别的 server 的 env 串味（只这两连接 → 6 个 key）。
        assert_eq!(cfg.env.len(), 6);
    }

    // ---- deploy-presets.json 形态：{server, projects}，projects 原样透传 ----

    #[test]
    fn build_deploy_presets_passthrough_projects() {
        let projects = json!({
            "人资管理端": {
                "environments": {
                    "test": { "job": "example-access-web", "jenkinsEnvironment": "test",
                              "params": { "branch": "dev" } }
                }
            }
        });
        let out = build_deploy_presets_json(&projects);
        assert_eq!(out["server"], "jenkins");
        assert_eq!(out["projects"], projects);

        // 非对象 projects（null/数组）→ 落空对象，保证可被 deploy.rs 解析。
        assert_eq!(build_deploy_presets_json(&Value::Null)["projects"], json!({}));
        assert_eq!(
            build_deploy_presets_json(&json!([1, 2]))["projects"],
            json!({})
        );
    }

    // 与 deploy.rs 的读端做最小往返：build 出的 presets 能被同款 serde 模型解析回来。
    #[test]
    fn deploy_presets_roundtrips_with_reader_shape() {
        // 镜像 tools/deploy.rs 的读端模型（只取关心的字段），验证写出的 JSON 可解析。
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct EnvPreset {
            job: String,
            jenkins_environment: String,
            #[serde(default)]
            params: BTreeMap<String, String>,
        }
        #[derive(Deserialize)]
        struct ProjPreset {
            #[serde(default)]
            environments: BTreeMap<String, EnvPreset>,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Reader {
            server: String,
            #[serde(default)]
            projects: BTreeMap<String, ProjPreset>,
        }

        let projects = json!({
            "人资管理端": {
                "environments": {
                    "test": { "job": "example-access-web", "jenkinsEnvironment": "test",
                              "params": { "branch": "dev", "server_ip": "192.0.2.23" } }
                }
            }
        });
        let presets = build_deploy_presets_json(&projects);
        let text = serde_json::to_string(&presets).unwrap();
        let parsed: Reader = serde_json::from_str(&text).expect("deploy.rs 读端应能解析");
        assert_eq!(parsed.server, "jenkins");
        let proj = parsed.projects.get("人资管理端").expect("项目");
        let env = proj.environments.get("test").expect("test 环境");
        assert_eq!(env.job, "example-access-web");
        assert_eq!(env.jenkins_environment, "test");
        assert_eq!(env.params.get("branch").map(String::as_str), Some("dev"));
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
        let conns = vec![conn("test", "http://t", "alice", Some("tok"))];
        let old = mcp_cfg.servers.get(JENKINS_SERVER_ID).cloned();
        let path = resolve_jenkins_mcp_path(None, old.as_ref());
        // 没传 path → 沿用旧 args[0]。
        assert_eq!(path, r"D:\old\index.js");

        let jenkins_cfg = build_jenkins_server_config(
            &path,
            &conns,
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
        assert!(jenkins.env.get("JENKINS_ENV_OLD_URL").is_none());
        assert_eq!(jenkins.env.get("JENKINS_ENV_TEST_URL").unwrap(), "http://t");
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

        // 入参优先。
        assert_eq!(
            resolve_jenkins_mcp_path(Some("D:\\new\\index.js"), Some(&old)),
            "D:\\new\\index.js"
        );
        // 入参空白 → 退到现有 args[0]。
        assert_eq!(
            resolve_jenkins_mcp_path(Some("   "), Some(&old)),
            "D:\\old\\index.js"
        );
        assert_eq!(resolve_jenkins_mcp_path(None, Some(&old)), "D:\\old\\index.js");
        // 都没有 → 默认路径。
        assert_eq!(
            resolve_jenkins_mcp_path(None, None),
            DEFAULT_JENKINS_MCP_PATH
        );
    }

    // ---- env 反解连接名：按 _URL 锚点、去重、小写 ----

    #[test]
    fn connection_names_from_env_dedups_by_url_anchor() {
        let mut env = std::collections::HashMap::new();
        // test 连接三件套（只 _URL 当锚点 → 只数一次）。
        env.insert("JENKINS_ENV_TEST_URL".to_string(), "http://t".to_string());
        env.insert("JENKINS_ENV_TEST_USERNAME".to_string(), "a".to_string());
        env.insert("JENKINS_ENV_TEST_TOKEN".to_string(), "keychain:x".to_string());
        // prod 连接只有 URL（半配）也能识别。
        env.insert("JENKINS_ENV_PROD_URL".to_string(), "http://p".to_string());
        // 无关 env 不被误认。
        env.insert("SOME_OTHER".to_string(), "v".to_string());

        let names = connection_names_from_env(&env);
        assert_eq!(names, vec!["prod".to_string(), "test".to_string()]); // BTreeSet 排序
    }

    // ---- 两文件缺失按空：build 出的视图字段齐全（hasToken 等不靠真实 ~/.jarvis）----

    #[test]
    fn empty_inputs_yield_empty_collections() {
        // 空 env → 空连接列表（纯解析，不碰密钥链）。
        let empty: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        assert!(connection_names_from_env(&empty).is_empty());
        // 空 projects → 空对象。
        assert_eq!(build_deploy_presets_json(&json!({}))["projects"], json!({}));
    }
}
