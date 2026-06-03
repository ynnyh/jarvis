use serde_json::{json, Value};

use chrono::{Datelike, NaiveDate};

/// 把机器人传的周期档/自定义日期解析成 (begin, end)（YYYY-MM-DD，含端点）。
/// 自定义优先：start 和 end 同时给才算自定义；否则按 range 档（缺省本月）。
fn resolve_range(range: Option<&str>, start: Option<&str>, end: Option<&str>) -> (String, String) {
    let start = start.map(str::trim).filter(|s| !s.is_empty());
    let end = end.map(str::trim).filter(|s| !s.is_empty());
    if let (Some(s), Some(e)) = (start, end) {
        return (s.to_string(), e.to_string());
    }

    let today = chrono::Local::now().date_naive();
    let y = today.year();
    let m = today.month(); // 1..=12
    let fmt = |d: NaiveDate| d.format("%Y-%m-%d").to_string();
    let first_of = |yy: i32, mm: u32| NaiveDate::from_ymd_opt(yy, mm, 1).expect("valid first-of-month");
    // 某月最后一天 = 下月 1 号的前一天
    let last_of = |yy: i32, mm: u32| {
        let (ny, nm) = if mm == 12 { (yy + 1, 1) } else { (yy, mm + 1) };
        first_of(ny, nm).pred_opt().expect("valid last-of-month")
    };

    match range.unwrap_or("thisMonth") {
        "thisQuarter" => {
            let qs = ((m - 1) / 3) * 3 + 1; // 1/4/7/10
            let qe = qs + 2;                // 3/6/9/12
            (fmt(first_of(y, qs)), fmt(last_of(y, qe)))
        }
        "halfYear" => {
            // 近半年：含当前月在内往前数 6 个月的 1 号 → 今天
            let mut sy = y;
            let mut sm = m as i32 - 5;
            while sm <= 0 {
                sm += 12;
                sy -= 1;
            }
            (fmt(first_of(sy, sm as u32)), fmt(today))
        }
        "thisYear" => (fmt(first_of(y, 1)), fmt(last_of(y, 12))),
        _ => (fmt(first_of(y, m)), fmt(last_of(y, m))), // thisMonth（含未知值兜底）
    }
}

/// 轻量预览：只拉团队成员列表，不算工时/成本，用于二次确认。
pub(crate) async fn cost_report_preview(input: Value) -> Result<Value, String> {
    let project_name = input
        .get("projectName")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if project_name.is_empty() {
        return Err("projectName 不能为空".into());
    }

    let members = crate::cost_rates::cost_team_members_inner(&project_name).await?;

    // 展示中文名（空则账号），同时把 account 一并带上方便机器人对照。
    let names: Vec<String> = members
        .iter()
        .map(|m| {
            if m.realname.is_empty() || m.realname == m.account {
                m.account.clone()
            } else {
                m.realname.clone()
            }
        })
        .collect();

    Ok(json!({
        "projectName": project_name,
        "memberCount": members.len(),
        "members": names,
    }))
}

/// 项目成本文本报告——供机器人渠道使用，返回格式化纯文本。
pub(crate) async fn cost_report(input: Value) -> Result<Value, String> {
    let project_name = input
        .get("projectName")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if project_name.is_empty() {
        return Err("projectName 不能为空".into());
    }
    let include_overtime = input
        .get("includeOvertime")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let include_resigned = input
        .get("includeResigned")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let range = input.get("range").and_then(|v| v.as_str());
    let start = input.get("startDate").and_then(|v| v.as_str());
    let end = input.get("endDate").and_then(|v| v.as_str());
    let (begin, end_date) = resolve_range(range, start, end);

    let result = crate::cost_rates::project_cost_summary_inner(
        &project_name,
        include_overtime,
        Some(begin.as_str()),
        Some(end_date.as_str()),
        include_resigned,
    )
    .await?;
    let text = format_report(&result, &begin, &end_date);

    Ok(json!({
        "projectName": result.project_name,
        "startDate": begin,
        "endDate": end_date,
        "totalHours": result.total_hours,
        "totalCost": result.total_cost,
        "memberCount": result.members.len(),
        "text": text,
    }))
}

fn fmt_hours(v: f64) -> String {
    if v >= 100.0 {
        format!("{:.0}", v)
    } else {
        format!("{:.1}", v)
    }
}

fn fmt_money(v: f64) -> String {
    if v >= 10000.0 {
        format!("{:.1}万", v / 10000.0)
    } else if v >= 1000.0 {
        format!("{:.0}", v)
    } else {
        format!("{:.0}", v)
    }
}

fn format_report(result: &crate::cost_rates::CostSummaryResult, begin: &str, end: &str) -> String {
    if result.members.is_empty() {
        return format!(
            "📊 项目成本 · {}\n🗓 {} ~ {}\n\n暂无工时数据。",
            result.project_name, begin, end
        );
    }

    let max_hours = result
        .members
        .iter()
        .map(|m| m.hours)
        .fold(0.0_f64, f64::max)
        .max(1.0);

    let has_overtime = result.total_overtime_hours.is_some();

    let mut lines = Vec::new();
    lines.push(format!("📊 项目成本 · {}", result.project_name));
    lines.push(format!("🗓 {} ~ {}", begin, end));
    lines.push(String::new());

    let summary = if has_overtime {
        let nh = result.total_normal_hours.unwrap_or(0.0);
        let oh = result.total_overtime_hours.unwrap_or(0.0);
        format!(
            "总工时 {}h（正常 {}h / 加班 {}h）| 总成本 ¥{} | {}人",
            fmt_hours(result.total_hours), fmt_hours(nh), fmt_hours(oh),
            fmt_money(result.total_cost), result.members.len(),
        )
    } else {
        format!(
            "总工时 {}h | 总成本 ¥{} | {}人",
            fmt_hours(result.total_hours),
            fmt_money(result.total_cost),
            result.members.len(),
        )
    };
    lines.push(summary);
    lines.push(String::new());

    // 条形图 + 明细
    let bar_width = 16;
    for m in &result.members {
        let name = if m.display_name != m.account {
            &m.display_name
        } else {
            &m.account
        };
        let filled = ((m.hours / max_hours) * bar_width as f64).round() as usize;
        let filled = filled.min(bar_width);
        let empty = bar_width - filled;
        let bar: String = "■".repeat(filled) + &"·".repeat(empty);
        let mut line = format!("{} {}h {}", bar, fmt_hours(m.hours), name);
        if has_overtime {
            let nh = m.normal_hours.unwrap_or(0.0);
            let oh = m.overtime_hours.unwrap_or(0.0);
            if oh > 0.0 {
                line += &format!("（正常{}h/加班{}h）", fmt_hours(nh), fmt_hours(oh));
            }
        }
        if m.cost > 0.0 {
            line += &format!(" ¥{}", fmt_money(m.cost));
        }
        lines.push(line);
    }

    lines.join("\n")
}
