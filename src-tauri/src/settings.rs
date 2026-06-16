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

pub const SECRET_PLACEHOLDER: &str = "********";
const SECRET_SERVICE_NAME: &str = "Jarvis-Secrets";

pub fn secret_get(account: &str) -> Option<String> {
    let entry = keyring::Entry::new(SECRET_SERVICE_NAME, account).ok()?;
    match entry.get_password() {
        Ok(s) => {
            let s = s.trim().to_string();
            if s.is_empty() {
                None
            } else {
                Some(s)
            }
        }
        // NoEntry 是正常情况（还没存过）；其它错误（密钥链被锁/服务不可用）要留痕，
        // 否则凭据静默变空，表现为"密码明明配了却登录失败"，极难排查。
        Err(keyring::Error::NoEntry) => None,
        Err(e) => {
            tracing::warn!(
                target: "settings",
                "读取密钥链 '{}' 失败（非 NoEntry，凭据按空处理）: {}",
                account, e
            );
            None
        }
    }
}

pub fn secret_set(account: &str, value: &str) -> Result<(), String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed == SECRET_PLACEHOLDER {
        return Ok(());
    }
    keyring::Entry::new(SECRET_SERVICE_NAME, account)
        .map_err(|e| format!("无法访问密钥链: {}", e))?
        .set_password(trimmed)
        .map_err(|e| format!("保存密钥失败: {}", e))
}

pub fn secret_clear(account: &str) -> Result<(), String> {
    let entry = keyring::Entry::new(SECRET_SERVICE_NAME, account)
        .map_err(|e| format!("Failed to access keychain: {}", e))?;
    match entry.delete_credential() {
        Ok(_) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("Failed to clear secret: {}", e)),
    }
}

pub fn secret_exists(account: &str) -> bool {
    secret_get(account).is_some()
}

/// 配置目录 ~/.jarvis/
pub fn jarvis_dir() -> PathBuf {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_default();
    PathBuf::from(home).join(".jarvis")
}

/// 串行化所有对 config.json 的写入（config_save / save_reminders 等），避免
/// read-modify-write 之间的 lost update：设置面板写整份 config 与机器人写 reminders
/// 并发时互相覆盖字段。写入统一走 util::write_atomic + 本锁。
pub static CONFIG_WRITE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

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

