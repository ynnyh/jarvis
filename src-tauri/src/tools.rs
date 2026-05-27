// Tool 实现 + dispatch。
//
// 这里收纳每个原 TS tool 的 Rust 版本，对应 src/tools/*.ts。dispatch 入口在
// commands::tool_execute，按 name 字段路由到本文件的具体函数。
//
// 已迁工具：
//   get_tasks         → zentao.get_my_tasks
//   get_today_tasks   → zentao.get_my_tasks + 今日截止过滤
//   get_task_detail   → zentao.get_task
//   get_task_commits  → commit_link::link_tasks_with_commits
//   get_daily_review  → daily_review::build_daily_review (+ 可选 LLM 改写)
//   analyze_risk      → zentao.get_my_tasks + heuristic (+ 可选 LLM 改写)
//   log-task-effort   → zentao.add_effort（带审计日志）
//   ask-llm           → llm.chat（直接转发）
//   cc_switch_import  → 读 ~/.cc-switch/{settings.json,cc-switch.db}
//   chat_send         → chat_agent::run_agent
//
// dispatch() 作为统一入口，供 commands::tool_execute 和 chat_agent loop 调用。

#![allow(dead_code)]

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::commit_link::{self, LinkCommitsOptions, TaskInput};
use crate::daily_review::{self, BuildOptions, ReviewTaskInfo};
use crate::git_scan::RangePreset;
use crate::llm::{self, ChatMessage, ChatRequest, Role};
use crate::zentao::ZentaoClient;

// ============================================================================
// dispatch：所有 tool 的统一入口
// ============================================================================

pub async fn dispatch(name: &str, input: Value) -> Result<Value, String> {
    match name {
        "get_tasks" => get_tasks(input).await,
        "get_today_tasks" => get_today_tasks(input).await,
        "get_task_detail" => get_task_detail(input).await,
        "get_task_commits" => get_task_commits(input).await,
        "get_daily_review" => get_daily_review(input).await,
        "get_classified_tasks" => get_classified_tasks(input).await,
        "analyze_risk" => analyze_risk(input).await,
        "log-task-effort" => log_task_effort(input).await,
        "ask-llm" => ask_llm(input).await,
        "cc_switch_import" => cc_switch_import(input).await,
        // chat_send → run_agent → dispatch（递归调其它 tool）。Box::pin 打破
        // async fn 静态递归类型，否则 rustc 报"recursive async fn requires indirection"。
        // agent 自身不会再叫 chat_send（不在 DEFAULT_AGENT_TOOLS），这层 Pin 只为通过编译。
        "chat_send" => Box::pin(chat_send(input)).await,
        _ => Err(format!("未知工具: {}", name)),
    }
}

// ============================================================================
// get_tasks
// ============================================================================

pub async fn get_tasks(_input: Value) -> Result<Value, String> {
    let client = ZentaoClient::from_settings()?;
    let tasks = client.get_my_tasks().await?;
    Ok(Value::Array(tasks))
}

// ============================================================================
// get_classified_tasks：拉任务并按工时分类（运维 / 日常事务 / 新增功能）
// ============================================================================

