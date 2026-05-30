// 设置窗口需要的零散 Tauri 命令：选目录 + 业务线排除列表读写。

use rfd::FileDialog;
use std::fs;
use std::path::PathBuf;

fn jarvis_dir() -> PathBuf {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_default();
    PathBuf::from(home).join(".jarvis")
}

fn excluded_file() -> PathBuf {
    jarvis_dir().join("excluded-business-lines.json")
}

#[tauri::command]
pub async fn pick_directory(title: Option<String>) -> Result<Option<String>, String> {
    // rfd 阻塞调用，在 async 命令里用 spawn_blocking 让 UI 不卡
    let title = title.unwrap_or_else(|| "选择文件夹".to_string());
    let picked = tokio::task::spawn_blocking(move || {
        FileDialog::new().set_title(&title).pick_folder()
    })
    .await
    .map_err(|e| format!("调用文件选择对话框失败: {}", e))?;

    Ok(picked.map(|p| p.to_string_lossy().into_owned()))
}

#[tauri::command]
pub fn excluded_business_lines_load() -> Result<Vec<String>, String> {
    let path = excluded_file();
    if !path.exists() {
        return Ok(vec![]);
    }
    let raw = fs::read_to_string(&path).map_err(|e| format!("读取排除列表失败: {}", e))?;
    let parsed: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|e| format!("排除列表解析失败: {}", e))?;
    let list = parsed
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .filter(|s| !s.trim().is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Ok(list)
}

#[tauri::command]
pub async fn excluded_business_lines_save(lines: Vec<String>) -> Result<(), String> {
    let dir = jarvis_dir();
    fs::create_dir_all(&dir).map_err(|e| format!("创建配置目录失败: {}", e))?;
    let cleaned: Vec<String> = lines
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let content =
        serde_json::to_string_pretty(&cleaned).map_err(|e| format!("序列化失败: {}", e))?;
    crate::util::write_atomic(&excluded_file(), &content).map_err(|e| format!("写入失败: {}", e))?;
    // daemon 已下线，无需通知；下次调 tool 时 Rust 端直接重新读 excluded-business-lines.json
    Ok(())
}
