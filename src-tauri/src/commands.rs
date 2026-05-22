use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Command;

/// 获取项目根目录（package.json 所在目录）
fn project_root() -> PathBuf {
    let cwd = std::env::current_dir().unwrap_or_default();
    if cwd.join("package.json").exists() {
        cwd
    } else if cwd.parent().map(|p| p.join("package.json").exists()).unwrap_or(false) {
        cwd.parent().unwrap().to_path_buf()
    } else {
        cwd
    }
}

/// 创建不弹出 console 窗口的 Command
fn silent_command(program: &str) -> Command {
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    cmd
}

#[tauri::command]
pub async fn drag_window(window: tauri::WebviewWindow) -> Result<(), String> {
    window.start_dragging().map_err(|e| e.to_string())
}

// ===== 应用控制 =====

#[tauri::command]
pub fn quit_app(app: tauri::AppHandle) {
    app.exit(0);
}

#[tauri::command]
pub fn toggle_avatar_window(app: tauri::AppHandle) -> Result<(), String> {
    use tauri::Manager;
    if let Some(window) = app.get_webview_window("avatar") {
        if window.is_visible().unwrap_or(false) {
            window.hide().map_err(|e| e.to_string())?;
        } else {
            window.show().map_err(|e| e.to_string())?;
            let _ = window.set_focus();
        }
    }
    Ok(())
}

// ===== Tool 调用 =====

#[derive(Debug, Serialize, Deserialize)]
pub struct ToolResult {
    pub success: bool,
    pub data: Option<serde_json::Value>,
    pub error: Option<String>,
}

#[tauri::command]
pub async fn tool_execute(name: String, input: Option<serde_json::Value>) -> Result<ToolResult, String> {
    let input_json = input.unwrap_or(serde_json::json!({}));
    
    let output = silent_command("node")
        .arg("dist/cli/agent-core.js")
        .arg("tool")
        .arg(&name)
        .arg(input_json.to_string())
        .current_dir(project_root())
        .output()
        .map_err(|e| format!("Failed to execute tool: {}", e))?;
    
    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        // 解析 JSON 输出
        match serde_json::from_str(&stdout) {
            Ok(data) => Ok(ToolResult { success: true, data: Some(data), error: None }),
            Err(_) => Ok(ToolResult { 
                success: true, 
                data: Some(serde_json::json!({ "output": stdout.to_string() })), 
                error: None 
            }),
        }
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Ok(ToolResult { success: false, data: None, error: Some(stderr.to_string()) })
    }
}

#[tauri::command]
pub async fn tool_list() -> Result<Vec<serde_json::Value>, String> {
    let output = silent_command("node")
        .arg("dist/cli/agent-core.js")
        .arg("tools")
        .current_dir(project_root())
        .output()
        .map_err(|e| format!("Failed to list tools: {}", e))?;
    
    if output.status.success() {
        // 简化处理：返回空数组，实际应该解析输出
        Ok(vec![])
    } else {
        Err("Failed to list tools".to_string())
    }
}

// ===== Action 调用 =====

#[tauri::command]
pub async fn action_execute(id: String) -> Result<ToolResult, String> {
    let output = silent_command("node")
        .arg("dist/cli/agent-core.js")
        .arg("action")
        .arg(&id)
        .current_dir(project_root())
        .output()
        .map_err(|e| format!("Failed to execute action: {}", e))?;
    
    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(ToolResult { 
            success: true, 
            data: Some(serde_json::json!({ "output": stdout.to_string() })), 
            error: None 
        })
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Ok(ToolResult { success: false, data: None, error: Some(stderr.to_string()) })
    }
}

#[tauri::command]
pub async fn action_list() -> Result<Vec<serde_json::Value>, String> {
    let output = silent_command("node")
        .arg("dist/cli/agent-core.js")
        .arg("actions")
        .current_dir(project_root())
        .output()
        .map_err(|e| format!("Failed to list actions: {}", e))?;
    
    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(vec![serde_json::json!({ "output": stdout.to_string() })])
    } else {
        Err("Failed to list actions".to_string())
    }
}

