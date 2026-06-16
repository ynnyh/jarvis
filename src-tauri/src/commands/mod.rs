use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

// ===== 子模块声明 =====

pub mod chat;
pub mod config;
pub mod custom_pet;
pub mod deploy_config;
pub mod llm;
pub mod tasks;
pub mod tool;
pub mod window;

// ===== 第三方 re-export（保持 lib.rs 路径不变） =====

pub use chat::*;
pub use config::*;
pub use custom_pet::*;
pub use deploy_config::*;
pub use llm::*;
pub use tasks::*;
pub use tool::*;
pub use window::*;

// ===== 共享工具函数 =====

/// 获取项目根目录（package.json 所在目录）
fn project_root() -> PathBuf {
    let cwd = std::env::current_dir().unwrap_or_default();
    if cwd.join("package.json").exists() {
        cwd
    } else if cwd
        .parent()
        .map(|p| p.join("package.json").exists())
        .unwrap_or(false)
    {
        cwd.parent().unwrap().to_path_buf()
    } else {
        cwd
    }
}

/// 创建不弹出 console 窗口的 Command
fn silent_command(program: &str) -> Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut cmd = Command::new(program);
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        cmd
    }
    #[cfg(not(windows))]
    {
        Command::new(program)
    }
}

/// 用系统默认浏览器打开 URL
fn open_url_in_browser(url: &str) -> Result<(), String> {
    #[cfg(windows)]
    {
        silent_command("cmd")
            .args(["/C", "start", "", url])
            .spawn()
            .map_err(|e| format!("打开浏览器失败: {}", e))?;
        Ok(())
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .map_err(|e| format!("打开浏览器失败: {}", e))?;
        Ok(())
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

/// 简易读取项目根目录下 .env 中指定 key 的值（不依赖 dotenv crate）
fn read_dotenv_value(root: &Path, key: &str) -> Option<String> {
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

/// 导出诊断日志:打包最近 3 天日志 + 脱敏环境摘要,弹保存框。
/// 红线:导出内容不含 apiKey/token/password 明文。
#[tauri::command]
pub fn export_diagnostic_logs() -> Result<String, String> {
    crate::logging::export_diagnostic_logs()
}

// ===== 共享类型 =====

#[derive(Debug, Serialize, Deserialize)]
pub struct ToolResult {
    pub success: bool,
    pub data: Option<serde_json::Value>,
    pub error: Option<String>,
}

#[derive(Default)]
pub struct WriteHoursState {
    pub payload: std::sync::Mutex<Option<serde_json::Value>>,
}

// ===== 配置相关辅助函数 =====

/// 配置文件存储目录 ~/.jarvis/
fn jarvis_dir() -> PathBuf {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_default();
    PathBuf::from(home).join(".jarvis")
}

pub(crate) fn config_path() -> PathBuf {
    jarvis_dir().join("config.json")
}

/// 默认配置（与用户的实际作息一致：8-12 / 14-18，周一到周五）
pub(crate) fn default_config() -> serde_json::Value {
    serde_json::json!({
        "assistantName": "Jarvis",
        "workSchedule": {
            "workDays": [1, 2, 3, 4, 5],
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
            "eveningSummaryMinutesBefore": 30,
            "eveningSummaryChannelNotify": false,
            "effortClosingCheck": true,
            "effortClosingMinutesAfterWork": 10,
            "effortClosingTargetHours": 8,
            "effortClosingRepeatMinutes": 0,
            "effortClosingLatestTime": "21:00",
            "effortClosingChannelNotify": false
        },
        "override": {
            "todayMode": "normal",
            "todayModeSetOn": ""
        },
        "zentao": {
            "baseUrl": "",
            "account": ""
        },
        "llm": {
            "provider": "deepseek",
            "baseUrl": "https://api.deepseek.com",
            "model": "deepseek-chat",
            "apiKey": ""
        },
        "channels": {
            "autoStart": false,
            "telegram": {
                "enabled": false,
                "botToken": "",
                "apiBaseUrl": "https://api.telegram.org",
                "proxy": "",
                "allowChatIds": [],
                "notifyChatIds": []
            },
            "qqbot": {
                "enabled": false,
                "appId": "",
                "appSecret": "",
                "sandbox": false,
                "allowUserIds": [],
                "allowGroupIds": [],
                "notifyUserIds": [],
                "notifyGroupIds": []
            }
        },
        "repoRoots": [],
        "leftClickAction": "tasks",
        "deployEnabled": false
    })
}

fn get_path<'a>(value: &'a serde_json::Value, path: &[&str]) -> Option<&'a serde_json::Value> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    Some(current)
}

fn get_path_str(value: &serde_json::Value, path: &[&str]) -> Option<String> {
    get_path(value, path)
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn set_path_value(value: &mut serde_json::Value, path: &[&str], next: serde_json::Value) {
    let mut current = value;
    for key in &path[..path.len().saturating_sub(1)] {
        if !current.get(*key).map(|v| v.is_object()).unwrap_or(false) {
            current[*key] = serde_json::json!({});
        }
        current = current.get_mut(*key).expect("object path created");
    }
    if let Some(last) = path.last() {
        current[*last] = next;
    }
}

fn mark_secret_if_saved(value: &mut serde_json::Value, path: &[&str], secret_account: &str) {
    if crate::settings::secret_exists(secret_account) {
        set_path_value(
            value,
            path,
            serde_json::Value::String(crate::settings::SECRET_PLACEHOLDER.to_string()),
        );
    }
}

fn extract_secret_to_keychain(
    value: &mut serde_json::Value,
    path: &[&str],
    secret_account: &str,
) -> Result<(), String> {
    let Some(secret) = get_path_str(value, path) else {
        return Ok(());
    };
    if secret == crate::settings::SECRET_PLACEHOLDER {
        return Ok(());
    }
    crate::settings::secret_set(secret_account, &secret)?;
    set_path_value(
        value,
        path,
        serde_json::Value::String(crate::settings::SECRET_PLACEHOLDER.to_string()),
    );
    Ok(())
}

pub(crate) fn hydrate_secret_placeholders(value: &mut serde_json::Value) {
    mark_secret_if_saved(value, &["llm", "apiKey"], "llm.apiKey");
    mark_secret_if_saved(value, &["zentao", "sessionCookie"], "zentao.sessionCookie");
    mark_secret_if_saved(
        value,
        &["channels", "telegram", "botToken"],
        "channels.telegram.botToken",
    );
    mark_secret_if_saved(
        value,
        &["channels", "qqbot", "appSecret"],
        "channels.qqbot.appSecret",
    );
    if let Some(profiles) = value.get_mut("llmProfiles").and_then(|v| v.as_array_mut()) {
        for p in profiles.iter_mut() {
            if let Some(id) = p.get("id").and_then(|v| v.as_str()) {
                let account = format!("llm.profile.{}.apiKey", id);
                mark_secret_if_saved(p, &["apiKey"], &account);
            }
        }
    }
}

pub(crate) fn strip_secrets_for_save(value: &mut serde_json::Value) -> Result<(), String> {
    extract_secret_to_keychain(value, &["llm", "apiKey"], "llm.apiKey")?;
    extract_secret_to_keychain(value, &["zentao", "sessionCookie"], "zentao.sessionCookie")?;
    extract_secret_to_keychain(
        value,
        &["channels", "telegram", "botToken"],
        "channels.telegram.botToken",
    )?;
    extract_secret_to_keychain(
        value,
        &["channels", "qqbot", "appSecret"],
        "channels.qqbot.appSecret",
    )?;
    if let Some(profiles) = value.get_mut("llmProfiles").and_then(|v| v.as_array_mut()) {
        for p in profiles.iter_mut() {
            if let Some(id) = p.get("id").and_then(|v| v.as_str()) {
                let account = format!("llm.profile.{}.apiKey", id);
                extract_secret_to_keychain(p, &["apiKey"], &account)?;
            }
        }
    }
    Ok(())
}

/// 递归把缺失的字段从默认值补齐
/// 检查所有 repoRoots 中有未提交改动的仓库
#[tauri::command]
pub async fn check_dirty_repos() -> Result<Vec<String>, String> {
    let roots = crate::git_scan::get_repo_roots();
    let mut dirty = Vec::new();
    for root in &roots {
        let path = std::path::Path::new(root);
        if !path.join(".git").exists() && !path.exists() {
            continue;
        }
        let output = silent_command("git")
            .arg("-C")
            .arg(root)
            .args(["status", "--porcelain"])
            .output();
        match output {
            Ok(o) if !o.stdout.is_empty() => {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| root.clone());
                dirty.push(name);
            }
            _ => {}
        }
    }
    Ok(dirty)
}

pub(crate) fn merge_defaults(user: &mut serde_json::Value, defaults: &serde_json::Value) {
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
