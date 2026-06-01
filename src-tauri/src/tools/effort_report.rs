use std::collections::{HashMap, HashSet};

use chrono::{Datelike, Duration as ChronoDuration, Local, NaiveDate};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

// ============================================================================
// get_efforts：查帆软工时报表
// ============================================================================

#[derive(Debug, Deserialize)]
struct GetEffortsInput {
    #[serde(default)]
    begin: Option<String>,
    #[serde(default)]
    end: Option<String>,
    #[serde(default)]
    range: Option<String>,
    #[serde(default, rename = "realName")]
    real_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GetEffortReportInput {
    begin: Option<String>,
    end: Option<String>,
    #[serde(default)]
    range: Option<String>,
    #[serde(default, rename = "realName")]
    real_name: Option<String>,
}

#[derive(Debug, Serialize)]
struct EffortReportTheme {
    name: String,
    hours: f32,
    task_count: usize,
    project_count: usize,
    systems: Vec<String>,
    tasks: Vec<String>,
    work_items: Vec<String>,
}

#[derive(Debug, Serialize)]
struct EffortReportAppendix {
    begin: String,
    end: String,
    total_hours: f32,
    task_count: usize,
    project_count: usize,
    system_count: usize,
    task_hours: Vec<Value>,
    project_hours: Vec<Value>,
    daily_hours: Vec<Value>,
}

fn holiday_label(kind: chinese_holiday::DayKind) -> Option<&'static str> {
    use chinese_holiday::DayKind;
    match kind {
        DayKind::NormalWorkday | DayKind::NormalHoliday => None,
        DayKind::NewYearsDayHoliday => Some("元旦"),
        DayKind::NewYearsDayWorkday => Some("元旦·调休"),
        DayKind::SpringFestivalHoliday => Some("春节"),
        DayKind::SpringFestivalWorkday => Some("春节·调休"),
        DayKind::ChingMingFestivalHoliday => Some("清明"),
        DayKind::ChingMingFestivalWorkday => Some("清明·调休"),
        DayKind::InternationalWorkersDayHoliday => Some("劳动节"),
        DayKind::InternationalWorkersDayWorkday => Some("劳动节·调休"),
        DayKind::DragonBoatFestivalHoliday => Some("端午"),
        DayKind::DragonBoatFestivalWorkday => Some("端午·调休"),
        DayKind::MidAutumnFestivalHoliday => Some("中秋"),
        DayKind::MidAutumnFestivalWorkday => Some("中秋·调休"),
        DayKind::NationalDayHoliday => Some("国庆"),
        DayKind::NationalDayWorkday => Some("国庆·调休"),
        DayKind::OtherHoliday => Some("假日"),
        DayKind::OtherWorkday => Some("调休"),
    }
}

pub(crate) async fn get_efforts(input: Value) -> Result<Value, String> {
    let parsed: GetEffortsInput =
        serde_json::from_value(input).map_err(|e| format!("get_efforts 入参错误: {}", e))?;
    let (begin, end, range_label) = resolve_effort_range(parsed.begin, parsed.end, parsed.range)?;
    let result =
        crate::fine_report::finereport_get_efforts(begin.clone(), end.clone(), parsed.real_name)
            .await?;

    // 只返回 LLM 需要的字段，截断 summary/detailHtml 避免炸 token
    let records = serde_json::to_value(&result.records)
        .map_err(|e| format!("effort records 序列化失败: {}", e))?;
    let total_hours: f32 = result.records.iter().map(|r| r.item_hours).sum();
    Ok(json!({
        "begin": begin,
        "end": end,
        "range": range_label,
        "records": records,
        "count": result.records.len(),
        "totalHours": total_hours,
    }))
}

