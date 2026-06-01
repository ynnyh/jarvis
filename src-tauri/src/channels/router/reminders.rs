use crate::channels::types::AgentReply;
use crate::settings;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::Emitter;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct ScheduledReminder {
    pub id: String,
    pub cron: String,
    pub message: String,
    pub enabled: bool,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
}

/**
 * 支持的命令格式：
 *   定时 HH:MM 提醒内容        → 每天 HH:MM 触发
 *   定时 分 时 日 月 周 提醒内容 → 标准 cron 表达式
 *   定时列表                   → 列出所有提醒
 *   删除定时 N                 → 删除第 N 个提醒
 */
pub(super) fn maybe_handle_reminder_command(app: &tauri::AppHandle, text: &str) -> Option<AgentReply> {
    let trimmed = text.trim();

    // 列表
    if trimmed == "定时列表" || trimmed == "提醒列表" || trimmed == "我的定时" {
        return Some(list_reminder());
    }

    // 删除
    if let Some(idx) = try_parse_delete_reminder(trimmed) {
        let reply = delete_reminder(idx);
        let _ = app.emit("reminders-changed", ());
        return Some(reply);
    }

    // 添加
    if trimmed.starts_with("定时")
        || trimmed.starts_with("添加定时")
        || trimmed.starts_with("添加提醒")
    {
        let reply = add_reminder(trimmed);
        let _ = app.emit("reminders-changed", ());
        return Some(reply);
    }

    None
}

pub(super) fn try_parse_delete_reminder(text: &str) -> Option<usize> {
    let patterns = ["删除定时 ", "删除提醒 ", "取消定时 ", "取消提醒 "];
    for pat in patterns {
        if let Some(rest) = text.strip_prefix(pat) {
            if let Ok(n) = rest.trim().parse::<usize>() {
                if n > 0 {
                    return Some(n - 1);
                }
            }
        }
    }
    None
}

pub(super) fn load_reminders() -> Vec<ScheduledReminder> {
    let cfg = settings::load_raw_config().unwrap_or_else(|| json!({}));
    cfg.get("reminders")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

pub(super) fn save_reminders(reminders: &[ScheduledReminder]) {
    // 持写锁覆盖整个 read-modify-write，避免与设置面板的 config_save 互相覆盖字段。
    let _guard = settings::CONFIG_WRITE_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let mut cfg = settings::load_raw_config().unwrap_or_else(|| json!({}));
    cfg["reminders"] = serde_json::to_value(reminders).unwrap_or(json!([]));
    let path = settings::config_path();
    if let Ok(content) = serde_json::to_string_pretty(&cfg) {
        let _ = crate::util::write_atomic(&path, &content);
    }
}

pub(super) fn add_reminder(text: &str) -> AgentReply {
    // 去掉前缀
    let content = text
        .trim_start_matches("添加定时")
        .trim_start_matches("添加提醒")
        .trim_start_matches("定时")
        .trim();

    let (cron, message) = parse_reminder_input(content);
    if message.is_empty() {
        return AgentReply {
            text:
                "格式不对。用法：\n定时 17:30 写日报\n定时 30 8 * * 1-5 晨会\n定时列表\n删除定时 1"
                    .to_string(),
        };
    }

    let reminder = ScheduledReminder {
        id: format!("r{}", chrono::Utc::now().timestamp_millis()),
        cron: cron.clone(),
        message: message.to_string(),
        enabled: true,
        created_at: chrono::Utc::now().timestamp_millis(),
    };

    let mut reminders = load_reminders();
    reminders.push(reminder);
    save_reminders(&reminders);

    AgentReply {
        text: format!("已添加定时提醒：\nCron: {}\n内容: {}\n\n用「定时列表」查看所有提醒，「删除定时 N」删除。", cron, message),
    }
}

pub(super) fn list_reminder() -> AgentReply {
    let reminders = load_reminders();
    if reminders.is_empty() {
        return AgentReply {
            text: "当前没有定时提醒。发送「定时 17:30 写日报」来添加。".to_string(),
        };
    }

    let mut lines = vec!["📋 定时提醒列表：".to_string()];
    for (i, r) in reminders.iter().enumerate() {
        let status = if r.enabled { "✅" } else { "⏸" };
        lines.push(format!("{}. {} {} — {}", i + 1, status, r.cron, r.message));
    }
    lines.push("\n发送「删除定时 N」删除指定提醒".to_string());

    AgentReply {
        text: lines.join("\n"),
    }
}

pub(super) fn delete_reminder(index: usize) -> AgentReply {
    let mut reminders = load_reminders();
    if index >= reminders.len() {
        return AgentReply {
            text: format!(
                "没有第 {} 个提醒，当前共 {} 个。",
                index + 1,
                reminders.len()
            ),
        };
    }
    let removed = reminders.remove(index);
    save_reminders(&reminders);
    AgentReply {
        text: format!("已删除定时提醒：{} — {}", removed.cron, removed.message),
    }
}

/**
 * 解析用户输入为 (cron, message)。
 *   "17:30 写日报"     → ("30 17 * * *", "写日报")
 *   "30 8 * * 1-5 晨会" → ("30 8 * * 1-5", "晨会")
 */
pub fn parse_reminder_input(input: &str) -> (String, String) {
    // 尝试匹配标准 cron 格式：5 个数字段 + 消息
    let re = regex::Regex::new(r"^(\S+)\s+(\S+)\s+(\S+)\s+(\S+)\s+(\S+)\s+(.+)$").ok();

    if let Some(re) = &re {
        if let Some(caps) = re.captures(input) {
            let fields: Vec<&str> = (1..=5)
                .filter_map(|i| caps.get(i).map(|m| m.as_str()))
                .collect();
            if fields.len() == 5 {
                // 验证是否都是合法的 cron 字段
                let all_valid = fields.iter().all(|f| {
                    f.chars()
                        .all(|c| c.is_ascii_digit() || c == '*' || c == '-' || c == ',' || c == '/')
                });
                if all_valid {
                    let msg = caps.get(6).map(|m| m.as_str().trim()).unwrap_or("");
                    if !msg.is_empty() {
                        return (fields.join(" "), msg.to_string());
                    }
                }
            }
        }
    }

    // 尝试匹配 HH:MM 格式
    let time_re = regex::Regex::new(r"^([0-9]{1,2}):([0-9]{2})\s+(.+)$").ok();

    if let Some(re) = &time_re {
        if let Some(caps) = re.captures(input) {
            let hour: u32 = caps
                .get(1)
                .and_then(|m| m.as_str().parse().ok())
                .unwrap_or(0);
            let minute: u32 = caps
                .get(2)
                .and_then(|m| m.as_str().parse().ok())
                .unwrap_or(0);
            let msg = caps.get(3).map(|m| m.as_str().trim()).unwrap_or("");
            if !msg.is_empty() && hour < 24 && minute < 60 {
                return (format!("{} {} * * *", minute, hour), msg.to_string());
            }
        }
    }

    (String::new(), String::new())
}
