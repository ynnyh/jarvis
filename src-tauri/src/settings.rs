// Shared settings access for Rust modules.
//
// 单一数据源：~/.jarvis/config.json（由 Tauri config_save 写入）。
// 本模块只读，不写。
//
// 这一层对齐原 TS 端 src/config/settings.ts 的语义：
//   - 缺字段用 DEFAULTS 兜底
//   - LLM 凭证支持 env 回退（LLM_API_KEY / DEEPSEEK_API_KEY / OPENAI_API_KEY 等）
//
// daemon 整体 Rust 化后这里会成为唯一的 settings 入口。

use std::path::PathBuf;

/// 配置目录 ~/.jarvis/
pub fn jarvis_dir() -> PathBuf {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_default();
    PathBuf::from(home).join(".jarvis")
}

pub fn config_path() -> PathBuf {
    jarvis_dir().join("config.json")
}

/// 读 config.json 并 parse 成 JSON。文件不存在 / 解析失败都返回 None。
/// 调用方根据具体 key 用 `.get().and_then(...)` 链取值并自带默认值。
pub fn load_raw_config() -> Option<serde_json::Value> {
    let path = config_path();
    let content = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&content).ok()
}

// ===== LLM 凭证 =====

#[derive(Debug, Clone)]
pub struct LlmCredentials {
    #[allow(dead_code)]
    pub provider: String,
    pub base_url: String,
    pub model: String,
    pub api_key: String,
    /// 'chat'（默认）或 'responses'（Codex CLI 协议）
    pub wire_api: String,
}

/// 读 LLM 凭证。config.json 优先，缺字段从 env 兜底，再缺用 DeepSeek 默认值。
///
/// env fallback 顺序：
///   apiKey: LLM_API_KEY > DEEPSEEK_API_KEY > OPENAI_API_KEY
///   baseUrl: LLM_BASE_URL > https://api.deepseek.com
///   model:   LLM_MODEL > deepseek-chat
pub fn get_llm_credentials() -> LlmCredentials {
    let cfg = load_raw_config();
    let llm = cfg.as_ref().and_then(|v| v.get("llm"));

    let s = |key: &str| -> Option<String> {
        llm.and_then(|v| v.get(key))
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    };

    let env_first = |keys: &[&str]| -> Option<String> {
        keys.iter()
            .filter_map(|k| std::env::var(k).ok())
            .find(|v| !v.trim().is_empty())
    };

    LlmCredentials {
        provider: s("provider").unwrap_or_else(|| "deepseek".to_string()),
        base_url: s("baseUrl")
            .or_else(|| env_first(&["LLM_BASE_URL"]))
            .unwrap_or_else(|| "https://api.deepseek.com".to_string()),
        model: s("model")
            .or_else(|| env_first(&["LLM_MODEL"]))
            .unwrap_or_else(|| "deepseek-chat".to_string()),
        api_key: s("apiKey")
            .or_else(|| env_first(&["LLM_API_KEY", "DEEPSEEK_API_KEY", "OPENAI_API_KEY"]))
            .unwrap_or_default(),
        wire_api: match s("wireApi").as_deref() {
            Some("responses") => "responses".to_string(),
            _ => "chat".to_string(),
        },
    }
}

// ===== 禅道凭证 =====

#[derive(Debug, Clone)]
pub struct ZentaoCredentials {
    pub base_url: String,
    pub account: String,
    pub password: String,
    /// 应急通道：用户可在 ~/.jarvis/zentaosid.txt 放浏览器复制的 cookie 跳过登录
    pub session_cookie: Option<String>,
}

/// 读禅道凭证。
/// - baseUrl/account 从 ~/.jarvis/config.json 读，env 兜底
/// - password 从 OS 密钥链按 account 取（Service="Jarvis"）
/// - sessionCookie 优先级：config.zentao.sessionCookie > ~/.jarvis/zentaosid.txt > env
pub fn get_zentao_credentials() -> ZentaoCredentials {
    let cfg = load_raw_config();
    let zentao = cfg.as_ref().and_then(|v| v.get("zentao"));

    let s = |key: &str| -> Option<String> {
        zentao
            .and_then(|v| v.get(key))
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    };

    let env_first = |keys: &[&str]| -> Option<String> {
        keys.iter()
            .filter_map(|k| std::env::var(k).ok())
            .find(|v| !v.trim().is_empty())
    };

    let base_url = s("baseUrl")
        .or_else(|| env_first(&["ZENTAO_BASE_URL", "ZENTAO_URL"]))
        .unwrap_or_default();
    let account = s("account")
        .or_else(|| env_first(&["ZENTAO_ACCOUNT", "ZENTAO_USER"]))
        .unwrap_or_default();

    // 从 keychain 取密码
    let password = if !account.is_empty() {
        keyring::Entry::new("Jarvis", &account)
            .ok()
            .and_then(|e| e.get_password().ok())
            .unwrap_or_default()
    } else {
        String::new()
    };
    let password = if password.is_empty() {
        env_first(&["ZENTAO_PASSWORD", "ZENTAO_PASS"]).unwrap_or_default()
    } else {
        password
    };

    // session cookie 兜底通道
    let cookie_from_file = std::fs::read_to_string(jarvis_dir().join("zentaosid.txt"))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let session_cookie = s("sessionCookie")
        .or(cookie_from_file)
        .or_else(|| env_first(&["ZENTAO_SESSION_COOKIE"]));

    ZentaoCredentials {
        base_url,
        account,
        password,
        session_cookie,
    }
}