pub(crate) async fn get_effort_report(input: Value) -> Result<Value, String> {
    let parsed: GetEffortReportInput =
        serde_json::from_value(input).map_err(|e| format!("get_effort_report 入参错误: {}", e))?;
    let (begin, end, range_label) = resolve_effort_range(parsed.begin, parsed.end, parsed.range)?;
    let result =
        crate::fine_report::finereport_get_efforts(begin.clone(), end.clone(), parsed.real_name)
            .await?;

    let total_hours: f32 = result.records.iter().map(|r| r.item_hours).sum();
    let mut task_hours: HashMap<String, (f32, String)> = HashMap::new();
    let mut project_hours: HashMap<String, f32> = HashMap::new();
    let mut daily_hours: HashMap<String, f32> = HashMap::new();
    let mut theme_map: HashMap<String, EffortReportTheme> = HashMap::new();
    let mut system_set: HashSet<String> = HashSet::new();

    for record in &result.records {
        let task_name = record.task_name.trim();
        let project_name = record.project_name.trim();
        let system_name = record.system.trim();
        let work_content = record.work_content.trim();

        if !task_name.is_empty() {
            let entry = task_hours
                .entry(task_name.to_string())
                .or_insert((0.0, project_name.to_string()));
            entry.0 += record.item_hours;
        }
        if !project_name.is_empty() {
            *project_hours.entry(project_name.to_string()).or_insert(0.0) += record.item_hours;
        }
        if !record.date.trim().is_empty() {
            *daily_hours.entry(record.date.clone()).or_insert(0.0) += record.item_hours;
        }
        if !system_name.is_empty() {
            system_set.insert(system_name.to_string());
        }

        let theme_name = if !project_name.is_empty() {
            project_name.to_string()
        } else if !system_name.is_empty() {
            system_name.to_string()
        } else if !task_name.is_empty() {
            task_name.to_string()
        } else {
            "其他事项".to_string()
        };

        let theme = theme_map.entry(theme_name.clone()).or_insert(EffortReportTheme {
            name: theme_name,
            hours: 0.0,
            task_count: 0,
            project_count: 0,
            systems: Vec::new(),
            tasks: Vec::new(),
            work_items: Vec::new(),
        });
        theme.hours += record.item_hours;
        if !system_name.is_empty() && !theme.systems.iter().any(|s| s == system_name) {
            theme.systems.push(system_name.to_string());
        }
        if !task_name.is_empty() && !theme.tasks.iter().any(|s| s == task_name) {
            theme.tasks.push(task_name.to_string());
        }
        if !project_name.is_empty()
            && !theme.work_items.iter().any(|s| s == project_name)
            && theme.work_items.len() < 8
        {
            theme.work_items.push(project_name.to_string());
        }
        if !work_content.is_empty()
            && !theme.work_items.iter().any(|s| s == work_content)
            && theme.work_items.len() < 8
        {
            theme.work_items.push(work_content.to_string());
        }
    }

    let mut themes: Vec<EffortReportTheme> = theme_map.into_values().collect();
    for theme in &mut themes {
        theme.task_count = theme.tasks.len();
        theme.project_count = theme
            .work_items
            .iter()
            .filter(|s| !s.trim().is_empty())
            .count()
            .max(theme.systems.len());
    }
    themes.sort_by(|a, b| {
        b.hours
            .partial_cmp(&a.hours)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.name.cmp(&b.name))
    });

    let mut task_hours_vec: Vec<Value> = task_hours
        .into_iter()
        .map(|(task, (hours, project))| json!({
            "taskName": task,
            "hours": hours,
            "projectName": project,
        }))
        .collect();
    task_hours_vec.sort_by(|a, b| {
        let ah = a.get("hours").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let bh = b.get("hours").and_then(|v| v.as_f64()).unwrap_or(0.0);
        bh.partial_cmp(&ah).unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut project_hours_vec: Vec<Value> = project_hours
        .into_iter()
        .map(|(project, hours)| json!({
            "projectName": project,
            "hours": hours,
        }))
        .collect();
    project_hours_vec.sort_by(|a, b| {
        let ah = a.get("hours").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let bh = b.get("hours").and_then(|v| v.as_f64()).unwrap_or(0.0);
        bh.partial_cmp(&ah).unwrap_or(std::cmp::Ordering::Equal)
    });

    let weekday_names = ["周日", "周一", "周二", "周三", "周四", "周五", "周六"];
    let mut daily_hours_vec: Vec<Value> = daily_hours
        .into_iter()
        .map(|(date, hours)| {
            let (weekday, holiday, is_workday) = NaiveDate::parse_from_str(&date, "%Y-%m-%d")
                .ok()
                .map(|d| {
                    let dow = d.weekday().num_days_from_sunday() as usize;
                    let kind = chinese_holiday::chinese_holiday(&d);
                    (
                        weekday_names[dow].to_string(),
                        holiday_label(kind),
                        kind.is_workday(),
                    )
                })
                .unwrap_or_else(|| (String::new(), None, false));
            json!({
                "date": date,
                "hours": hours,
                "weekday": weekday,
                "holiday": holiday,
                "isWorkday": is_workday,
            })
        })
        .collect();
    daily_hours_vec.sort_by(|a, b| {
        let ad = a.get("date").and_then(|v| v.as_str()).unwrap_or("");
        let bd = b.get("date").and_then(|v| v.as_str()).unwrap_or("");
        ad.cmp(bd)
    });

    let appendix = EffortReportAppendix {
        begin: begin.clone(),
        end: end.clone(),
        total_hours,
        task_count: task_hours_vec.len(),
        project_count: project_hours_vec.len(),
        system_count: system_set.len(),
        task_hours: task_hours_vec,
        project_hours: project_hours_vec,
        daily_hours: daily_hours_vec,
    };

    let summary_text = build_effort_report_text(&range_label, &appendix, &themes);
    let mode = report_mode_for_range(&range_label, &begin, &end);

    Ok(json!({
        "mode": mode,
        "begin": begin,
        "end": end,
        "range": range_label,
        "summaryText": summary_text,
        "themes": themes,
        "appendix": appendix,
        "records": result.records,
    }))
}

fn resolve_effort_range(
    begin: Option<String>,
    end: Option<String>,
    range: Option<String>,
) -> Result<(String, String, String), String> {
    let today = Local::now().date_naive();
    let range = range
        .as_deref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or("thisWeek");

    let (mut begin_date, mut end_date, label) = match range {
        "today" => (today, today, "today".to_string()),
        "yesterday" => {
            let day = today - ChronoDuration::days(1);
            (day, day, "yesterday".to_string())
        }
        "thisMonth" | "month" => (
            NaiveDate::from_ymd_opt(today.year(), today.month(), 1).unwrap_or(today),
            today,
            "thisMonth".to_string(),
        ),
        "thisQuarter" | "quarter" => {
            let quarter_start_month = ((today.month() - 1) / 3) * 3 + 1;
            (
                NaiveDate::from_ymd_opt(today.year(), quarter_start_month, 1).unwrap_or(today),
                today,
                "thisQuarter".to_string(),
            )
        }
        "last6Months" | "halfYear" | "halfyear" | "half" => (
            today - ChronoDuration::days(182),
            today,
            "last6Months".to_string(),
        ),
        "thisYear" | "year" => (
            NaiveDate::from_ymd_opt(today.year(), 1, 1).unwrap_or(today),
            today,
            "thisYear".to_string(),
        ),
        "thisWeek" | "week" => (
            today - ChronoDuration::days(today.weekday().num_days_from_monday() as i64),
            today,
            "thisWeek".to_string(),
        ),
        "lastWeek" => {
            let dow0 = today.weekday().num_days_from_monday() as i64;
            let this_monday = today - ChronoDuration::days(dow0);
            let last_sunday = this_monday - ChronoDuration::days(1);
            let last_monday = this_monday - ChronoDuration::days(7);
            (last_monday, last_sunday, "lastWeek".to_string())
        }
        other => {
            if begin.is_none() || end.is_none() {
                return Err(format!(
                    "get_efforts range 不支持: {}。可用 today/yesterday/lastWeek/thisWeek/thisMonth/thisQuarter/last6Months/thisYear，或显式传 begin/end。",
                    other
                ));
            }
            (today, today, "custom".to_string())
        }
    };

    if let Some(v) = begin.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        begin_date = parse_effort_date(v, "begin")?;
    }
    if let Some(v) = end.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        end_date = parse_effort_date(v, "end")?;
    }
    if begin_date > end_date {
        return Err(format!(
            "get_efforts 日期范围错误: begin {} 晚于 end {}",
            begin_date, end_date
        ));
    }

    Ok((
        begin_date.format("%Y-%m-%d").to_string(),
        end_date.format("%Y-%m-%d").to_string(),
        label,
    ))
}