pub async fn get_classified_tasks(_input: Value) -> Result<Value, String> {
    let client = ZentaoClient::from_settings()?;
    let classified = client.get_classified_tasks().await?;
    serde_json::to_value(&classified).map_err(|e| format!("序列化分类任务失败: {}", e))
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

// ============================================================================
// get_today_tasks
// ============================================================================

pub async fn get_today_tasks(_input: Value) -> Result<Value, String> {
    let client = ZentaoClient::from_settings()?;
    let tasks = client.get_my_tasks().await?;
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let filtered: Vec<Value> = tasks
        .into_iter()
        .filter(|t| {
            let dl = t.get("deadline").and_then(|v| v.as_str()).unwrap_or("");
            dl.len() >= 10 && &dl[..10] == today
        })
        .collect();
    Ok(Value::Array(filtered))
}

// ============================================================================
// get_task_detail
// ============================================================================

#[derive(Debug, Deserialize)]
struct GetTaskDetailInput {
    id: String,
}

pub async fn get_task_detail(input: Value) -> Result<Value, String> {
    let parsed: GetTaskDetailInput = serde_json::from_value(input)
        .map_err(|e| format!("get_task_detail 入参错误: {}", e))?;
    if parsed.id.is_empty() {
        return Err("id 不能为空".into());
    }
    let client = ZentaoClient::from_settings()?;
    let task = client.get_task(&parsed.id).await?;
    Ok(task.unwrap_or(Value::Null))
}

// ============================================================================
// get_task_commits
// ============================================================================

#[derive(Debug, Deserialize)]
struct GetTaskCommitsInput {
    #[serde(default)]
    range: Option<String>,
    #[serde(default)]
    since: Option<String>,
    #[serde(default)]
    until: Option<String>,
    #[serde(default, rename = "rootDir")]
    root_dir: Option<Value>,
    #[serde(default, rename = "includeBody")]
    include_body: Option<bool>,
    #[serde(default, rename = "taskIds")]
    task_ids: Option<Vec<Value>>,
    #[serde(default, rename = "useLlm")]
    use_llm: Option<bool>,
}

fn coerce_root_dir(v: Option<Value>) -> Vec<String> {
    match v {
        Some(Value::String(s)) if !s.trim().is_empty() => vec![s.trim().to_string()],
        Some(Value::Array(arr)) => arr
            .into_iter()
            .filter_map(|x| x.as_str().map(|s| s.trim().to_string()))
            .filter(|s| !s.is_empty())
            .collect(),
        _ => Vec::new(),
    }
}

pub async fn get_task_commits(input: Value) -> Result<Value, String> {
    let parsed: GetTaskCommitsInput = serde_json::from_value(input)
        .map_err(|e| format!("get_task_commits 入参错误: {}", e))?;

    // 1. 拉禅道任务（自己的全部）
    let client = ZentaoClient::from_settings()?;
    let all_tasks = client.get_my_tasks().await?;

    // 2. 按 taskIds 过滤（如果给了）
    let filtered: Vec<Value> = if let Some(ids) = &parsed.task_ids {
        let id_strs: Vec<String> = ids.iter().map(value_to_id_string).collect();
        all_tasks
            .into_iter()
            .filter(|t| {
                let tid = t.get("id").map(value_to_id_string).unwrap_or_default();
                id_strs.iter().any(|x| x == &tid)
            })
            .collect()
    } else {
        all_tasks
    };

    let task_inputs: Vec<TaskInput> = filtered
        .iter()
        .map(|t| TaskInput {
            id: t.get("id").map(value_to_id_string).unwrap_or_default(),
            name: t
                .get("name")
                .or(t.get("title"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
        })
        .collect();

    // 3. 解析 rootDir：入参优先，否则 settings.repoRoots
    let root_dirs = {
        let from_input = coerce_root_dir(parsed.root_dir.clone());
        if !from_input.is_empty() {
            from_input
        } else {
            crate::git_scan::get_repo_roots()
        }
    };
    if root_dirs.is_empty() {
        return Err("缺少 rootDir。请在设置里配置代码根目录或传 rootDir 参数".into());
    }

    let range = RangePreset::parse(parsed.range.as_deref().unwrap_or("today"));
    let options = LinkCommitsOptions {
        range,
        since: parsed.since.as_deref(),
        until: parsed.until.as_deref(),
        root_dirs: &root_dirs,
        include_body: parsed.include_body.unwrap_or(true),
        // v0.6.0 起默认 true：新流程的多候选场景只有走 LLM 才能正确归属，
        // 关掉就只剩单候选直归 + exact 显式 ID，对多任务并行的 repo 形同没用。
        // 没配 LLM 也安全 —— 分类失败时这些 commit 留作孤儿，不会乱关联。
        use_llm: parsed.use_llm.unwrap_or(true),
        min_confidence: 0.4,
    };
    let result = commit_link::link_tasks_with_commits(&task_inputs, options).await?;
    serde_json::to_value(result).map_err(|e| format!("commit_link 结果序列化失败: {}", e))
}

fn value_to_id_string(v: &Value) -> String {
    if let Some(s) = v.as_str() {
        return s.to_string();
    }
    if let Some(n) = v.as_i64() {
        return n.to_string();
    }
    if let Some(f) = v.as_f64() {
        if f.fract() == 0.0 {
            return (f as i64).to_string();
        }
        return f.to_string();
    }
    v.to_string().trim_matches('"').to_string()
}

// ============================================================================
// analyze_risk
// ============================================================================

#[derive(Debug, Deserialize)]
struct AnalyzeRiskInput {
    #[serde(default, rename = "useLlm")]
    use_llm: Option<bool>,
}

pub async fn analyze_risk(input: Value) -> Result<Value, String> {
    let parsed: AnalyzeRiskInput = serde_json::from_value(input).unwrap_or(AnalyzeRiskInput { use_llm: None });
    let client = ZentaoClient::from_settings()?;
    let tasks = client.get_my_tasks().await?;

    let now = chrono::Local::now();
    let three_days_later = now + chrono::Duration::days(3);

    let parse_deadline = |t: &Value| -> Option<chrono::DateTime<chrono::Local>> {
        let dl = t.get("deadline").and_then(|v| v.as_str())?;
        if dl.len() < 10 {
            return None;
        }
        let d = chrono::NaiveDate::parse_from_str(&dl[..10], "%Y-%m-%d").ok()?;
        chrono::Local
            .from_local_datetime(&d.and_hms_opt(0, 0, 0).unwrap_or_default())
            .single()
    };
    let status_active = |t: &Value| {
        let s = t.get("status").and_then(|v| v.as_str()).unwrap_or("");
        s != "done" && s != "closed"
    };

    let overdue: Vec<Value> = tasks
        .iter()
        .filter(|t| {
            if !status_active(t) {
                return false;
            }
            match parse_deadline(t) {
                Some(d) => d < now,
                None => false,
            }
        })
        .cloned()
        .collect();
    let near_deadline: Vec<Value> = tasks
        .iter()
        .filter(|t| {
            if !status_active(t) {
                return false;
            }
            match parse_deadline(t) {
                Some(d) => d >= now && d <= three_days_later,
                None => false,
            }
        })
        .cloned()
        .collect();
    let high_priority: Vec<Value> = tasks
        .iter()
        .filter(|t| {
            if !status_active(t) {
                return false;
            }
            let p = t.get("priority").and_then(|v| v.as_str()).unwrap_or("");
            // 禅道 priority 在 OpenAPI 里是字符串，1=urgent，看用户已有的实现按字面值匹配
            p == "urgent" || p == "high" || p == "1"
        })
        .cloned()
        .collect();

    // dependency 字段在当前 zentao 实现里没拉出来，这里走空兜底
    let dependency_risks: Vec<Value> = Vec::new();

    let heuristic_summary = build_risk_summary(
        overdue.len(),
        near_deadline.len(),
        high_priority.len(),
        dependency_risks.len(),
    );

    let mut overdue_combined = overdue.clone();
    overdue_combined.extend(near_deadline.clone());
    let base = json!({
        "overdueTasks": overdue_combined,
        "highPriorityTasks": high_priority,
        "dependencyRisks": dependency_risks,
        "summary": heuristic_summary,
    });

    if !parsed.use_llm.unwrap_or(false) {
        return Ok(base);
    }
    match summarize_risk_with_llm(&overdue, &near_deadline, &high_priority).await {
        Ok(llm_summary) => {
            let mut out = base;
            if let Value::Object(map) = &mut out {
                map.insert("summary".into(), Value::String(llm_summary));
                map.insert("summaryHeuristic".into(), Value::String(heuristic_summary));
                map.insert("llmUsed".into(), Value::Bool(true));
            }
            Ok(out)
        }
        Err(e) => {
            let mut out = base;
            if let Value::Object(map) = &mut out {
                map.insert("llmUsed".into(), Value::Bool(false));
                map.insert("llmError".into(), Value::String(e));
            }
            Ok(out)
        }
    }
}

fn build_risk_summary(overdue: usize, near: usize, high: usize, dep: usize) -> String {
    let mut lines: Vec<String> = Vec::new();
    if overdue > 0 {
        lines.push(format!("发现 {} 个已延期任务，需要立即处理。", overdue));
    }
    if near > 0 {
        lines.push(format!("发现 {} 个即将到期任务（3天内），请密切关注。", near));
    }
    if high > 0 {
        lines.push(format!("有 {} 个高优先级任务待处理。", high));
    }
    if dep > 0 {
        lines.push(format!("发现 {} 个任务存在依赖风险。", dep));
    }
    if lines.is_empty() {
        lines.push("当前任务状态良好，未发现明显风险。".into());
    }
    lines.join("\n")
}

async fn summarize_risk_with_llm(
    overdue: &[Value],
    near: &[Value],
    high: &[Value],
) -> Result<String, String> {
    let brief = |t: &Value| {
        json!({
            "id": t.get("id"),
            "title": t.get("title").or(t.get("name")),
            "status": t.get("status"),
            "priority": t.get("priority"),
            "deadline": t.get("deadline"),
        })
    };
    let payload = json!({
        "overdue": overdue.iter().map(brief).collect::<Vec<_>>(),
        "nearDeadline": near.iter().map(brief).collect::<Vec<_>>(),
        "highPriority": high.iter().map(brief).collect::<Vec<_>>(),
        "today": chrono::Local::now().format("%Y-%m-%d").to_string(),
    });
    let messages = vec![
        ChatMessage {
            role: Role::System,
            content: "你是一个简短直接的任务风险提示助手。基于结构化的风险数据，告诉用户今天应该优先关注什么。\n\
约束：\n\
1. 不堆砌\"发现 N 个...\"这种计数语，要给出具体建议（哪些任务先做、为什么）\n\
2. 只能基于输入数据，不要编没有的任务名或事项\n\
3. 中文，纯文本，3~6 句话以内\n\
4. 如果数据里没有风险，直接说\"今天没有明显风险\""
                .into(),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        },
        ChatMessage {
            role: Role::User,
            content: format!("```json\n{}\n```", serde_json::to_string_pretty(&payload).unwrap_or_default()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        },
    ];
    let mut req = ChatRequest::new(messages);
    req.temperature = Some(0.3);
    req.max_tokens = Some(800);
    let resp = llm::chat(req).await?;
    Ok(resp.text.trim().to_string())
}

// chrono::TimeZone trait（analyze_risk 用到）
use chrono::TimeZone;

// ============================================================================
// get_daily_review
// ============================================================================

#[derive(Debug, Deserialize)]
struct GetDailyReviewInput {
    #[serde(default)]
    range: Option<String>,
    #[serde(default)]
    since: Option<String>,
    #[serde(default)]
    until: Option<String>,
    #[serde(default)]
    date: Option<String>,
    #[serde(default, rename = "hoursPerWorkDay")]
    hours_per_work_day: Option<f64>,
    #[serde(default, rename = "useLlm")]
    use_llm: Option<bool>,
}

pub async fn get_daily_review(input: Value) -> Result<Value, String> {
    let parsed: GetDailyReviewInput = serde_json::from_value(input)
        .map_err(|e| format!("get_daily_review 入参错误: {}", e))?;

    // 1. 拉禅道任务
    let client = ZentaoClient::from_settings()?;
    let all_tasks = client.get_my_tasks().await?;

    let task_inputs: Vec<TaskInput> = all_tasks
        .iter()
        .map(|t| TaskInput {
            id: t.get("id").map(value_to_id_string).unwrap_or_default(),
            name: t
                .get("name")
                .or(t.get("title"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
        })
        .collect();
    let review_tasks: Vec<ReviewTaskInfo> = all_tasks
        .iter()
        .map(|t| ReviewTaskInfo {
            id: t.get("id").map(value_to_id_string).unwrap_or_default(),
            name: t
                .get("name")
                .or(t.get("title"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            status: t.get("status").and_then(|v| v.as_str()).unwrap_or("wait").to_string(),
        })
        .collect();

    // 2. 拉 commit + 关联
    let root_dirs = crate::git_scan::get_repo_roots();
    if root_dirs.is_empty() {
        return Err("缺少 rootDir。请在设置里配置代码根目录".into());
    }
    let range = RangePreset::parse(parsed.range.as_deref().unwrap_or("today"));
    let link_result = commit_link::link_tasks_with_commits(
        &task_inputs,
        LinkCommitsOptions {
            range,
            since: parsed.since.as_deref(),
            until: parsed.until.as_deref(),
            root_dirs: &root_dirs,
            include_body: true,
            // 同 get_task_commits：v0.6.0 起 LLM 默认开，确保多候选 commit 能归属
            use_llm: parsed.use_llm.unwrap_or(true),
            min_confidence: 0.4,
        },
    )
    .await?;

    // 3. 合成日报
    let review = daily_review::build_daily_review(
        link_result,
        &review_tasks,
        BuildOptions {
            date: parsed.date.as_deref(),
            hours_per_work_day: parsed.hours_per_work_day.unwrap_or(0.0),
        },
    );

    // 4. 可选 LLM 改写 plainText
    if parsed.use_llm.unwrap_or(false) {
        match rewrite_daily_with_llm(&review).await {
            Ok(text) => {
                let mut v = serde_json::to_value(&review).map_err(|e| e.to_string())?;
                if let Value::Object(map) = &mut v {
                    map.insert("plainTextHeuristic".into(), Value::String(review.plain_text.clone()));
                    map.insert("plainText".into(), Value::String(text));
                    map.insert("llmUsed".into(), Value::Bool(true));
                }
                return Ok(v);
            }
            Err(e) => {
                let mut v = serde_json::to_value(&review).map_err(|e| e.to_string())?;
                if let Value::Object(map) = &mut v {
                    map.insert("llmUsed".into(), Value::Bool(false));
                    map.insert("llmError".into(), Value::String(e));
                }
                return Ok(v);
            }
        }
    }
    serde_json::to_value(&review).map_err(|e| format!("daily review 序列化失败: {}", e))
}

async fn rewrite_daily_with_llm(review: &daily_review::DailyReview) -> Result<String, String> {
    let summary_payload = json!({
        "date": review.date,
        "totalCommits": review.summary.total_commits,
        "advancedTasks": review.advanced_tasks.iter().map(|t| json!({
            "id": t.task_id,
            "name": t.task_name,
            "status": t.status,
            "businessLine": t.business_line,
            "commitCount": t.commit_count,
            "suggestedHours": t.suggested_hours,
        })).collect::<Vec<_>>(),
        "byBusinessLine": review.by_business_line.iter().map(|g| {
            let titles: std::collections::HashSet<String> = g.commits.iter().map(|c| c.title.clone()).collect();
            let titles_vec: Vec<String> = titles.into_iter().take(20).collect();
            json!({
                "businessLine": g.business_line,
                "commitCount": g.commits.len(),
                "taskCount": g.tasks.len(),
                "suggestedHours": g.suggested_hours,
                "commitTitles": titles_vec,
            })
        }).collect::<Vec<_>>(),
        "needsStatusUpdate": review.needs_status_update.iter().map(|n| json!({
            "id": n.task_id, "name": n.task_name, "commitCount": n.commit_count,
        })).collect::<Vec<_>>(),
        "orphanCommitCount": review.summary.orphan_commit_count,
        "totalHoursForEstimate": review.total_hours_for_estimate,
    });
    let messages = vec![
        ChatMessage {
            role: Role::System,
            content: "你是一个简洁的日报助手。基于结构化的当日工作数据生成自然语言日报。\n\
强约束：\n\
1. 完全去技术化——不出现 commit/sha/repo/PR/branch 等词，commit 标题原样使用（不要加技术修饰）\n\
2. 不要凭空发挥，所有内容必须能从输入数据里找到依据\n\
3. 用项目维度（业务线）+ 任务推进 + 需要补登/更新状态的事项 组织文章\n\
4. 段落清晰，避免大段流水账。提供工时建议时按业务线总览，不必每个任务展开\n\
5. 输出纯文本，不要 Markdown 符号（#、*、- 都不用）"
                .into(),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        },
        ChatMessage {
            role: Role::User,
            content: format!(
                "请基于以下结构化数据，写一份 {} 的工作日报（中文，纯文本）：\n\n```json\n{}\n```",
                review.date,
                serde_json::to_string_pretty(&summary_payload).unwrap_or_default()
            ),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        },
    ];
    let mut req = ChatRequest::new(messages);
    req.temperature = Some(0.4);
    req.max_tokens = Some(1500);
    let resp = llm::chat(req).await?;
    Ok(resp.text.trim().to_string())
}

// ============================================================================
// chat_send
// ============================================================================

#[derive(Debug, Deserialize)]
struct ChatSendInput {
    messages: Vec<ChatSendMessage>,
    #[serde(default, rename = "assistantName")]
    assistant_name: Option<String>,
    #[serde(default, rename = "userTitle")]
    user_title: Option<String>,
    #[serde(default, rename = "maxIterations")]
    max_iterations: Option<u32>,
    #[serde(default)]
    temperature: Option<f32>,
    #[serde(default, rename = "allowedTools")]
    allowed_tools: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct ChatSendMessage {
    role: String,
    content: String,
    #[serde(default)]
    tool_calls: Option<Vec<Value>>,
    #[serde(default)]
    tool_call_id: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

pub async fn chat_send(input: Value) -> Result<Value, String> {
    let parsed: ChatSendInput =
        serde_json::from_value(input).map_err(|e| format!("chat_send 入参错误: {}", e))?;
    if parsed.messages.is_empty() {
        return Err("messages 不能为空".into());
    }

    let allowed_tools: Vec<String> = parsed
        .allowed_tools
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| crate::chat_agent::DEFAULT_AGENT_TOOLS.iter().map(|s| s.to_string()).collect());

    let has_system = parsed.messages.first().map(|m| m.role == "system").unwrap_or(false);
    let system_prompt = if has_system {
        None
    } else {
        Some(crate::chat_agent::default_system_prompt(
            parsed.assistant_name.as_deref().unwrap_or("Jarvis"),
            parsed.user_title.as_deref().unwrap_or("主人"),
        ))
    };

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
            tool_calls: m.tool_calls.and_then(|tc| serde_json::from_value(Value::Array(tc)).ok()),
            tool_call_id: m.tool_call_id,
            name: m.name,
        })
        .collect();

    let result = crate::chat_agent::run_agent(crate::chat_agent::RunAgentOptions {
        messages,
        allowed_tools: &allowed_tools,
        max_iterations: parsed.max_iterations.unwrap_or(8),
        temperature: parsed.temperature.unwrap_or(0.3),
        max_tokens: 2048,
        system_prompt,
    })
    .await;

    Ok(json!({
        "newMessages": result.new_messages,
        "steps": result.steps,
        "tokensIn": result.tokens_in,
        "tokensOut": result.tokens_out,
        "truncated": result.truncated,
        "allowedTools": allowed_tools,
    }))
}