/// 读 LLM 凭证。优先从 OS 密钥链取 apiKey；旧配置里的明文值仅作为迁移兜底。
///
/// env fallback 顺序：
///   apiKey: LLM_API_KEY > DEEPSEEK_API_KEY > OPENAI_API_KEY
///   baseUrl: LLM_BASE_URL > https://api.deepseek.com
///   model:   LLM_MODEL > deepseek-chat
pub fn get_llm_credentials() -> LlmCredentials {
    let cfg = load_raw_config();
    let llm = cfg.as_ref().and_then(|v| v.get("llm"));
    let mut active_profile_id = String::new();
    let mut active_profile: Option<&serde_json::Value> = None;

    if let Some(root) = cfg.as_ref() {
        if let Some(id) = root
            .get("activeLlmProfileId")
            .and_then(|v| v.as_str())
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
        {
            active_profile_id = id.to_string();
            active_profile = root
                .get("llmProfiles")
                .and_then(|v| v.as_array())
                .and_then(|arr| {
                    arr.iter()
                        .find(|p| p.get("id").and_then(|v| v.as_str()) == Some(id))
                });
        }
    }

    let llm_s = |key: &str| -> Option<String> {
        llm.and_then(|v| v.get(key))
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    };
    let profile_s = |key: &str| -> Option<String> {
        active_profile
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

    let profile_keychain = if active_profile_id.is_empty() {
        None
    } else {
        secret_get(&format!("llm.profile.{}.apiKey", active_profile_id))
    };

    LlmCredentials {
        provider: profile_s("provider")
            .or_else(|| llm_s("provider"))
            .unwrap_or_else(|| "deepseek".to_string()),
        base_url: profile_s("baseUrl")
            .or_else(|| llm_s("baseUrl"))
            .or_else(|| env_first(&["LLM_BASE_URL"]))
            .unwrap_or_else(|| "https://api.deepseek.com".to_string()),
        model: profile_s("model")
            .or_else(|| llm_s("model"))
            .or_else(|| env_first(&["LLM_MODEL"]))
            .unwrap_or_else(|| "deepseek-chat".to_string()),
        api_key: profile_keychain
            .or_else(|| profile_s("apiKey").filter(|v| v != SECRET_PLACEHOLDER))
            .or_else(|| secret_get("llm.apiKey"))
            .or_else(|| llm_s("apiKey").filter(|v| v != SECRET_PLACEHOLDER))
            .or_else(|| env_first(&["LLM_API_KEY", "DEEPSEEK_API_KEY", "OPENAI_API_KEY"]))
            .unwrap_or_default(),
        wire_api: match profile_s("wireApi").or_else(|| llm_s("wireApi")).as_deref() {
            Some("responses") => "responses".to_string(),
            Some("anthropic") => "anthropic".to_string(),
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

    let session_cookie = secret_get("zentao.sessionCookie")
        .or_else(|| s("sessionCookie").filter(|v| v != SECRET_PLACEHOLDER))
        .or(cookie_from_file)
        .or_else(|| env_first(&["ZENTAO_SESSION_COOKIE"]));

    ZentaoCredentials {
        base_url,
        account,
        password,
        session_cookie,
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    // ===== secret_set 短路逻辑 =====
    // secret_set 对空值/placeholder 直接返回 Ok,不碰密钥链。
    // 这是不依赖 IO 的纯逻辑分支,可以安全单测。

    #[test]
    fn secret_set_empty_value_is_noop() {
        // 空字符串 → trim 后为空 → 直接 Ok,不碰密钥链
        assert!(secret_set("test-account-empty", "").is_ok());
    }

    #[test]
    fn secret_set_whitespace_only_is_noop() {
        // 只有空白 → trim 后为空 → 直接 Ok
        assert!(secret_set("test-account-ws", "   \t\n  ").is_ok());
    }

    #[test]
    fn secret_set_placeholder_is_noop() {
        // placeholder("********")→ 直接 Ok,避免 UI 回显值覆盖真实密钥
        assert!(secret_set("test-account-ph", SECRET_PLACEHOLDER).is_ok());
    }

    // ===== jarvis_dir 路径拼接 =====
    // jarvis_dir 读 USERPROFILE(Windows)或 HOME(*nix)。
    // 注意:std::env::set_var 非线程安全,测试并行时有风险。
    // 这里测试只 set/restore 本进程的环境变量,操作极快,
    // 且用不存在的 account 测 secret_set 短路(不碰真实密钥链)。
    // jarvis_dir 测试用独立的 account 名隔离。

    #[test]
    fn jarvis_dir_joins_home_dot_jarvis() {
        // 备份原值
        let orig_profile = std::env::var("USERPROFILE").ok();
        let orig_home = std::env::var("HOME").ok();

        // Windows 优先 USERPROFILE
        std::env::set_var("USERPROFILE", r"C:\fake\home");
        std::env::remove_var("HOME");
        let dir = jarvis_dir();
        // join() 会使用当前操作系统的路径分隔符，因此用 PathBuf 构造期望值
        let expected = std::path::PathBuf::from(r"C:\fake\home").join(".jarvis");
        assert_eq!(dir, expected);

        // 恢复
        match orig_profile {
            Some(v) => std::env::set_var("USERPROFILE", v),
            None => std::env::remove_var("USERPROFILE"),
        }
        match orig_home {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
    }

    #[test]
    fn jarvis_dir_falls_back_to_home_when_no_userprofile() {
        let orig_profile = std::env::var("USERPROFILE").ok();
        let orig_home = std::env::var("HOME").ok();

        std::env::remove_var("USERPROFILE");
        std::env::set_var("HOME", "/fake/unix-home");
        let dir = jarvis_dir();
        // join() 会使用当前操作系统的路径分隔符
        let expected = std::path::PathBuf::from("/fake/unix-home").join(".jarvis");
        assert_eq!(dir, expected);

        match orig_profile {
            Some(v) => std::env::set_var("USERPROFILE", v),
            None => std::env::remove_var("USERPROFILE"),
        }
        match orig_home {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
    }

    #[test]
    fn config_path_appends_config_json() {
        let dir = jarvis_dir();
        let cfg = config_path();
        assert_eq!(cfg, dir.join("config.json"));
        assert!(cfg.to_string_lossy().ends_with("config.json"));
    }
}

