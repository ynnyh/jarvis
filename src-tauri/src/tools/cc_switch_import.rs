use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

use serde::Serialize;
use serde_json::Value;

// rusqlite 的 Option 风格 query_row 需要 OptionalExtension
use rusqlite::OptionalExtension;

// ============================================================================
// cc_switch_import
// ============================================================================

#[derive(Debug, Serialize)]
struct CcImportResult {
    found: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider: Option<CcImportProvider>,
}

#[derive(Debug, Serialize)]
struct CcImportProvider {
    name: String,
    #[serde(rename = "apiKey")]
    api_key: String,
    #[serde(rename = "baseUrl")]
    base_url: String,
    model: String,
    #[serde(rename = "wireApi", skip_serializing_if = "Option::is_none")]
    wire_api: Option<String>,
}

pub(crate) async fn cc_switch_import(_input: Value) -> Result<Value, String> {
    let cc_dir = home_dir().join(".cc-switch");
    let settings_path = cc_dir.join("settings.json");
    let db_path = cc_dir.join("cc-switch.db");

    if !settings_path.exists() || !db_path.exists() {
        return Ok(serde_json::to_value(CcImportResult {
            found: false,
            reason: Some("未检测到 CC Switch（~/.cc-switch/ 目录不完整）".into()),
            provider: None,
        })
        .unwrap());
    }

    let current_id: String = {
        let raw = std::fs::read_to_string(&settings_path)
            .map_err(|e| format!("CC Switch settings.json 读取失败: {}", e))?;
        let json: Value = serde_json::from_str(&raw)
            .map_err(|e| format!("CC Switch settings.json 解析失败: {}", e))?;
        match json.get("currentProviderCodex").and_then(|v| v.as_str()) {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => return Ok(serde_json::to_value(CcImportResult {
                found: false,
                reason: Some(
                    "CC Switch 没有选定的 Codex（OpenAI）provider，请先在 CC Switch 里切换到一个"
                        .into(),
                ),
                provider: None,
            })
            .unwrap()),
        }
    };

    // 打开 SQLite 只读（rusqlite 没有显式 readonly flag，open 后只 select 即可）
    let conn = rusqlite::Connection::open(&db_path)
        .map_err(|e| format!("打开 CC Switch 数据库失败: {}", e))?;
    let row: Option<(String, String, String)> = conn
        .query_row(
            "SELECT id, name, settings_config FROM providers WHERE id = ?1 AND app_type = ?2",
            rusqlite::params![current_id, "codex"],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(|e| format!("查询 CC Switch provider 失败: {}", e))?;

    let (_id, name, settings_config) = match row {
        Some(r) => r,
        None => {
            return Ok(serde_json::to_value(CcImportResult {
                found: false,
                reason: Some(format!(
                    "在 CC Switch 数据库里找不到当前 Codex provider (id={})",
                    current_id
                )),
                provider: None,
            })
            .unwrap())
        }
    };

    let config: Value = serde_json::from_str(&settings_config)
        .map_err(|e| format!("CC Switch provider 的 settings_config 不是合法 JSON: {}", e))?;
    let api_key = config
        .get("auth")
        .and_then(|v| v.get("OPENAI_API_KEY"))
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let api_key = match api_key {
        Some(k) => k,
        None => {
            return Ok(serde_json::to_value(CcImportResult {
                found: false,
                reason: Some(format!(
                    "CC Switch provider 「{}」未配置 OPENAI_API_KEY",
                    name
                )),
                provider: None,
            })
            .unwrap())
        }
    };

    let toml_text = config
        .get("config")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let parsed = parse_codex_toml(&toml_text);
    let base_url = match parsed.base_url {
        Some(b) => b,
        None => {
            return Ok(serde_json::to_value(CcImportResult {
                found: false,
                reason: Some(format!(
                    "CC Switch provider 「{}」的 base_url 解析失败",
                    name
                )),
                provider: None,
            })
            .unwrap())
        }
    };

    Ok(serde_json::to_value(CcImportResult {
        found: true,
        reason: None,
        provider: Some(CcImportProvider {
            name,
            api_key,
            base_url,
            model: parsed.model.unwrap_or_else(|| "gpt-4o-mini".into()),
            wire_api: parsed.wire_api,
        }),
    })
    .unwrap())
}

struct CodexTomlParsed {
    model: Option<String>,
    base_url: Option<String>,
    provider_name: Option<String>,
    wire_api: Option<String>,
}

/// 从 Codex CLI 风格 TOML 抠 model / model_provider / 对应 section 的 base_url + wire_api。
/// 只识别 `key = "value"` 简单形式（CC Switch 写出来的 TOML 是这个形态）。
fn parse_codex_toml(text: &str) -> CodexTomlParsed {
    use regex::Regex;
    // 顶层块：第一个 [section] 之前
    let top_re = Regex::new(r"(?m)^\s*\[[^\]]+\]\s*$").unwrap();
    let top_block = top_re.splitn(text, 2).next().unwrap_or(text);

    let model = match_string(top_block, r#"(?m)^\s*model\s*=\s*"([^"]+)""#);
    let provider_name = match_string(top_block, r#"(?m)^\s*model_provider\s*=\s*"([^"]+)""#);

    let mut base_url: Option<String> = None;
    let mut wire_api: Option<String> = None;
    if let Some(p) = provider_name.as_deref() {
        let escaped = regex::escape(p);
        let section_re = format!(r"\[model_providers\.{}\]([\s\S]*?)(?:\n\[|$)", escaped);
        if let Ok(re) = Regex::new(&section_re) {
            if let Some(cap) = re.captures(text) {
                let section = cap.get(1).map(|m| m.as_str()).unwrap_or("");
                base_url = match_string(section, r#"(?m)^\s*base_url\s*=\s*"([^"]+)""#);
                wire_api = match_string(section, r#"(?m)^\s*wire_api\s*=\s*"([^"]+)""#);
            }
        }
    }
    if base_url.is_none() {
        base_url = match_string(text, r#"(?m)^\s*base_url\s*=\s*"([^"]+)""#);
    }

    CodexTomlParsed {
        model,
        base_url,
        provider_name,
        wire_api,
    }
}

fn match_string(text: &str, pattern: &str) -> Option<String> {
    let re = regex::Regex::new(pattern).ok()?;
    re.captures(text)
        .and_then(|c| c.get(1).map(|m| m.as_str().to_string()))
}

// ============================================================================
// CC Switch 全量 provider 扫描 + 批量导入
// ============================================================================

#[derive(Debug, Serialize)]
pub(crate) struct CcSwitchProviderSummary {
    pub id: String,
    pub name: String,
    #[serde(rename = "appType")]
    pub app_type: String,
    #[serde(rename = "baseUrl")]
    pub base_url: String,
    pub model: String,
    #[serde(rename = "wireApi")]
    pub wire_api: String,
    #[serde(rename = "hasApiKey")]
    pub has_api_key: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct CcSwitchImportResult {
    pub id: String,
    pub name: String,
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(rename = "alreadyExists", skip_serializing_if = "Option::is_none")]
    pub already_exists: Option<bool>,
}

/// 扫描 CC Switch SQLite 中全部 providers，按 app_type 返回摘要列表。
pub(crate) fn list_cc_switch_providers() -> Result<Vec<CcSwitchProviderSummary>, String> {
    let db_path = home_dir().join(".cc-switch").join("cc-switch.db");
    if !db_path.exists() {
        return Ok(vec![]);
    }

    let conn = rusqlite::Connection::open(&db_path)
        .map_err(|e| format!("打开 CC Switch 数据库失败: {}", e))?;

    let mut stmt = conn
        .prepare("SELECT id, name, app_type, settings_config FROM providers")
        .map_err(|e| format!("准备查询语句失败: {}", e))?;

    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .map_err(|e| format!("查询 CC Switch providers 失败: {}", e))?;

    let mut providers = Vec::new();
    for row in rows {
        let (id, name, app_type, settings_config) =
            row.map_err(|e| format!("读取行失败: {}", e))?;

        let config: Value = match serde_json::from_str(&settings_config) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let has_api_key = extract_api_key_from_config(&config).is_some();
        let (base_url, model, wire_api) = resolve_endpoint(&app_type, &config);

        providers.push(CcSwitchProviderSummary {
            id,
            name,
            app_type,
            base_url,
            model,
            wire_api,
            has_api_key,
        });
    }

    Ok(providers)
}

/// 从 settings_config JSON 中提取 apiKey。
///
/// codex provider 把 key 存在 `auth.OPENAI_API_KEY`；
/// claude provider 把 key 存在 `settings_config.env.ANTHROPIC_AUTH_TOKEN`
/// （cc-switch claude 走 AUTH_TOKEN；保险起见也尝试 env.ANTHROPIC_API_KEY 和 auth.ANTHROPIC_API_KEY）。
fn extract_api_key_from_config(config: &Value) -> Option<String> {
    let auth = config.get("auth");
    let env = config.get("env");
    auth.and_then(|a| a.get("OPENAI_API_KEY"))
        .or_else(|| env.and_then(|e| e.get("ANTHROPIC_AUTH_TOKEN")))
        .or_else(|| env.and_then(|e| e.get("ANTHROPIC_API_KEY")))
        .or_else(|| auth.and_then(|a| a.get("ANTHROPIC_API_KEY")))
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// 按 app_type 从 settings_config 解出 (base_url, model, wire_api)，供 list / import 复用。
///
/// - claude：base_url/model 来自 `settings_config.env.ANTHROPIC_BASE_URL` / `ANTHROPIC_MODEL`，
///   wire_api 固定 `anthropic`（model 无值给默认 claude-sonnet-4-20250514）。
/// - 其它（codex）：base_url/model/wire_api 来自 Codex 风格 TOML（parse_codex_toml）；
///   wire_api 仅当 TOML 写明 `responses` 时为 responses，否则 chat。
fn resolve_endpoint(app_type: &str, config: &Value) -> (String, String, String) {
    if app_type == "claude" {
        let env = config.get("env");
        let base_url = env
            .and_then(|e| e.get("ANTHROPIC_BASE_URL"))
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_default();
        let model = env
            .and_then(|e| e.get("ANTHROPIC_MODEL"))
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "claude-sonnet-4-20250514".into());
        (base_url, model, "anthropic".to_string())
    } else {
        let toml_text = config.get("config").and_then(|v| v.as_str()).unwrap_or("");
        let parsed = parse_codex_toml(toml_text);
        (
            parsed.base_url.unwrap_or_default(),
            parsed.model.unwrap_or_else(|| "gpt-4o-mini".into()),
            parsed
                .wire_api
                .filter(|w| w == "responses")
                .unwrap_or_else(|| "chat".to_string()),
        )
    }
}

/// 为 provider 生成确定性 profile ID。
fn cc_provider_profile_id(provider_id: &str) -> String {
    let mut hasher = DefaultHasher::new();
    provider_id.hash(&mut hasher);
    format!("lp-cc-{:016x}", hasher.finish())
}

/// 按 ID 导入单个 CC Switch provider 为 llmProfile。
pub(crate) async fn import_cc_switch_provider_by_id(
    provider_id: &str,
) -> Result<CcSwitchImportResult, String> {
    let db_path = home_dir().join(".cc-switch").join("cc-switch.db");
    let conn = rusqlite::Connection::open(&db_path)
        .map_err(|e| format!("打开 CC Switch 数据库失败: {}", e))?;

    let (id, name, app_type, settings_config): (String, String, String, String) = conn
        .query_row(
            "SELECT id, name, app_type, settings_config FROM providers WHERE id = ?1",
            rusqlite::params![provider_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => {
                format!("CC Switch 中找不到 provider: {}", provider_id)
            }
            other => format!("查询 CC Switch provider 失败: {}", other),
        })?;

    let config: Value = serde_json::from_str(&settings_config)
        .map_err(|e| format!("settings_config 解析失败: {}", e))?;

    let api_key = extract_api_key_from_config(&config).unwrap_or_default();

    let (base_url, model, wire_api_str) = resolve_endpoint(&app_type, &config);

    let profile_id = cc_provider_profile_id(&id);

    // 复用现有 upsert 逻辑创建 profile
    let mut profile = serde_json::json!({
        "id": profile_id,
        "name": name,
        "provider": "custom",
        "baseUrl": base_url,
        "model": model,
    });
    profile
        .as_object_mut()
        .unwrap()
        .insert("wireApi".into(), serde_json::Value::String(wire_api_str));

    if !api_key.is_empty() {
        let keychain_key = format!("llm.profile.{}.apiKey", profile_id);
        let _ = crate::settings::secret_set(&keychain_key, &api_key);
    }

    let path = crate::commands::config_path();
    let mut cfg: serde_json::Value = if path.exists() {
        let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        serde_json::json!({})
    };

    if cfg.get("llmProfiles").is_none() {
        cfg.as_object_mut()
            .ok_or("配置文件顶层不是 JSON 对象")?
            .insert("llmProfiles".into(), serde_json::json!([]));
    }
    let profiles = cfg
        .get_mut("llmProfiles")
        .and_then(|v| v.as_array_mut())
        .unwrap();

    // 检查是否已存在（按 ID 去重，或按 baseUrl+model 去重）
    let already_exists = profiles.iter().any(|p| {
        p.get("id").and_then(|v| v.as_str()) == Some(&profile_id)
            || (p.get("baseUrl").and_then(|v| v.as_str()) == Some(&base_url)
                && p.get("model").and_then(|v| v.as_str()) == Some(&model))
    });

    if already_exists {
        // 更新已有 profile
        if let Some(idx) = profiles
            .iter()
            .position(|p| p.get("id").and_then(|v| v.as_str()) == Some(&profile_id))
        {
            profiles[idx] = profile;
        } else if let Some(idx) = profiles.iter().position(|p| {
            p.get("baseUrl").and_then(|v| v.as_str()) == Some(&base_url)
                && p.get("model").and_then(|v| v.as_str()) == Some(&model)
        }) {
            profiles[idx] = profile;
        }
    } else {
        profiles.push(profile);
    }

    crate::commands::strip_secrets_for_save(&mut cfg)?;
    let content = serde_json::to_string_pretty(&cfg).unwrap_or_default();
    crate::util::write_atomic(&path, &content).map_err(|e| e.to_string())?;

    crate::commands::hydrate_secret_placeholders(&mut cfg);
    let defaults = crate::commands::default_config();
    crate::commands::merge_defaults(&mut cfg, &defaults);

    Ok(CcSwitchImportResult {
        id,
        name,
        success: true,
        error: None,
        already_exists: Some(already_exists),
    })
}

fn home_dir() -> PathBuf {
    std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .map(PathBuf::from)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 仿真 cc-switch claude provider 的 settings_config（假 token，非真密钥）。
    fn fake_claude_config() -> Value {
        serde_json::json!({
            "env": {
                "ANTHROPIC_AUTH_TOKEN": "sk-ant-test-xxx",
                "ANTHROPIC_BASE_URL": "https://api.anthropic.com",
                "ANTHROPIC_MODEL": "claude-x"
            }
        })
    }

    #[test]
    fn extract_api_key_reads_claude_auth_token_from_env() {
        let config = fake_claude_config();
        assert_eq!(
            extract_api_key_from_config(&config),
            Some("sk-ant-test-xxx".to_string())
        );
    }

    #[test]
    fn extract_api_key_falls_back_to_anthropic_api_key() {
        let config = serde_json::json!({
            "env": { "ANTHROPIC_API_KEY": "  sk-ant-alt  " }
        });
        // trim 后取到
        assert_eq!(
            extract_api_key_from_config(&config),
            Some("sk-ant-alt".to_string())
        );
    }

    #[test]
    fn extract_api_key_still_reads_codex_openai_key() {
        let config = serde_json::json!({
            "auth": { "OPENAI_API_KEY": "sk-openai-test" }
        });
        assert_eq!(
            extract_api_key_from_config(&config),
            Some("sk-openai-test".to_string())
        );
    }

    #[test]
    fn extract_api_key_empty_string_filtered() {
        let config = serde_json::json!({ "env": { "ANTHROPIC_AUTH_TOKEN": "   " } });
        assert_eq!(extract_api_key_from_config(&config), None);
    }

    #[test]
    fn resolve_endpoint_claude_uses_env_and_anthropic_wire() {
        let config = fake_claude_config();
        let (base_url, model, wire_api) = resolve_endpoint("claude", &config);
        assert_eq!(base_url, "https://api.anthropic.com");
        assert_eq!(model, "claude-x");
        assert_eq!(wire_api, "anthropic");
    }

    #[test]
    fn resolve_endpoint_claude_defaults_model_when_missing() {
        let config = serde_json::json!({
            "env": { "ANTHROPIC_BASE_URL": "https://relay.example.com" }
        });
        let (base_url, model, wire_api) = resolve_endpoint("claude", &config);
        assert_eq!(base_url, "https://relay.example.com");
        assert_eq!(model, "claude-sonnet-4-20250514");
        assert_eq!(wire_api, "anthropic");
    }

    #[test]
    fn resolve_endpoint_codex_uses_toml_and_chat_wire() {
        // codex provider 的 base_url/model 来自 TOML，wire_api 默认 chat
        let config = serde_json::json!({
            "config": "model = \"gpt-4o\"\nmodel_provider = \"openai\"\n[model_providers.openai]\nbase_url = \"https://api.openai.com/v1\"\n"
        });
        let (base_url, model, wire_api) = resolve_endpoint("codex", &config);
        assert_eq!(base_url, "https://api.openai.com/v1");
        assert_eq!(model, "gpt-4o");
        assert_eq!(wire_api, "chat");
    }

    #[test]
    fn resolve_endpoint_codex_responses_wire_preserved() {
        let config = serde_json::json!({
            "config": "model = \"gpt-5-codex\"\nmodel_provider = \"oai\"\n[model_providers.oai]\nbase_url = \"http://host/codex\"\nwire_api = \"responses\"\n"
        });
        let (_base_url, _model, wire_api) = resolve_endpoint("codex", &config);
        assert_eq!(wire_api, "responses");
    }
}
