/// 任务提醒与禅道集成

use serde::{Deserialize, Serialize};
use crate::commands;

#[derive(Debug, Serialize, Deserialize)]
pub struct TaskAlert {
    pub id: String,
    pub title: String,
    pub deadline: String,
    pub assignee: String,
    pub alert_type: String,
    pub days_until_due: i32,
    pub status: String,
    pub priority: String,
    pub estimated_hours: f64,
    pub consumed_hours: f64,
    pub left_hours: f64,
    pub is_team: bool,
}

#[tauri::command]
pub async fn fetch_task_alerts(app: tauri::AppHandle) -> Result<Vec<TaskAlert>, String> {
    let root = commands::project_root();
    let me = commands::read_dotenv_value(&root, "ZENTAO_ACCOUNT")
        .or_else(|| std::env::var("ZENTAO_ACCOUNT").ok())
        .unwrap_or_default();

    let parsed = match crate::tools::get_tasks(serde_json::json!({})).await {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[fetch_task_alerts] zentao 调用失败: {}", e);
            return Err(e);
        }
    };

    let tasks = if let Some(arr) = parsed.as_array() {
        arr.clone()
    } else if let Some(arr) = parsed.get("tasks").and_then(|v| v.as_array()) {
        arr.clone()
    } else {
        vec![]
    };

    // 新任务发现
    {
        use tauri::Emitter;
        let my_tasks: Vec<crate::task_snapshot::TaskRef> = tasks
            .iter()
            .filter(|task| {
                let status = task.get("status").and_then(|v| v.as_str()).unwrap_or("");
                if status == "done" || status == "closed" || status == "cancel" {
                    return false;
                }
                if me.is_empty() {
                    return true;
                }
                let assignee = task.get("assignee").and_then(|v| v.as_str()).unwrap_or("");
                let team_has_me = task
                    .get("team")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .any(|m| m.get("account").and_then(|a| a.as_str()) == Some(me.as_str()))
                    })
                    .unwrap_or(false);
                assignee == me || team_has_me
            })
            .map(|task| {
                let id = task
                    .get("id")
                    .map(|v| {
                        v.as_str()
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| v.to_string().trim_matches('"').to_string())
                    })
                    .unwrap_or_default();
                let title = task
                    .get("title")
                    .and_then(|v| v.as_str())
                    .or(task.get("name").and_then(|v| v.as_str()))
                    .unwrap_or("")
                    .to_string();
                let priority = task
                    .get("priority")
                    .and_then(|v| v.as_str())
                    .unwrap_or("normal")
                    .to_string();
                let deadline = task
                    .get("deadline")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                crate::task_snapshot::TaskRef {
                    id,
                    title,
                    priority,
                    deadline,
                }
            })
            .filter(|t| !t.id.is_empty())
            .collect();

        let new_tasks = crate::task_snapshot::diff_and_persist(&my_tasks);
        if !new_tasks.is_empty() {
            let _ = app.emit("new-tasks-detected", &new_tasks);
        }
    }

    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let today_date = chrono::NaiveDate::parse_from_str(&today, "%Y-%m-%d").unwrap_or_default();

    let mut alerts = vec![];
    for task in &tasks {
        let status = task
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if status == "done" || status == "closed" || status == "cancel" {
            continue;
        }

        let deadline = task.get("deadline").and_then(|v| v.as_str()).unwrap_or("");
        if deadline.len() < 10 || deadline.starts_with("2099") {
            continue;
        }

        let assignee = task
            .get("assignee")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let my_team_entry = task.get("team").and_then(|v| v.as_array()).and_then(|arr| {
            arr.iter()
                .find(|m| m.get("account").and_then(|a| a.as_str()) == Some(&me))
        });

        if !me.is_empty() && assignee != me && my_team_entry.is_none() {
            continue;
        }

        if let Some(me_entry) = my_team_entry {
            if let Some(my_status) = me_entry.get("status").and_then(|v| v.as_str()) {
                if my_status == "done" || my_status == "closed" || my_status == "cancel" {
                    continue;
                }
            }
        }

        let deadline_date = match chrono::NaiveDate::parse_from_str(&deadline[..10], "%Y-%m-%d") {
            Ok(d) => d,
            Err(_) => continue,
        };
        let days_until_due = (deadline_date - today_date).num_days() as i32;

        let alert_type = if days_until_due < 0 {
            "overdue"
        } else if days_until_due == 0 {
            "today"
        } else if days_until_due <= 3 {
            "soon"
        } else if days_until_due <= 7 {
            "upcoming"
        } else {
            continue;
        };

        let id = task
            .get("id")
            .map(|v| {
                v.as_str()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| v.to_string().trim_matches('"').to_string())
            })
            .unwrap_or_default();
        let title = task
            .get("title")
            .and_then(|v| v.as_str())
            .or(task.get("name").and_then(|v| v.as_str()))
            .unwrap_or("")
            .to_string();
        let priority = task
            .get("priority")
            .and_then(|v| v.as_str())
            .unwrap_or("normal")
            .to_string();

        let parse_num = |v: &serde_json::Value| -> f64 {
            v.as_f64()
                .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
                .unwrap_or(0.0)
        };

        let (estimated_hours, consumed_hours, left_hours, my_status) =
            if let Some(me_entry) = my_team_entry {
                let est = me_entry.get("estimate").map(parse_num).unwrap_or(0.0);
                let con = me_entry.get("consumed").map(parse_num).unwrap_or(0.0);
                let left = me_entry
                    .get("left")
                    .map(parse_num)
                    .unwrap_or_else(|| (est - con).max(0.0));
                (
                    est,
                    con,
                    left,
                    me_entry
                        .get("status")
                        .and_then(|v| v.as_str())
                        .unwrap_or(&status)
                        .to_string(),
                )
            } else {
                let est = task
                    .get("estimatedHours")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                let con = task
                    .get("consumedHours")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                let left = task
                    .get("left")
                    .or_else(|| task.get("leftHours"))
                    .map(parse_num)
                    .unwrap_or_else(|| (est - con).max(0.0));
                (est, con, left, status.clone())
            };

        let is_team = my_team_entry.is_some();

        alerts.push(TaskAlert {
            id,
            title,
            deadline: deadline[..10].to_string(),
            assignee,
            alert_type: alert_type.to_string(),
            days_until_due,
            status: my_status,
            priority,
            estimated_hours,
            consumed_hours,
            left_hours,
            is_team,
        });
    }

    alerts.sort_by(|a, b| a.days_until_due.cmp(&b.days_until_due));
    Ok(alerts)
}

