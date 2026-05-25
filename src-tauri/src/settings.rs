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
