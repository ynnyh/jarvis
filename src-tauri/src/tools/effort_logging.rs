use serde::Deserialize;
use serde_json::{json, Value};

use crate::zentao::ZentaoClient;

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
    #[serde(default, rename = "clientRequestId")]
    client_request_id: Option<String>,
    #[serde(default, rename = "taskName")]
    task_name: Option<String>,
}

/// 扫审计日志找同 clientRequestId 的成功写入，返回其 effortId（幂等去重用）。
/// 取最后一条匹配（最近一次成功）。文件不存在 / 无匹配返回 None。
fn find_prior_successful_effort(client_request_id: &str) -> Option<String> {
    let path = crate::settings::jarvis_dir().join("write-back.log");
    let raw = std::fs::read_to_string(path).ok()?;
    let mut found: Option<String> = None;
    for line in raw.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if v.get("ok").and_then(|x| x.as_bool()) != Some(true) {
            continue;
        }
        if v.get("clientRequestId").and_then(|x| x.as_str()) == Some(client_request_id) {
            found = v
                .get("effortId")
                .map(|x| x.to_string().trim_matches('"').to_string());
        }
    }
    found
}

pub(crate) async fn log_task_effort(input: Value) -> Result<Value, String> {
    let parsed: LogEffortInput =
        serde_json::from_value(input).map_err(|e| format!("log-task-effort 入参错误: {}", e))?;
    if parsed.task_id.is_empty() {
        return Err("taskId 不能为空".into());
    }
    if parsed.hours <= 0.0 {
        return Err("hours 必须为正数".into());
    }
    if parsed.work.is_empty() {
        return Err("work 不能为空".into());
    }

    // 幂等去重：clientRequestId 命中历史成功写入，直接返回原 effortId，不再写禅道。
    // 防 session 丢失 / 重复点击 / 重试导致同一张卡在禅道写出多条工时（不可逆）。
    if let Some(req_id) = parsed.client_request_id.as_deref().filter(|s| !s.is_empty()) {
        if let Some(effort_id) = find_prior_successful_effort(req_id) {
            return Ok(json!({
                "ok": true,
                "effortId": effort_id,
                "deduped": true,
            }));
        }
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
            "clientRequestId": parsed.client_request_id,
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
            "clientRequestId": parsed.client_request_id,
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

pub(crate) async fn prepare_log_task_effort(input: Value) -> Result<Value, String> {
    let parsed: LogEffortInput = serde_json::from_value(input)
        .map_err(|e| format!("prepare-log-task-effort 入参错误: {}", e))?;
    if parsed.task_id.trim().is_empty() {
        return Err("taskId 不能为空".into());
    }
    if parsed.hours <= 0.0 {
        return Err("hours 必须为正数".into());
    }
    if parsed.work.trim().is_empty() {
        return Err("work 不能为空".into());
    }
    let date = parsed
        .date
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| chrono::Local::now().format("%Y-%m-%d").to_string());
    Ok(json!({
        "pendingWrite": true,
        "kind": "log-task-effort",
        "payload": {
            "taskId": parsed.task_id.trim(),
            "hours": parsed.hours,
            "work": parsed.work.trim(),
            "date": date,
        },
        "summary": format!(
            "任务: #{}{}\n工时: {}h\n日期: {}\n内容: {}",
            parsed.task_id.trim(),
            parsed.hours,
                parsed.task_name.as_ref().map(|n| format!(" {}", n)).unwrap_or_default(),
            date,
            parsed.work.trim()
        ),
        "message": "已准备写入建议，请用户确认后再执行。"
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