// ===== Memory 操作 =====

#[derive(Debug, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub id: String,
    pub r#type: String,
    pub content: String,
    pub tags: Vec<String>,
    pub importance: i32,
    pub created_at: String,
}

#[tauri::command]
pub async fn memory_add(
    r#type: String,
    content: String,
    tags: Vec<String>,
    importance: i32,
) -> Result<MemoryEntry, String> {
    let output = silent_command("node")
        .arg("dist/cli/agent-core.js")
        .arg("memory")
        .arg("add")
        .arg(&r#type)
        .arg(&content)
        .arg(tags.join(","))
        .arg(importance.to_string())
        .current_dir(project_root())
        .output()
        .map_err(|e| format!("Failed to add memory: {}", e))?;
    
    if output.status.success() {
        Ok(MemoryEntry {
            id: format!("mem_{}", chrono::Utc::now().timestamp_millis()),
            r#type,
            content,
            tags,
            importance,
            created_at: chrono::Utc::now().to_rfc3339(),
        })
    } else {
        Err("Failed to add memory".to_string())
    }
}

#[tauri::command]
pub async fn memory_list() -> Result<Vec<MemoryEntry>, String> {
    // 读取内存文件
    let memory_file = std::path::Path::new(".jarvis/memory/memories.json");
    if memory_file.exists() {
        let content = std::fs::read_to_string(memory_file)
            .map_err(|e| format!("Failed to read memory: {}", e))?;
        let entries: Vec<MemoryEntry> = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse memory: {}", e))?;
        Ok(entries)
    } else {
        Ok(vec![])
    }
}

// ===== Agent 状态 =====

#[derive(Debug, Serialize, Deserialize)]
pub struct AgentState {
    pub state: String,
    pub duration: u64,
    pub history_count: usize,
}

#[tauri::command]
pub async fn agent_get_state() -> Result<AgentState, String> {
    let output = silent_command("node")
        .arg("dist/cli/agent-core.js")
        .arg("state")
        .current_dir(project_root())
        .output()
        .map_err(|e| format!("Failed to get state: {}", e))?;
    
    let stdout = String::from_utf8_lossy(&output.stdout);
    
    // 解析状态输出
    let state = if stdout.contains("thinking") {
        "thinking"
    } else if stdout.contains("working") {
        "working"
    } else if stdout.contains("notifying") {
        "notifying"
    } else {
        "idle"
    };
    
    Ok(AgentState {
        state: state.to_string(),
        duration: 0,
        history_count: 0,
    })
}

// ===== Scheduler =====

#[tauri::command]
pub async fn scheduler_start() -> Result<(), String> {
    // 启动调度器（在后台运行）
    std::thread::spawn(|| {
        let _ = silent_command("node")
            .arg("dist/cli/agent-core.js")
            .arg("start")
            .current_dir(project_root())
            .spawn();
    });
    
    Ok(())
}

#[tauri::command]
pub async fn scheduler_status() -> Result<serde_json::Value, String> {
    let output = silent_command("node")
        .arg("dist/cli/agent-core.js")
        .arg("scheduler")
        .current_dir(project_root())
        .output()
        .map_err(|e| format!("Failed to get scheduler status: {}", e))?;
    
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(serde_json::json!({
        "output": stdout.to_string(),
        "running": stdout.contains("运行中: true")
    }))
}

// ===== Context Builder =====

#[tauri::command]
pub async fn context_build() -> Result<String, String> {
    let output = silent_command("node")
        .arg("dist/cli/agent-core.js")
        .arg("context")
        .current_dir(project_root())
        .output()
        .map_err(|e| format!("Failed to build context: {}", e))?;
    
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout.to_string())
}

// ===== 用户配置 =====

