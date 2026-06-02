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

/// 从禅道拉取指定项目的参与人员（从任务 team/assignedTo 提取 account，去重）。
pub async fn cost_team_members_inner(project_name: &str) -> Result<Vec<String>, String> {
    let client = crate::zentao::ZentaoClient::from_settings()?;
    client.get_project_team_members(project_name).await
}

#[tauri::command]
pub async fn cost_team_members(project_name: String) -> Result<Vec<String>, String> {
    cost_team_members_inner(&project_name).await
}

// ============================================================================
// 项目成本汇总——纯禅道数据，不走帆软
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

/// 解析 JSON 值为 f64，兼容数字和字符串两种格式（禅道 consumed 字段是字符串如 "0.00"）。
fn json_as_f64(v: &serde_json::Value) -> f64 {
    if let Some(n) = v.as_f64() {
        return n;
    }
    if let Some(s) = v.as_str() {
        return s.parse::<f64>().unwrap_or(0.0);
    }
    0.0
}

/// 从禅道 task team 数据聚合每人已消耗工时 × 时薪 = 成本。
/// 纯禅道数据源，不依赖帆软。team 字段包含每人在此任务上的 consumed。
/// `include_overtime = true` 时，并发拉取每个任务的工作日志，按日期拆分正常/加班工时。
pub async fn project_cost_summary_inner(
    project_name: &str,
    include_overtime: bool,
) -> Result<CostSummaryResult, String> {
    let client = crate::zentao::ZentaoClient::from_settings()?;
    let tasks = client.get_all_project_tasks(&project_name).await?;

    let rates = read_all();

    // account -> (total_hours, task_count)
    let mut hour_map: HashMap<String, (f64, usize)> = HashMap::new();

    for t in &tasks {
        let assignee = t.get("assignedTo").and_then(|v| v.as_str()).unwrap_or("");
        let task_consumed = t.get("consumed").map(json_as_f64).unwrap_or(0.0);
        let team_arr = t.get("team").and_then(|v| v.as_array());

        // 团队成员 consumed（主要数据源）
        if let Some(team) = team_arr {
            for m in team {
                let account = m.get("account").and_then(|v| v.as_str()).unwrap_or("");
                let consumed = m.get("consumed").map(json_as_f64).unwrap_or(0.0);
                if !account.is_empty() && consumed > 0.0 {
                    let entry = hour_map.entry(account.to_string()).or_default();
                    entry.0 += consumed;
                    entry.1 += 1;
                }
            }
        }
        // 负责人 consumed（如果不在 team 里则补充）
        if !assignee.is_empty() && task_consumed > 0.0 {
            let entry = hour_map.entry(assignee.to_string()).or_default();
            if entry.0 == 0.0 {
                entry.0 += task_consumed;
                entry.1 += 1;
            }
        }
    }

    // 加班拆分：并发拉取每个任务的工作日志
    // account -> (normal_hours, overtime_hours)
    let overtime_map = if include_overtime {
        Some(fetch_overtime_breakdown(&client, &tasks).await?)
    } else {
        None
    };

    let mut members: Vec<MemberCost> = hour_map
        .into_iter()
        .map(|(account, (hours, task_count))| {
            let rate = rates.get(&account).map(|r| r.hourly_rate).unwrap_or(0.0);
            let display_name = rates
                .get(&account)
                .map(|r| r.display_name.clone())
                .unwrap_or_else(|| account.clone());

            let (normal_hours, overtime_hours, normal_cost, overtime_cost) =
                if let Some(ref omap) = overtime_map {
                    let (nh, oh) = omap.get(&account).copied().unwrap_or((0.0, 0.0));
                    // 如果工作日志没覆盖全部工时，剩余部分按正常工时补
                    let logged = nh + oh;
                    let nh = if logged < hours && logged > 0.0 {
                        nh + (hours - logged)
                    } else {
                        nh
                    };
                    (Some(nh), Some(oh), Some(nh * rate), Some(oh * rate))
                } else {
                    (None, None, None, None)
                };

            MemberCost {
                cost: hours * rate,
                display_name,
                account,
                hours,
                hourly_rate: rate,
                task_count,
                normal_hours,
                overtime_hours,
                normal_cost,
                overtime_cost,
            }
        })
        .collect();
    members.sort_by(|a, b| b.hours.partial_cmp(&a.hours).unwrap_or(std::cmp::Ordering::Equal));

    let total_hours: f64 = members.iter().map(|m| m.hours).sum();
    let total_cost: f64 = members.iter().map(|m| m.cost).sum();
    let total_normal_hours = if overtime_map.is_some() {
        Some(members.iter().map(|m| m.normal_hours.unwrap_or(0.0)).sum())
    } else {
        None
    };
    let total_overtime_hours = if overtime_map.is_some() {
        Some(members.iter().map(|m| m.overtime_hours.unwrap_or(0.0)).sum())
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

/// 并发拉取所有任务的工作日志，按 (account, date) 聚合后拆分正常/加班。
/// 返回 account -> (normal_hours, overtime_hours)。
async fn fetch_overtime_breakdown(
    client: &crate::zentao::ZentaoClient,
    tasks: &[serde_json::Value],
) -> Result<HashMap<String, (f64, f64)>, String> {
    use futures_util::future::join_all;

    // 收集有 consumed > 0 的任务 ID
    let task_ids: Vec<String> = tasks
        .iter()
        .filter(|t| {
            t.get("consumed").map(json_as_f64).unwrap_or(0.0) > 0.0
                || t.get("team")
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter().any(|m| m.get("consumed").map(json_as_f64).unwrap_or(0.0) > 0.0))
                    .unwrap_or(false)
        })
        .filter_map(|t| t.get("id").and_then(|v| v.as_u64()).map(|id| id.to_string()))
        .collect();

    if task_ids.is_empty() {
        return Ok(HashMap::new());
    }

    // 并发拉取工作日志（限制并发数避免打爆禅道）
    let chunk_size = 10;
    let mut all_works: Vec<serde_json::Value> = Vec::new();
    for chunk in task_ids.chunks(chunk_size) {
        let futs: Vec<_> = chunk
            .iter()
            .map(|id| client.get_task_works(id))
            .collect();
        let results = join_all(futs).await;
        for r in results {
            match r {
                Ok(works) => all_works.extend(works),
                Err(_) => {} // 单个任务失败不阻塞整体
            }
        }
    }

    // (account, date) -> consumed_hours
    let mut daily_map: HashMap<(String, String), f64> = HashMap::new();
    for w in &all_works {
        let account = w.get("account").and_then(|v| v.as_str()).unwrap_or("");
        let date = w.get("date").and_then(|v| v.as_str()).unwrap_or("");
        let consumed = w.get("consumed").map(json_as_f64).unwrap_or(0.0);
        if !account.is_empty() && !date.is_empty() && consumed > 0.0 {
            let entry = daily_map
                .entry((account.to_string(), date.to_string()))
                .or_insert(0.0);
            *entry += consumed;
        }
    }

    // 按 (account, date) 拆分正常/加班
    // 工作日：≤8h 正常，>8h 部分加班；非工作日：全部加班
    let mut result: HashMap<String, (f64, f64)> = HashMap::new();
    for ((account, date), hours) in &daily_map {
        let is_workday = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
            .ok()
            .map(|d| chinese_holiday::chinese_holiday(&d).is_workday())
            .unwrap_or(true); // 解析失败默认当工作日

        let (normal, overtime) = if is_workday {
            let n = (*hours).min(8.0);
            let o = (*hours - 8.0).max(0.0);
            (n, o)
        } else {
            (0.0, *hours)
        };

        let entry = result.entry(account.clone()).or_insert((0.0, 0.0));
        entry.0 += normal;
        entry.1 += overtime;
    }

    Ok(result)
}

#[tauri::command]
pub async fn project_cost_summary(
    project_name: String,
    include_overtime: Option<bool>,
) -> Result<CostSummaryResult, String> {
    project_cost_summary_inner(&project_name, include_overtime.unwrap_or(false)).await
}
