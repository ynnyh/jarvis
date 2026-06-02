use crate::channels::types::AgentReply;
use crate::tools;
use serde_json::json;

pub(super) async fn maybe_handle_effort_query(text: &str) -> Result<Option<AgentReply>, String> {
    if !is_effort_query(text) {
        return Ok(None);
    }

    let (range, label) = effort_query_range(text);
    let tool_name = if matches!(range, "thisMonth" | "thisQuarter" | "last6Months" | "thisYear") {
        "get_effort_report"
    } else {
        "get_efforts"
    };
    let response = tools::dispatch(tool_name, json!({ "range": range })).await?;

    Ok(Some(AgentReply {
        text: if tool_name == "get_effort_report" {
            format_effort_report_reply(&label, &response)
        } else {
            format_effort_reply(&label, &response)
        },
    }))
}

pub fn is_effort_query(text: &str) -> bool {
    let lower = text.to_lowercase();
    let has_effort_word = ["工时", "耗时", "小时", "effort"]
        .iter()
        .any(|w| lower.contains(w));
    if !has_effort_word {
        return false;
    }
    if ["写", "写入", "记录", "新增", "填", "补", "提交"]
        .iter()
        .any(|w| lower.contains(w))
    {
        return false;
    }
    [
        "查", "查询", "看", "统计", "汇总", "多少", "明细", "本周", "上周", "今天", "昨天", "本月", "本季度", "季度", "半年", "近半年", "今年",
    ]
    .iter()
    .any(|w| lower.contains(w))
}

pub fn effort_query_range(text: &str) -> (&'static str, String) {
    let lower = text.to_lowercase();
    if lower.contains("昨天") || lower.contains("昨日") {
        ("yesterday", "昨天".to_string())
    } else if lower.contains("今天") || lower.contains("今日") {
        ("today", "今天".to_string())
    } else if lower.contains("上周") || lower.contains("上一周") {
        ("lastWeek", "上周".to_string())
    } else if lower.contains("本月") || lower.contains("这个月") {
        ("thisMonth", "本月".to_string())
    } else if lower.contains("本季度") || lower.contains("这个季度") || lower.contains("季度") {
        ("thisQuarter", "本季度".to_string())
    } else if lower.contains("近半年") || lower.contains("半年") || lower.contains("半年度") {
        ("last6Months", "近半年".to_string())
    } else if lower.contains("今年") || lower.contains("本年") {
        ("thisYear", "今年".to_string())
    } else {
        ("thisWeek", "本周".to_string())
    }
}

fn format_effort_reply(label: &str, response: &serde_json::Value) -> String {
    let count = response.get("count").and_then(|v| v.as_u64()).unwrap_or(0);
    let total_hours = response
        .get("totalHours")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let records = response
        .get("records")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    if count == 0 || records.is_empty() {
        return format!("{}没有查到工时记录。", label);
    }

    let begin = response.get("begin").and_then(|v| v.as_str()).unwrap_or("");
    let end = response.get("end").and_then(|v| v.as_str()).unwrap_or("");
    let mut lines = vec![format!(
        "{}工时（{} ~ {}）：共 {} 条，合计 {:.1} 小时。",
        label, begin, end, count, total_hours
    )];

    // Group records by task name for structured display
    let mut groups: std::collections::BTreeMap<String, Vec<&serde_json::Value>> =
        std::collections::BTreeMap::new();
    for record in &records {
        let name = record
            .get("taskName")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .unwrap_or("未命名任务")
            .to_string();
        groups.entry(name).or_default().push(record);
    }

    for (task_name, recs) in &groups {
        let task_total: f64 = recs
            .iter()
            .filter_map(|r| r.get("itemHours").and_then(|v| v.as_f64()))
            .sum();
        lines.push(format!("【{}】{:.1}h", task_name, task_total));

        let mut seen: Vec<&str> = Vec::new();
        for r in recs {
            let work = r
                .get("workContent")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            if !work.is_empty() && !seen.contains(&work) {
                seen.push(work);
                lines.push(format!("  - {}", work));
            }
        }
    }
    lines.push(format!("合计：{:.1} 小时", total_hours));
    lines.join("\n")
}

fn format_effort_report_reply(label: &str, response: &serde_json::Value) -> String {
    let summary = response
        .get("summaryText")
        .and_then(|v| v.as_str())
        .unwrap_or("未生成阶段汇报内容。");

    let appendix = response.get("appendix").cloned().unwrap_or_else(|| json!({}));
    let total_hours = appendix.get("total_hours").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let task_count = appendix.get("task_count").and_then(|v| v.as_u64()).unwrap_or(0);
    let project_count = appendix.get("project_count").and_then(|v| v.as_u64()).unwrap_or(0);
    let begin = response.get("begin").and_then(|v| v.as_str()).unwrap_or("");
    let end = response.get("end").and_then(|v| v.as_str()).unwrap_or("");

    let mut parts = vec![format!(
        "{}工作汇报（{} ~ {}）\n\n{}",
        label, begin, end, summary
    )];

    // Anomaly detection: workday < 8h or non-workday > 0h
    let anomalies: Vec<&serde_json::Value> = appendix
        .get("daily_hours")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter(|item| {
                    let h = item.get("hours").and_then(|v| v.as_f64()).unwrap_or(0.0);
                    let wd = item.get("isWorkday").and_then(|v| v.as_bool()).unwrap_or(true);
                    (wd && h < 8.0) || (!wd && h > 0.0)
                })
                .collect()
        })
        .unwrap_or_default();

    if !anomalies.is_empty() {
        let mut anomaly_lines = vec!["\n工时异常：".to_string()];
        for item in &anomalies {
            let date = item.get("date").and_then(|v| v.as_str()).unwrap_or("");
            let hours = item.get("hours").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let is_workday = item.get("isWorkday").and_then(|v| v.as_bool()).unwrap_or(true);
            let holiday = item
                .get("holiday")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty());

            if is_workday {
                anomaly_lines.push(format!("  - {} 只录了 {:.1}h（工作日不足 8h）", date, hours));
            } else {
                let suffix = holiday.map(|h| format!("（{}）", h)).unwrap_or_default();
                anomaly_lines.push(format!("  - {}{} 录了 {:.1}h（非工作日）", date, suffix, hours));
            }
        }
        anomaly_lines.push(format!("共 {} 天异常", anomalies.len()));
        parts.push(anomaly_lines.join("\n"));
    }

    parts.push(format!(
        "\n数据附录：总工时 {:.1}h，任务数 {}，项目数 {}。",
        total_hours, task_count, project_count
    ));

    parts.join("\n")
}
