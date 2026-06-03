/// 人员时薪配置，存于 ~/.jarvis/cost-rates.json。
///
/// 结构: { "account": { "hourlyRate": 150, "displayName": "张三" }, ... }

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

fn jarvis_dir() -> PathBuf {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_default();
    PathBuf::from(home).join(".jarvis")
}

fn rates_path() -> PathBuf {
    jarvis_dir().join("cost-rates.json")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonRate {
    #[serde(rename = "hourlyRate")]
    pub hourly_rate: f64,
    #[serde(rename = "displayName")]
    pub display_name: String,
}

pub type RatesMap = HashMap<String, PersonRate>;

fn read_all() -> RatesMap {
    let path = rates_path();
    if !path.exists() {
        return HashMap::new();
    }
    let Ok(raw) = fs::read_to_string(&path) else {
        return HashMap::new();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

fn write_all(map: &RatesMap) -> Result<(), String> {
    let dir = jarvis_dir();
    fs::create_dir_all(&dir).map_err(|e| format!("创建配置目录失败: {}", e))?;
    let content =
        serde_json::to_string_pretty(map).map_err(|e| format!("时薪表序列化失败: {}", e))?;
    crate::util::write_atomic(&rates_path(), &content)
        .map_err(|e| format!("写入时薪表失败: {}", e))?;
    Ok(())
}

#[tauri::command]
pub fn cost_rates_load() -> Result<RatesMap, String> {
    Ok(read_all())
}

#[tauri::command]
pub fn cost_rates_save(rates: RatesMap) -> Result<(), String> {
    write_all(&rates)
}

/// 项目参与人员（带中文名），供设置页时薪表和机器人预览使用。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemberBrief {
    /// 帆软明细无禅道 account，这里直接存员工中文名（与时薪表 key 一致）。
    pub account: String,
    /// 中文名，空时回退为 account
    pub realname: String,
}

/// 从帆软拉指定项目（全周期）的工时明细，提取 distinct 员工中文名作为人员列表。
/// account 与 realname 都填员工中文名（帆软无禅道账号，成本聚合一律以中文名为 key）。
pub async fn cost_team_members_inner(project_name: &str) -> Result<Vec<MemberBrief>, String> {
    let begin = "2020-01-01".to_string();
    let end = chrono::Local::now().format("%Y-%m-%d").to_string();
    let records = crate::fine_report::finereport_get_efforts_raw(
        begin,
        end,
        None,
        true,
        Some(project_name.to_string()),
        "0",
    )
    .await?;

    // PJ_NAME 已粗筛，这里按 project_name 精确兜底，再按 employee 去重（保序）。
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut members: Vec<MemberBrief> = Vec::new();
    for r in &records {
        if r.project_name != project_name {
            continue;
        }
        let employee = r.employee.trim();
        if employee.is_empty() {
            continue;
        }
        if seen.insert(employee.to_string()) {
            members.push(MemberBrief {
                account: employee.to_string(),
                realname: employee.to_string(),
            });
        }
    }
    Ok(members)
}

#[tauri::command]
pub async fn cost_team_members(project_name: String) -> Result<Vec<MemberBrief>, String> {
    cost_team_members_inner(&project_name).await
}

/// 读 ~/.jarvis/config.json 的 workSchedule.periods（[{start:"HH:MM", end:"HH:MM"}]），
/// Σ(end-start)/60 = 每日正常工时阈值；解析失败 / 为空 / 非正 → 8.0。
fn daily_work_hours_from_config() -> f64 {
    fn parse_hm(s: &str) -> Option<f64> {
        let mut it = s.split(':');
        let h: f64 = it.next()?.trim().parse().ok()?;
        let m: f64 = it.next()?.trim().parse().ok()?;
        Some(h * 60.0 + m)
    }

    let cfg = match crate::settings::load_raw_config() {
        Some(c) => c,
        None => return 8.0,
    };
    let periods = cfg
        .get("workSchedule")
        .and_then(|w| w.get("periods"))
        .and_then(|v| v.as_array());
    let Some(periods) = periods else {
        return 8.0;
    };

    let mut minutes = 0.0;
    for p in periods {
        let start = p.get("start").and_then(|v| v.as_str()).and_then(parse_hm);
        let end = p.get("end").and_then(|v| v.as_str()).and_then(parse_hm);
        if let (Some(s), Some(e)) = (start, end) {
            if e > s {
                minutes += e - s;
            }
        }
    }
    let hours = minutes / 60.0;
    if hours > 0.0 {
        hours
    } else {
        8.0
    }
}

// ============================================================================
// 项目成本汇总——数据源为帆软 BI（reportIndex=1 工时任务完成明细），不走禅道 effort
// ============================================================================

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CostSummaryResult {
    pub project_name: String,
    pub members: Vec<MemberCost>,
    pub total_hours: f64,
    pub total_cost: f64,
    #[serde(rename = "totalNormalHours", skip_serializing_if = "Option::is_none")]
    pub total_normal_hours: Option<f64>,
    #[serde(rename = "totalOvertimeHours", skip_serializing_if = "Option::is_none")]
    pub total_overtime_hours: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemberCost {
    pub account: String,
    pub display_name: String,
    pub hours: f64,
    pub hourly_rate: f64,
    pub cost: f64,
    pub task_count: usize,
    #[serde(rename = "normalHours", skip_serializing_if = "Option::is_none")]
    pub normal_hours: Option<f64>,
    #[serde(rename = "overtimeHours", skip_serializing_if = "Option::is_none")]
    pub overtime_hours: Option<f64>,
    #[serde(rename = "normalCost", skip_serializing_if = "Option::is_none")]
    pub normal_cost: Option<f64>,
    #[serde(rename = "overtimeCost", skip_serializing_if = "Option::is_none")]
    pub overtime_cost: Option<f64>,
}

/// 项目成本汇总——数据源为**帆软 BI 工时明细**（reportIndex=1）。
///
/// 流程：帆软按 PJ_NAME + 日期范围一次拉全量明细 → Rust 按 project_name 精确兜底 →
/// 按员工中文名聚合 Σitem_hours + distinct task_name → 按 (employee,date) 聚合后用
/// 每日工时阈值拆分正常/加班 → × 时薪得成本。**完全不调禅道 effort。**
///
/// 聚合 key 为员工中文名（帆软明细无禅道 account）。`MemberCost.account` 存 employee；
/// `display_name` = cost-rates.json 覆盖值（非空且≠employee）否则 employee。
///
/// 不变式：每人 `总工时 == normal + overtime`（同源，无补差）。
/// `start_date` / `end_date` 为 "YYYY-MM-DD"（含端点）；缺省 → 本月 1 号 ~ 今天
/// （原默认全周期 2020-01-01 ~ 今天，数据量太大导致帆软超时，改本月）。
/// `include_resigned`=true 时 USER_STATUS="" 拉全部（含离职）；false 仅在职（"0"）。
pub async fn project_cost_summary_inner(
    project_name: &str,
    include_overtime: bool,
    start_date: Option<&str>,
    end_date: Option<&str>,
    include_resigned: bool,
) -> Result<CostSummaryResult, String> {
    let rates = read_all();

    // 帆软报表必须有日期范围：缺省取本月 1 号 ~ 今天（全周期数据量太大会超时）。
    let now = chrono::Local::now();
    let begin = start_date
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| now.format("%Y-%m-01").to_string());
    let end = end_date
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| now.format("%Y-%m-%d").to_string());

    // 员工状态：含离职 → 不筛（""）；否则仅在职（"0"）。
    let user_status = if include_resigned { "" } else { "0" };

    // 一次帆软查询：PJ_NAME 粗筛 + all_people 拉全部门。
    let records = crate::fine_report::finereport_get_efforts_raw(
        begin.clone(),
        end.clone(),
        None,
        true,
        Some(project_name.to_string()),
        user_status,
    )
    .await?;
    eprintln!(
        "[cost] 帆软返回 {} 条明细（project={}, {} ~ {}）",
        records.len(),
        project_name,
        begin,
        end
    );

    // 聚合：
    //   hour_map: employee -> (Σitem_hours, distinct task_name 集合)
    //   daily_map: (employee, date) -> Σitem_hours
    let mut hour_map: HashMap<String, (f64, std::collections::HashSet<String>)> = HashMap::new();
    let mut daily_map: HashMap<(String, String), f64> = HashMap::new();
    let mut matched = 0usize;

    for r in &records {
        // PJ_NAME 已粗筛，这里按 project_name 精确兜底。
        if r.project_name != project_name {
            continue;
        }
        let employee = r.employee.trim();
        let item_hours = r.item_hours as f64;
        if employee.is_empty() || item_hours <= 0.0 {
            continue;
        }
        matched += 1;

        let entry = hour_map.entry(employee.to_string()).or_default();
        entry.0 += item_hours;
        if !r.task_name.trim().is_empty() {
            entry.1.insert(r.task_name.trim().to_string());
        }

        *daily_map
            .entry((employee.to_string(), r.date.trim().to_string()))
            .or_insert(0.0) += item_hours;
    }
    eprintln!("[cost] project_name 精确匹配后 {} 条记录计入成本", matched);

    // 拆分正常/加班：每日阈值取设置里的工时时段总和（缺省 8h）。
    //   工作日：normal=min(h,threshold)、overtime=(h-threshold).max(0)；
    //   非工作日：normal=0、overtime=h。
    let threshold = daily_work_hours_from_config();
    let mut split_map: HashMap<String, (f64, f64)> = HashMap::new(); // employee -> (normal, overtime)
    for ((employee, date), hours) in &daily_map {
        let is_workday = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
            .ok()
            .map(|d| chinese_holiday::chinese_holiday(&d).is_workday())
            .unwrap_or(true); // 解析失败默认当工作日

        let (normal, overtime) = if is_workday {
            (hours.min(threshold), (*hours - threshold).max(0.0))
        } else {
            (0.0, *hours)
        };
        let entry = split_map.entry(employee.clone()).or_insert((0.0, 0.0));
        entry.0 += normal;
        entry.1 += overtime;
    }

    // 组装 members。display_name = cost-rates.json 覆盖值（非空且≠employee）否则 employee。
    let mut members: Vec<MemberCost> = hour_map
        .into_iter()
        .map(|(employee, (hours, task_set))| {
            let rate = rates.get(&employee).map(|r| r.hourly_rate).unwrap_or(0.0);
            let display_name = rates
                .get(&employee)
                .filter(|r| !r.display_name.is_empty() && r.display_name != employee)
                .map(|r| r.display_name.clone())
                .unwrap_or_else(|| employee.clone());

            let (normal_hours, overtime_hours, normal_cost, overtime_cost) = if include_overtime {
                let (nh, oh) = split_map.get(&employee).copied().unwrap_or((0.0, 0.0));
                (Some(nh), Some(oh), Some(nh * rate), Some(oh * rate))
            } else {
                (None, None, None, None)
            };

            MemberCost {
                cost: hours * rate,
                display_name,
                account: employee,
                hours,
                hourly_rate: rate,
                task_count: task_set.len(),
                normal_hours,
                overtime_hours,
                normal_cost,
                overtime_cost,
            }
        })
        .collect();
    members.sort_by(|a, b| {
        b.hours
            .partial_cmp(&a.hours)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let total_hours: f64 = members.iter().map(|m| m.hours).sum();
    let total_cost: f64 = members.iter().map(|m| m.cost).sum();
    let total_normal_hours = if include_overtime {
        Some(members.iter().map(|m| m.normal_hours.unwrap_or(0.0)).sum())
    } else {
        None
    };
    let total_overtime_hours = if include_overtime {
        Some(
            members
                .iter()
                .map(|m| m.overtime_hours.unwrap_or(0.0))
                .sum(),
        )
    } else {
        None
    };

    Ok(CostSummaryResult {
        project_name: project_name.to_string(),
        members,
        total_hours,
        total_cost,
        total_normal_hours,
        total_overtime_hours,
    })
}

#[tauri::command]
pub async fn project_cost_summary(
    project_name: String,
    include_overtime: Option<bool>,
    start_date: Option<String>,
    end_date: Option<String>,
    include_resigned: Option<bool>,
) -> Result<CostSummaryResult, String> {
    project_cost_summary_inner(
        &project_name,
        include_overtime.unwrap_or(false),
        start_date.as_deref(),
        end_date.as_deref(),
        include_resigned.unwrap_or(false),
    )
    .await
}
