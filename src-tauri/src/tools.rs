// Tool 实现 + dispatch。
//
// 这里收纳每个原 TS tool 的 Rust 版本，对应 src/tools/*.ts。dispatch 入口在
// commands::tool_execute，按 name 字段路由到本文件的具体函数；本会话先迁了简单
// wrapper：
//
//   get_tasks         → zentao.get_my_tasks（直接转发）
//   log-task-effort   → zentao.add_effort（带审计日志）
//   ask-llm           → llm.chat（直接转发）
//   cc_switch_import  → 读 ~/.cc-switch/{settings.json,cc-switch.db} + TOML 正则
//
// 下一会话要补：get_task_commits / get_daily_review / chat_send（这三个依赖
// git scan + memory + context-builder + LLM agent loop，量大）。dispatch 用
// daemon_client 做 fallback 让未迁的 tool 仍能跑。

#![allow(dead_code)]

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::llm::{self, ChatMessage, ChatRequest, Role};
use crate::zentao::ZentaoClient;

// ============================================================================
// get_tasks
// ============================================================================

pub async fn get_tasks(_input: Value) -> Result<Value, String> {
    let client = ZentaoClient::from_settings()?;
    let tasks = client.get_my_tasks().await?;
    Ok(Value::Array(tasks))
}

// ============================================================================
// log-task-effort
// ============================================================================

#[derive(Debug, Deserialize)]
struct LogEffortInput {
    #[serde(rename = "taskId")]
    task_id: String,
    hours: f64,
    work: String,
    date: Option<String>,
}

pub async fn log_task_effort(input: Value) -> Result<Value, String> {
    let parsed: LogEffortInput = serde_json::from_value(input)
        .map_err(|e| format!("log-task-effort 入参错误: {}", e))?;
    if parsed.task_id.is_empty() {
        return Err("taskId 不能为空".into());
    }
    if parsed.hours <= 0.0 {
        return Err("hours 必须为正数".into());
    }
    if parsed.work.is_empty() {
        return Err("work 不能为空".into());
    }

    let client = ZentaoClient::from_settings()?;
    let account = crate::settings::get_zentao_credentials().account;

    let date_ref = parsed.date.as_deref();
    let result = client
        .add_effort(&parsed.task_id, parsed.hours, &parsed.work, date_ref)
        .await;

    // 审计日志（JSONL）—— 成功失败都记，便于事后回溯
    let audit_entry = match &result {
        Ok(r) => json!({
            "action": "log-task-effort",
            "ok": true,
            "taskId": parsed.task_id,
            "hours": parsed.hours,
            "work": parsed.work,
            "date": parsed.date,
            "account": account,
            "effortId": r.id,
            "endpoint": r.endpoint,
            "preservedLeft": r.preserved_left,
            "consumedBefore": r.consumed_before,
            "consumedAfter": r.consumed_after,
            "responseText": r.response_text,
        }),
        Err(e) => json!({
            "action": "log-task-effort",
            "ok": false,
            "taskId": parsed.task_id,
            "hours": parsed.hours,
            "work": parsed.work,
            "date": parsed.date,
            "account": account,
            "error": e,
        }),
    };
    append_audit_log(audit_entry);

    let r = result?;
    Ok(json!({
        "ok": true,
        "effortId": r.id,
        "endpoint": r.endpoint,
        "preservedLeft": r.preserved_left,
        "consumedBefore": r.consumed_before,
        "consumedAfter": r.consumed_after,
    }))
}

fn append_audit_log(entry: Value) {
    let path = crate::settings::jarvis_dir().join("write-back.log");
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let merged = {
        let mut m = serde_json::Map::new();
        m.insert("ts".into(), Value::String(chrono::Utc::now().to_rfc3339()));
        if let Value::Object(obj) = entry {
            for (k, v) in obj {
                m.insert(k, v);
            }
        }
        Value::Object(m)
    };
    let line = format!("{}\n", merged);
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut f| {
            use std::io::Write;
            f.write_all(line.as_bytes())
        });
}

// ============================================================================
// ask-llm
// ============================================================================

#[derive(Debug, Deserialize)]
struct AskLlmInput {
    messages: Vec<AskLlmMessage>,
    temperature: Option<f32>,
    #[serde(rename = "maxTokens")]
    max_tokens: Option<u32>,
    model: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AskLlmMessage {
    role: String,
    content: String,
}

pub async fn ask_llm(input: Value) -> Result<Value, String> {
    let parsed: AskLlmInput =
        serde_json::from_value(input).map_err(|e| format!("ask-llm 入参错误: {}", e))?;
    if parsed.messages.is_empty() {
        return Err("messages 不能为空".into());
    }

    let messages: Vec<ChatMessage> = parsed
        .messages
        .into_iter()
        .map(|m| ChatMessage {
            role: match m.role.as_str() {
                "system" => Role::System,
                "assistant" => Role::Assistant,
                "tool" => Role::Tool,
                _ => Role::User,
            },
            content: m.content,
            tool_calls: None,
            tool_call_id: None,
            name: None,
        })
        .collect();

    let mut req = ChatRequest::new(messages);
    req.temperature = parsed.temperature;
    req.max_tokens = parsed.max_tokens;
    req.model = parsed.model;

    let resp = llm::chat(req).await?;
    Ok(json!({
        "text": resp.text,
        "tokensIn": resp.tokens_in,
        "tokensOut": resp.tokens_out,
        "model": resp.model,
    }))
}

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

pub async fn cc_switch_import(_input: Value) -> Result<Value, String> {
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
            _ => {
                return Ok(serde_json::to_value(CcImportResult {
                    found: false,
                    reason: Some(
                        "CC Switch 没有选定的 Codex（OpenAI）provider，请先在 CC Switch 里切换到一个".into(),
                    ),
                    provider: None,
                })
                .unwrap())
            }
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
        let section_re = format!(
            r"\[model_providers\.{}\]([\s\S]*?)(?:\n\[|$)",
            escaped
        );
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

fn home_dir() -> PathBuf {
    std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .map(PathBuf::from)
        .unwrap_or_default()
}

// rusqlite 的 Option 风格 query_row 需要 OptionalExtension
use rusqlite::OptionalExtension;