// ===== 主动提醒 =====

#[derive(Debug, Serialize, Deserialize)]
pub struct ProactiveReminder {
    pub text: String,
    pub emoji: String,
    pub state: String,
}

#[tauri::command]
pub async fn get_proactive_reminders() -> Result<Vec<ProactiveReminder>, String> {
    let mut reminders = vec![];

    if let Ok(tasks) = crate::tools::get_tasks(serde_json::json!({})).await {
        let mut has_overdue = false;
        let mut has_today = false;
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        if let Some(arr) = tasks.as_array() {
            for task in arr {
                let deadline = task.get("deadline").and_then(|d| d.as_str()).unwrap_or("");
                if deadline.len() < 10 {
                    continue;
                }
                let status = task.get("status").and_then(|v| v.as_str()).unwrap_or("");
                if status == "done" || status == "closed" || status == "cancel" {
                    continue;
                }
                let dl = &deadline[..10];
                if dl < today.as_str() {
                    has_overdue = true;
                } else if dl == today.as_str() {
                    has_today = true;
                }
            }
        }
        if has_overdue {
            reminders.push(ProactiveReminder {
                text: "⚠️ 有任务已延期，建议优先处理".to_string(),
                emoji: "🔥".to_string(),
                state: "warning".to_string(),
            });
        }
        if has_today {
            reminders.push(ProactiveReminder {
                text: "⏰ 今天有任务即将截止".to_string(),
                emoji: "📌".to_string(),
                state: "thinking".to_string(),
            });
        }
    }

    Ok(reminders)
}

// ===== 禅道项目列表 =====

#[tauri::command]
pub async fn list_projects() -> Result<Vec<serde_json::Value>, String> {
    let client = crate::zentao::ZentaoClient::from_settings()?;
    client.list_projects().await
}

// ===== 打开禅道任务页 =====

#[tauri::command]
pub async fn open_zentao_task(id: String) -> Result<(), String> {
    let root = commands::project_root();
    let base = commands::read_dotenv_value(&root, "ZENTAO_BASE_URL")
        .or_else(|| std::env::var("ZENTAO_BASE_URL").ok())
        .ok_or_else(|| "ZENTAO_BASE_URL 未配置".to_string())?;

    let base = base.trim_end_matches('/');
    let url = format!("{}/task-view-{}.html", base, id);

    commands::open_url_in_browser(&url)
}