fn is_named_report_range_label(label: &str) -> bool {
    matches!(label, "thisMonth" | "thisQuarter" | "last6Months" | "thisYear")
}

fn report_mode_for_range(range_label: &str, begin: &str, end: &str) -> &'static str {
    if is_named_report_range_label(range_label)
        || (range_label == "custom" && range_span_days(begin, end).unwrap_or(0) > 7)
    {
        "report"
    } else {
        "effort"
    }
}

fn range_span_days(begin: &str, end: &str) -> Option<i64> {
    let begin_date = NaiveDate::parse_from_str(begin, "%Y-%m-%d").ok()?;
    let end_date = NaiveDate::parse_from_str(end, "%Y-%m-%d").ok()?;
    Some((end_date - begin_date).num_days() + 1)
}

fn build_effort_report_text(
    range_label: &str,
    appendix: &EffortReportAppendix,
    themes: &[EffortReportTheme],
) -> String {
    let mode = report_mode_for_range(range_label, &appendix.begin, &appendix.end);
    let title = match range_label {
        "today" => "今日工时统计",
        "yesterday" => "昨日工时统计",
        "thisWeek" => "本周工时统计",
        "thisMonth" => "本月工作汇报",
        "thisQuarter" => "本季度工作汇报",
        "last6Months" => "近半年工作汇报",
        "thisYear" => "本年工作汇报",
        "custom" => "阶段工作汇报",
        _ => "工作汇报",
    };

    if mode != "report" {
        let mut lines = vec![format!(
            "{}（{} ~ {}）\n共 {:.1} 小时，涉及 {} 个任务、{} 个项目。",
            title,
            appendix.begin,
            appendix.end,
            appendix.total_hours,
            appendix.task_count,
            appendix.project_count
        )];
        for item in appendix.task_hours.iter().take(8) {
            let task = item.get("taskName").and_then(|v| v.as_str()).unwrap_or("未命名任务");
            let hours = item.get("hours").and_then(|v| v.as_f64()).unwrap_or(0.0);
            lines.push(format!("- {}：{:.1}h", task, hours));
        }
        return lines.join("\n");
    }

    if appendix.total_hours <= 0.0 || themes.is_empty() {
        return [
            format!("{}\n时间范围：{} ~ {}", title, appendix.begin, appendix.end),
            "一、阶段总述".to_string(),
            "本阶段未查询到工时记录，当前还无法基于系统数据生成有效的工作汇报正文。".to_string(),
            String::new(),
            "二、数据附录".to_string(),
            format!(
                "总工时：{:.1}h；任务数：{}；项目数：{}；系统数：{}。",
                appendix.total_hours, appendix.task_count, appendix.project_count, appendix.system_count
            ),
        ]
        .join("\n");
    }

    let top_theme_names = themes
        .iter()
        .take(3)
        .map(|theme| format!("{}（{:.1}h）", theme.name, theme.hours))
        .collect::<Vec<_>>()
        .join("、");
    let daily_distribution = appendix
        .daily_hours
        .iter()
        .map(|item| {
            let date = item.get("date").and_then(|v| v.as_str()).unwrap_or("");
            let hours = item.get("hours").and_then(|v| v.as_f64()).unwrap_or(0.0);
            format!("{} {:.1}h", date, hours)
        })
        .collect::<Vec<_>>()
        .join("；");
    let project_distribution = appendix
        .project_hours
        .iter()
        .map(|item| {
            let name = item
                .get("projectName")
                .and_then(|v| v.as_str())
                .filter(|v| !v.trim().is_empty())
                .unwrap_or("未命名项目");
            let hours = item.get("hours").and_then(|v| v.as_f64()).unwrap_or(0.0);
            format!("{} {:.1}h", name, hours)
        })
        .collect::<Vec<_>>()
        .join("；");
    let task_distribution = appendix
        .task_hours
        .iter()
        .map(|item| {
            let task = item
                .get("taskName")
                .and_then(|v| v.as_str())
                .filter(|v| !v.trim().is_empty())
                .unwrap_or("未命名任务");
            let hours = item.get("hours").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let project = item
                .get("projectName")
                .and_then(|v| v.as_str())
                .filter(|v| !v.trim().is_empty())
                .unwrap_or("未归属项目");
            format!("{} / {} {:.1}h", project, task, hours)
        })
        .collect::<Vec<_>>()
        .join("；");

    let mut lines = vec![
        format!("{}\n时间范围：{} ~ {}", title, appendix.begin, appendix.end),
        format!(
            "本阶段累计投入 {:.1} 小时，覆盖 {} 个任务、{} 个项目、{} 个系统。",
            appendix.total_hours, appendix.task_count, appendix.project_count, appendix.system_count
        ),
        String::new(),
        "一、阶段总述".to_string(),
    ];

    lines.push(format!(
        "本阶段的工作重心主要集中在{}，整体呈现\u{201c}主线推进 + 协同支撑并行\u{201d}的特点。",
        top_theme_names
    ));
    lines.push(format!(
        "从工时分布来看，本阶段累计 {:.1} 小时，工作量主要落在上述重点事项，其余时间用于问题跟进、联调支撑、沟通确认和日常处理。",
        appendix.total_hours
    ));

    lines.push(String::new());
    lines.push("二、重点工作".to_string());
    for (idx, theme) in themes.iter().take(5).enumerate() {
        lines.push(format!(
            "{}. {}：累计约 {:.1} 小时，涉及 {} 个任务。",
            idx + 1,
            theme.name,
            theme.hours,
            theme.task_count
        ));
        if !theme.tasks.is_empty() {
            lines.push(format!(
                "   代表任务：{}",
                theme.tasks.iter().cloned().collect::<Vec<_>>().join("；")
            ));
        }
        if !theme.work_items.is_empty() {
            lines.push(format!(
                "   具体事项：{}",
                theme.work_items.iter().cloned().collect::<Vec<_>>().join("；")
            ));
        }
    }

    lines.push(String::new());
    lines.push("三、协同与支撑工作".to_string());
    if themes.len() > 3 {
        let rest = themes
            .iter()
            .skip(3)
            .map(|t| format!("{}（{:.1}h）", t.name, t.hours))
            .collect::<Vec<_>>()
            .join("、");
        lines.push(format!("除重点事项外，还参与了{}等协同与支撑工作。", rest));
    } else {
        lines.push("除重点事项外，还承担了阶段内的沟通、跟进和日常支撑工作。".to_string());
    }
    if !daily_distribution.is_empty() {
        lines.push(format!("按天分布来看：{}。", daily_distribution));
    }

    lines.push(String::new());
    lines.push("四、风险与遗留事项".to_string());
    if let Some(top) = themes.first() {
        if top.hours >= appendix.total_hours * 0.6 {
            lines.push(format!(
                "{}占用了本阶段的大部分工时，后续需要继续关注该主线的收尾、验证与稳定性。",
                top.name
            ));
        } else {
            lines.push(
                "本阶段工作分布相对分散，后续需要持续关注多项目并行带来的切换成本和跟进完整性。"
                    .to_string(),
            );
        }
    }
    if appendix.task_count > 8 {
        lines.push("涉及任务数量较多，后续整理阶段汇报或复盘时，建议继续按主题归并，避免信息过散。".to_string());
    }

    lines.push(String::new());
    lines.push("五、下一阶段建议".to_string());
    let next_focus = themes
        .iter()
        .take(3)
        .map(|theme| theme.name.clone())
        .collect::<Vec<_>>()
        .join("、");
    lines.push(format!(
        "下一阶段可继续围绕{}推进，优先完成主线事项的闭环，同时把协同类工作沉淀为可复用结论，方便后续汇报和工时确认。",
        next_focus
    ));

    lines.push(String::new());
    lines.push("六、数据附录".to_string());
    lines.push(format!(
        "总工时：{:.1}h；任务数：{}；项目数：{}；系统数：{}。",
        appendix.total_hours, appendix.task_count, appendix.project_count, appendix.system_count
    ));
    if !project_distribution.is_empty() {
        lines.push(format!("项目分布：{}。", project_distribution));
    }
    if !task_distribution.is_empty() {
        lines.push(format!("任务分布：{}。", task_distribution));
    }
    if !daily_distribution.is_empty() {
        lines.push(format!("每日工时：{}。", daily_distribution));
    }

    lines.join("\n")
}

