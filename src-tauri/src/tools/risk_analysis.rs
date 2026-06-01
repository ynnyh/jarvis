use serde::Deserialize;
use serde_json::{json, Value};

use crate::llm::{self, ChatMessage, ChatRequest, Role};
use crate::zentao::ZentaoClient;

use chrono::TimeZone;

// ============================================================================
// analyze_risk
// ============================================================================

#[derive(Debug, Deserialize)]
struct AnalyzeRiskInput {
    #[serde(default, rename = "useLlm")]
    use_llm: Option<bool>,
}

pub(crate) async fn analyze_risk(input: Value) -> Result<Value, String> {
    let parsed: AnalyzeRiskInput =
        serde_json::from_value(input).unwrap_or(AnalyzeRiskInput { use_llm: None });
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
        lines.push(format!(
            "发现 {} 个即将到期任务（3天内），请密切关注。",
            near
        ));
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