/// 配置文件存储目录 ~/.jarvis/
fn jarvis_dir() -> PathBuf {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_default();
    PathBuf::from(home).join(".jarvis")
}

fn config_path() -> PathBuf {
    jarvis_dir().join("config.json")
}

/// 默认配置（与用户的实际作息一致：8-12 / 14-18，周一到周五）
fn default_config() -> serde_json::Value {
    serde_json::json!({
        "workSchedule": {
            "workDays": [1, 2, 3, 4, 5],          // 0=周日 ... 6=周六
            "periods": [
                { "start": "08:00", "end": "12:00", "label": "上午" },
                { "start": "14:00", "end": "18:00", "label": "下午" }
            ]
        },
        "notifications": {
            "quietDuringLunch": true,
            "quietAfterWork": true,
            "quietOnWeekends": true,
            "morningGreeting": true,
            "eveningSummary": true,
            "eveningSummaryMinutesBefore": 30
        },
        "override": {
            "todayMode": "normal",                // normal | overtime | dayoff
            "todayModeSetOn": ""                  // 日期，仅当天有效
        }
    })
}

/// 递归把缺失的字段从默认值补齐
fn merge_defaults(user: &mut serde_json::Value, defaults: &serde_json::Value) {
    if let (Some(u), Some(d)) = (user.as_object_mut(), defaults.as_object()) {
        for (k, v) in d {
            if !u.contains_key(k) {
                u.insert(k.clone(), v.clone());
            } else if v.is_object() {
                merge_defaults(u.get_mut(k).unwrap(), v);
            }
        }
    }
}

#[tauri::command]
pub fn config_load() -> Result<serde_json::Value, String> {
    let path = config_path();
    let defaults = default_config();
    if !path.exists() {
        return Ok(defaults);
    }
    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("读取配置失败: {}", e))?;
    let mut value: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("配置文件解析失败: {}", e))?;
    merge_defaults(&mut value, &defaults);
    Ok(value)
}

#[tauri::command]
pub fn config_save(config: serde_json::Value) -> Result<(), String> {
    let dir = jarvis_dir();
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("创建配置目录失败: {}", e))?;
    let path = config_path();
    let content = serde_json::to_string_pretty(&config)
        .map_err(|e| format!("配置序列化失败: {}", e))?;
    std::fs::write(&path, content)
        .map_err(|e| format!("写入配置失败: {}", e))?;
    Ok(())
}

// ===== 打开禅道任务页 =====

#[tauri::command]
pub async fn open_zentao_task(id: String) -> Result<(), String> {
    let root = project_root();
    let base = read_dotenv_value(&root, "ZENTAO_BASE_URL")
        .or_else(|| std::env::var("ZENTAO_BASE_URL").ok())
        .ok_or_else(|| "ZENTAO_BASE_URL 未配置".to_string())?;

    // 规整 base url
    let base = base.trim_end_matches('/');
    let url = format!("{}/task-view-{}.html", base, id);

    open_url_in_browser(&url)
}

/// 用系统默认浏览器打开 URL
fn open_url_in_browser(url: &str) -> Result<(), String> {
    #[cfg(windows)]
    {
        // Windows 上 start "" "<url>" 通过 cmd 启动；这里直接用 ShellExecute 等价物
        silent_command("cmd")
            .args(["/C", "start", "", url])
            .spawn()
            .map_err(|e| format!("打开浏览器失败: {}", e))?;
        return Ok(());
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .map_err(|e| format!("打开浏览器失败: {}", e))?;
        return Ok(());
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::process::Command::new("xdg-open")
            .arg(url)
            .spawn()
            .map_err(|e| format!("打开浏览器失败: {}", e))?;
        return Ok(());
    }
}

// ===== 任务提醒 =====

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
    pub left_hours: f64,           // 团队任务取 team[me].left，单人取 task.left；这是禅道独立维护的字段
    pub is_team: bool,
}