fn parse_effort_date(value: &str, field: &str) -> Result<NaiveDate, String> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|_| format!("get_efforts {} 必须是 YYYY-MM-DD，例如 2026-05-28", field))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_mode_uses_effort_for_short_custom_range() {
        assert_eq!(report_mode_for_range("custom", "2026-05-01", "2026-05-07"), "effort");
        assert_eq!(range_span_days("2026-05-01", "2026-05-07"), Some(7));
    }

    #[test]
    fn report_mode_uses_report_for_long_custom_range() {
        assert_eq!(report_mode_for_range("custom", "2026-05-01", "2026-05-08"), "report");
        assert_eq!(range_span_days("2026-05-01", "2026-05-08"), Some(8));
    }

    #[test]
    fn report_mode_keeps_short_named_ranges_stable() {
        assert_eq!(report_mode_for_range("today", "2026-05-31", "2026-05-31"), "effort");
        assert_eq!(report_mode_for_range("thisWeek", "2026-05-26", "2026-05-31"), "effort");
        assert_eq!(report_mode_for_range("thisMonth", "2026-05-01", "2026-05-31"), "report");
    }

    #[test]
    fn resolve_effort_range_supports_quarter_and_half_year() {
        let quarter = resolve_effort_range(None, None, Some("thisQuarter".to_string())).unwrap();
        assert_eq!(quarter.2, "thisQuarter");

        let half = resolve_effort_range(None, None, Some("last6Months".to_string())).unwrap();
        assert_eq!(half.2, "last6Months");
    }
}
