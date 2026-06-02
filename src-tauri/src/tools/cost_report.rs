use serde_json::{json, Value};

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

    Ok(json!({
        "projectName": project_name,
        "memberCount": members.len(),
        "members": members,
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

    let result = crate::cost_rates::project_cost_summary_inner(&project_name).await?;
    let text = format_report(&result);

    Ok(json!({
        "projectName": result.project_name,
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

fn format_report(result: &crate::cost_rates::CostSummaryResult) -> String {
    if result.members.is_empty() {
        return format!("📊 项目成本 · {}\n\n暂无工时数据。", result.project_name);
    }

    let max_hours = result
        .members
        .iter()
        .map(|m| m.hours)
        .fold(0.0_f64, f64::max)
        .max(1.0);

    let mut lines = Vec::new();
    lines.push(format!("📊 项目成本 · {}", result.project_name));
    lines.push(String::new());
    lines.push(format!(
        "总工时 {}h | 总成本 ¥{} | {}人",
        fmt_hours(result.total_hours),
        fmt_money(result.total_cost),
        result.members.len(),
    ));
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
        let cost_str = if m.cost > 0.0 {
            format!(" ¥{}", fmt_money(m.cost))
        } else {
            String::new()
        };
        lines.push(format!(
            "{} {}h {}",
            bar,
            fmt_hours(m.hours),
            name,
        ));
        if !cost_str.is_empty() {
            // 在名字后面追加成本，对齐
            lines.last_mut().unwrap().push_str(&cost_str);
        }
    }

    lines.join("\n")
}