/// 简易读取项目根目录下 .env 中指定 key 的值（不依赖 dotenv crate）
fn read_dotenv_value(root: &PathBuf, key: &str) -> Option<String> {
    let env_path = root.join(".env");
    let content = std::fs::read_to_string(&env_path).ok()?;
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            if k.trim() == key {
                let v = v.trim().trim_matches(|c| c == '"' || c == '\'');
                return Some(v.to_string());
            }
        }
    }
    None
}

#[tauri::command]
pub async fn fetch_task_alerts() -> Result<Vec<TaskAlert>, String> {
    // 找到项目根目录（agent-core.js 所在位置）
    let root = project_root();

    // 读取 .env 里的 ZENTAO_ACCOUNT，用作"只看我自己"的过滤条件
    // 注意：Rust 端不自动加载 .env，需要手动读
    let me = read_dotenv_value(&root, "ZENTAO_ACCOUNT")
        .or_else(|| std::env::var("ZENTAO_ACCOUNT").ok())
        .unwrap_or_default();

    // 调用 agent-core 获取全部任务（用线程 + mpsc 实现 30 秒超时）
    let root_clone = root.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = silent_command("node")
            .arg("dist/cli/agent-core.js")
            .arg("tool")
            .arg("get_tasks")
            .arg("{}")
            .current_dir(&root_clone)
            .output();
        let _ = tx.send(result);
    });

    let output = match rx.recv_timeout(std::time::Duration::from_secs(30)) {
        Ok(Ok(o)) => o,
        Ok(Err(e)) => {
            let msg = format!("agent-core 调用失败: {}", e);
            eprintln!("[fetch_task_alerts] {} (cwd: {:?})", msg, root);
            return Err(msg);
        }
        Err(_) => {
            let msg = "agent-core 30 秒内未返回（可能 Node 进程未退出或网络阻塞）".to_string();
            eprintln!("[fetch_task_alerts] {}", msg);
            return Err(msg);
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        eprintln!("[fetch_task_alerts] 工具执行失败: {}", stderr);
        // 取最后几行 stderr 作为错误信息
        let tail = stderr.lines().rev().take(3).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join(" | ");
        return Err(format!("agent-core 返回错误: {}", if tail.is_empty() { "未知错误".to_string() } else { tail }));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = match serde_json::from_str(&stdout) {
        Ok(v) => v,
        Err(e) => {
            let msg = format!("禅道返回不是合法 JSON: {}", e);
            eprintln!("[fetch_task_alerts] {}", msg);
            return Err(msg);
        }
    };

    // 解析任务列表（兼容两种返回格式）
    let tasks = if let Some(arr) = parsed.as_array() {
        arr.clone()
    } else if let Some(arr) = parsed.get("tasks").and_then(|v| v.as_array()) {
        arr.clone()
    } else if let Some(output_str) = parsed.get("output").and_then(|v| v.as_str()) {
        serde_json::from_str(output_str).unwrap_or_default()
    } else {
        vec![]
    };

    // 获取今天的日期
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let today_date = chrono::NaiveDate::parse_from_str(&today, "%Y-%m-%d").unwrap_or_default();

    let mut alerts = vec![];
    for task in &tasks {
        let status = task.get("status").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if status == "done" || status == "closed" || status == "cancel" {
            continue;
        }

        let deadline = task.get("deadline").and_then(|v| v.as_str()).unwrap_or("");
        if deadline.len() < 10 || deadline.starts_with("2099") {
            continue;
        }

        let assignee = task.get("assignee").and_then(|v| v.as_str()).unwrap_or("").to_string();

        // 找出"我"在团队任务中的条目（如果有的话）
        // 团队任务的 team 字段是 [{ account, estimate, consumed, left, status }, ...]
        let my_team_entry = task.get("team").and_then(|v| v.as_array()).and_then(|arr| {
            arr.iter().find(|m| {
                m.get("account").and_then(|a| a.as_str()) == Some(&me)
            })
        });

        // 过滤：assignee == 我  或  team 含我
        if !me.is_empty() && assignee != me && my_team_entry.is_none() {
            continue;
        }

        // 如果"我"在这个团队任务里已经是 done，跳过
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

        let id = task.get("id").map(|v| {
            v.as_str().map(|s| s.to_string()).unwrap_or_else(|| v.to_string().trim_matches('"').to_string())
        }).unwrap_or_default();
        let title = task.get("title").and_then(|v| v.as_str())
            .or(task.get("name").and_then(|v| v.as_str()))
            .unwrap_or("")
            .to_string();
        let priority = task.get("priority").and_then(|v| v.as_str()).unwrap_or("normal").to_string();

        // 工时数据：团队任务用"我"在 team 中的工时；普通任务用整个任务的工时
        // left 字段是禅道独立维护的剩余工时（用户可以手动调整，不一定 == estimate - consumed）
        let parse_num = |v: &serde_json::Value| -> f64 {
            v.as_f64()
                .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
                .unwrap_or(0.0)
        };

        let (estimated_hours, consumed_hours, left_hours, my_status) = if let Some(me_entry) = my_team_entry {
            let est = me_entry.get("estimate").map(parse_num).unwrap_or(0.0);
            let con = me_entry.get("consumed").map(parse_num).unwrap_or(0.0);
            // 优先用 left 字段，没有则 fallback 到 est - con
            let left = me_entry
                .get("left")
                .map(parse_num)
                .unwrap_or_else(|| (est - con).max(0.0));
            (
                est,
                con,
                left,
                me_entry.get("status").and_then(|v| v.as_str()).unwrap_or(&status).to_string(),
            )
        } else {
            let est = task.get("estimatedHours").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let con = task.get("consumedHours").and_then(|v| v.as_f64()).unwrap_or(0.0);
            // 任务对象上的 left 字段（蛇形或驼峰）
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

    // 排序：按 days_until_due 升序（最紧迫的在前）
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
    // 尝试获取今日任务
    let output = silent_command("node")
        .arg("dist/cli/agent-core.js")
        .arg("action")
        .arg("get_today_tasks")
        .current_dir(project_root())
        .output();

    let mut reminders = vec![];

    if let Ok(output) = output {
        let stdout = String::from_utf8_lossy(&output.stdout);
        
        // 解析任务数量，生成提醒
        if stdout.contains("延期") || stdout.contains("逾期") {
            reminders.push(ProactiveReminder {
                text: "⚠️ 有任务已延期，建议优先处理".to_string(),
                emoji: "🔥".to_string(),
                state: "warning".to_string(),
            });
        }
        
        if stdout.contains("截止") || stdout.contains("今天") {
            reminders.push(ProactiveReminder {
                text: "⏰ 今天有任务即将截止".to_string(),
                emoji: "📌".to_string(),
                state: "thinking".to_string(),
            });
        }
    }

    // 如果没有任何提醒，返回空数组（前端会降级到本地模拟）
    Ok(reminders)
}

// ===== Git 信息 =====

#[derive(Debug, Serialize, Deserialize)]
pub struct GitInfo {
    pub branch: String,
    pub commit_count: i32,
    pub remote_url: Option<String>,
    pub modified: Vec<String>,
    pub added: Vec<String>,
    pub untracked: Vec<String>,
}

#[tauri::command]
pub async fn git_info() -> Result<GitInfo, String> {
    let output = silent_command("node")
        .arg("dist/cli/agent-core.js")
        .arg("git")
        .current_dir(project_root())
        .output()
        .map_err(|e| format!("Failed to get git info: {}", e))?;
    
    let stdout = String::from_utf8_lossy(&output.stdout);
    
    if stdout.contains("不是 Git 仓库") {
        return Err("Not a git repository".to_string());
    }
    
    // 简化解析
    Ok(GitInfo {
        branch: "main".to_string(),
        commit_count: 0,
        remote_url: None,
        modified: vec![],
        added: vec![],
        untracked: vec![],
    })
}
