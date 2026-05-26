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

/// 返回鼠标相对窗口左上角的逻辑坐标（CSS px）。
///
/// 为什么不靠 WebView 的 mousemove + :hover：windowed 透明窗口启用 ignoreCursorEvents
/// 之后，OS 不再向 WebView 派发鼠标事件，CSS :hover 卡在最后一次状态。
///
/// 用 Tauri 的 cursor_position() 直接从 OS 取真实坐标，再换算到窗口本地 CSS 坐标，
/// 让前端 document.elementFromPoint(x, y) 自己判断鼠标下到底是不是可点击元素。
#[tauri::command]
pub fn cursor_pos_in_window(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
) -> Result<(f64, f64), String> {
    let cursor = app.cursor_position().map_err(|e| e.to_string())?;
    let win_pos = window.outer_position().map_err(|e| e.to_string())?;
    let scale = window.scale_factor().map_err(|e| e.to_string())?;

    // 历史教训：Tauri 2.x 在 macOS 上 `app.cursor_position()` 类型标的是
    // PhysicalPosition，但实测返回的是 **logical** 像素；`outer_position()` 仍是
    // physical。直接 (cur - win)/scale 在 retina 副屏（scale=2）上把 cursor 多除
    // 一次 → CSS 坐标变成真实值的两倍 → elementFromPoint 落到窗口外 → 永远判定
    // 不在 UI 上 → setIgnoreCursorEvents(true) → 整窗被穿透。主屏 1920 (scale=1)
    // logical==physical 歪打正着没事，副屏一定挂。
    //
    // 修法：macOS 上先把 win 转 logical 再减 logical 的 cursor。其它平台保持
    // 原算法（实测 Windows 上两者都是 physical，原公式正确）。
    #[cfg(target_os = "macos")]
    {
        let win_x_logical = win_pos.x as f64 / scale;
        let win_y_logical = win_pos.y as f64 / scale;
        Ok((cursor.x - win_x_logical, cursor.y - win_y_logical))
    }

    #[cfg(not(target_os = "macos"))]
    {
        let x = (cursor.x - win_pos.x as f64) / scale;
        let y = (cursor.y - win_pos.y as f64) / scale;
        Ok((x, y))
    }
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
    let input = input.unwrap_or(serde_json::json!({}));
    match crate::tools::dispatch(&name, input).await {
        Ok(data) => Ok(ToolResult { success: true, data: Some(data), error: None }),
        Err(e) => Ok(ToolResult { success: false, data: None, error: Some(e) }),
    }
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
        // 助手显示名，用户可改。默认 Jarvis；影响 UI、问候、写工时审计文本
        "assistantName": "Jarvis",
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
        },
        // 禅道连接信息。密码不在这里，单独存到 OS 密钥链（task #12）
        "zentao": {
            "baseUrl": "",                         // 如 http://zentao.example.com:9538/zentao
            "account": ""                          // 同事的禅道账号名
        },
        // LLM 接入（默认走 DeepSeek，OpenAI 兼容）。apiKey 这阶段先明文存 config，
        // 用户已表态不在乎隐私。换厂商改 provider + baseUrl + model。
        "llm": {
            "provider": "deepseek",                // deepseek | openai | custom
            "baseUrl": "https://api.deepseek.com", // 厂商根域名，客户端拼 /v1/chat/completions
            "model": "deepseek-chat",              // deepseek-chat（V3）/ deepseek-reasoner（R1）
            "apiKey": ""
        },
        // 本地代码根目录列表，用于扫描 git 提交。同事电脑可能放在 D:/work、C:/projects 等
        "repoRoots": [],
        // 左键单击小人弹什么。tasks=任务列表（默认），review=今日复盘
        "leftClickAction": "tasks"
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
pub async fn config_save(config: serde_json::Value) -> Result<(), String> {
    let dir = jarvis_dir();
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("创建配置目录失败: {}", e))?;
    let path = config_path();
    let content = serde_json::to_string_pretty(&config)
        .map_err(|e| format!("配置序列化失败: {}", e))?;
    std::fs::write(&path, content)
        .map_err(|e| format!("写入配置失败: {}", e))?;

    // 通知守护进程刷新缓存——daemon 已完全下线，pinging 已无意义，留个空 op
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
pub async fn fetch_task_alerts(app: tauri::AppHandle) -> Result<Vec<TaskAlert>, String> {
    let root = project_root();

    // 读取 .env 里的 ZENTAO_ACCOUNT，用作"只看我自己"的过滤条件
    let me = read_dotenv_value(&root, "ZENTAO_ACCOUNT")
        .or_else(|| std::env::var("ZENTAO_ACCOUNT").ok())
        .unwrap_or_default();

    // 走原生 Rust zentao client，避开 daemon HTTP 中转。
    // 旧实现走 daemon_client::post("/tool/get_tasks")，M5 之后 daemon 在退场，
    // 直接调 tools::get_tasks 拿数据，前端契约不变。
    let parsed = match crate::tools::get_tasks(serde_json::json!({})).await {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[fetch_task_alerts] zentao 调用失败: {}", e);
            return Err(e);
        }
    };

    // 解析任务列表（兼容两种返回格式）
    let tasks = if let Some(arr) = parsed.as_array() {
        arr.clone()
    } else if let Some(arr) = parsed.get("tasks").and_then(|v| v.as_array()) {
        arr.clone()
    } else {
        vec![]
    };

    // 新任务发现：先对全量"我的活跃任务"做 snapshot diff（不受 deadline 过滤影响 —
    // 即使任务没截止日期或在 1 个月后，也应该被识别为新任务）。
    // 首次启动 snapshot 不存在，diff 返回空 —— 老用户升级时不会被堆积存量任务轰炸。
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
                    return true; // 没配 ZENTAO_ACCOUNT 就全量当成自己的
                }
                let assignee = task.get("assignee").and_then(|v| v.as_str()).unwrap_or("");
                let team_has_me = task
                    .get("team")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter().any(|m| {
                            m.get("account").and_then(|a| a.as_str()) == Some(me.as_str())
                        })
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
                crate::task_snapshot::TaskRef { id, title, priority, deadline }
            })
            .filter(|t| !t.id.is_empty())
            .collect();

        let new_tasks = crate::task_snapshot::diff_and_persist(&my_tasks);
        if !new_tasks.is_empty() {
            // 前端 App.vue 监听 "new-tasks-detected" 事件，逐个弹绑定窗
            let _ = app.emit("new-tasks-detected", &new_tasks);
        }
    }

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
    let mut reminders = vec![];

    // 直接拿任务列表自己判，不走 daemon action
    if let Ok(tasks) = crate::tools::get_tasks(serde_json::json!({})).await {
        let mut has_overdue = false;
        let mut has_today = false;
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        if let Some(arr) = tasks.as_array() {
            for task in arr {
                let deadline = task.get("deadline").and_then(|d| d.as_str()).unwrap_or("");
                if deadline.len() < 10 { continue; }
                let status = task.get("status").and_then(|v| v.as_str()).unwrap_or("");
                if status == "done" || status == "closed" || status == "cancel" { continue; }
                let dl = &deadline[..10];
                if dl < today.as_str() { has_overdue = true; }
                else if dl == today.as_str() { has_today = true; }
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

// ===== Chat 窗口切换 =====
// 设计原则：avatar 与 chat 互斥可见。打开 chat 时 hide avatar，关闭 chat 时 show avatar。
// chat 窗口配置 closable: true 但前端拦截了 onCloseRequested → 调 chat_close 隐藏而非销毁。

#[tauri::command]
pub async fn chat_open(app: tauri::AppHandle) -> Result<(), String> {
    use tauri::Manager;
    if let Some(chat) = app.get_webview_window("chat") {
        chat.show().map_err(|e| format!("show chat 失败: {}", e))?;
        chat.set_focus().map_err(|e| format!("focus chat 失败: {}", e))?;
    } else {
        return Err("chat 窗口未注册".into());
    }
    if let Some(avatar) = app.get_webview_window("avatar") {
        avatar.hide().map_err(|e| format!("hide avatar 失败: {}", e))?;
    }
    Ok(())
}

#[tauri::command]
pub async fn chat_close(app: tauri::AppHandle) -> Result<(), String> {
    use tauri::Manager;
    if let Some(chat) = app.get_webview_window("chat") {
        chat.hide().map_err(|e| format!("hide chat 失败: {}", e))?;
    }
    if let Some(avatar) = app.get_webview_window("avatar") {
        avatar.show().map_err(|e| format!("show avatar 失败: {}", e))?;
        avatar.set_focus().ok();  // 失败不影响主流程
    }
    Ok(())
}

// ===== 写工时独立窗口 =====
// 设计：avatar 上的复盘窗触发 write_hours_open(payload) → Rust 把 payload 存
// 进 state、show writeHours 窗、hide avatar。WriteHoursApp 在 onMounted 调
// write_hours_take_payload 取数据；写入成功后 emit "write-hours-done" 事件让
// 复盘窗把任务标灰，然后调 write_hours_close 恢复 avatar。
//
// payload 用 Mutex<Option<Value>> 存：每次 open 覆盖，take 后清掉。Value 而非
// 具体结构体，避免前后端类型双份维护。

#[derive(Default)]
pub struct WriteHoursState {
    pub payload: std::sync::Mutex<Option<serde_json::Value>>,
}

#[tauri::command]
pub async fn write_hours_open(
    app: tauri::AppHandle,
    payload: serde_json::Value,
) -> Result<(), String> {
    use tauri::Manager;
    // 历史教训：原本想用 emit_to + 200ms 延迟兜底来推 payload，结果
    // listen('write-hours-payload-ready') 和 onFocusChanged 在 Tauri 2.x
    // Windows 上对预注册 + hide/show 复用的窗口都不可靠。第一次 open 时
    // webview 早已 mount 但 onMounted 早跑完，第二次 open 又只是 hide→show，
    // Vue 实例完全不会重新初始化，所以 textarea 一直显示上次的内容。
    //
    // 现在的方案：state 写完后 show 出来，然后立刻 eval("location.reload()")
    // 强制 webview 整页重载 → Vue 实例销毁重建 → onMounted 必然重跑 →
    // loadPayload 从 state 取到刚写入的新数据。同时也顺带清光 textarea。
    {
        let state = app.state::<WriteHoursState>();
        let mut slot = state.payload.lock().map_err(|e| format!("锁 payload 失败: {}", e))?;
        *slot = Some(payload);
    }
    if let Some(w) = app.get_webview_window("writeHours") {
        w.unminimize().ok();
        w.show().map_err(|e| format!("show writeHours 失败: {}", e))?;
        w.set_focus().ok();
        // 强制重载：保证 onMounted 必跑，从 state 拉到本次的新 payload
        let _ = w.eval("window.location.reload()");
    } else {
        return Err("writeHours 窗口未注册".into());
    }
    if let Some(avatar) = app.get_webview_window("avatar") {
        avatar.hide().map_err(|e| format!("hide avatar 失败: {}", e))?;
    }
    Ok(())
}

#[tauri::command]
pub async fn write_hours_close(app: tauri::AppHandle) -> Result<(), String> {
    use tauri::Manager;
    if let Some(w) = app.get_webview_window("writeHours") {
        w.hide().map_err(|e| format!("hide writeHours 失败: {}", e))?;
    }
    // 顺手清掉残留 payload，避免下次取到旧数据
    {
        let state = app.state::<WriteHoursState>();
        let lock_result = state.payload.lock();
        if let Ok(mut slot) = lock_result {
            *slot = None;
        }
    }
    if let Some(avatar) = app.get_webview_window("avatar") {
        // transparent + alwaysOnTop 的 avatar 窗在 Windows 上偶发"hide 后 show 不回来"。
        // 多打几道保险：unminimize（万一被最小化了）→ show → set_focus，全部失败也不阻塞主流程。
        avatar.unminimize().ok();
        avatar.show().map_err(|e| format!("show avatar 失败: {}", e))?;
        avatar.set_focus().ok();
    }
    Ok(())
}

#[tauri::command]
pub async fn avatar_show_fallback(app: tauri::AppHandle) -> Result<(), String> {
    // WriteHoursApp 在 write_hours_close 失败时的兜底入口：单独把 avatar 拽回来，
    // 至少别让用户陷入"小人消失只能重启 app"。
    use tauri::Manager;
    if let Some(avatar) = app.get_webview_window("avatar") {
        avatar.unminimize().ok();
        avatar.show().map_err(|e| format!("show avatar 失败: {}", e))?;
        avatar.set_focus().ok();
    }
    Ok(())
}

#[tauri::command]
pub async fn write_hours_take_payload(
    app: tauri::AppHandle,
) -> Result<Option<serde_json::Value>, String> {
    use tauri::Manager;
    let state = app.state::<WriteHoursState>();
    let slot = state.payload.lock().map_err(|e| format!("锁 payload 失败: {}", e))?;
    // 用 clone 而非 take：onMounted 首次拉 + payload-ready 事件二次拉都能拿到，
    // 不会因为两者抢着 take 导致一方拿到 None。slot 由 write_hours_close 清空。
    Ok(slot.clone())
}
