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
    for record in records.iter().take(8) {
        let date = record.get("date").and_then(|v| v.as_str()).unwrap_or("");
        let hours = record
            .get("itemHours")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let task = record
            .get("taskName")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .unwrap_or("未命名任务");
        let work = record
            .get("workContent")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        if work.is_empty() {
            lines.push(format!("- {} {:.1}h {}", date, hours, task));
        } else {
            lines.push(format!("- {} {:.1}h {}：{}", date, hours, task, work));
        }
    }
    if records.len() > 8 {
        lines.push(format!("还有 {} 条明细没有展开。", records.len() - 8));
    }
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

    format!(
        "{}工作汇报（{} ~ {}）\n\n{}\n\n数据附录：总工时 {:.1}h，任务数 {}，项目数 {}。",
        label, begin, end, summary, total_hours, task_count, project_count
    )
}
