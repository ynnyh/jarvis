//! 配置加载/保存

use tauri::Emitter;
use crate::commands;

#[tauri::command]
pub fn config_load() -> Result<serde_json::Value, String> {
    let path = commands::config_path();
    let defaults = commands::default_config();
    if !path.exists() {
        let mut defaults = defaults;
        commands::hydrate_secret_placeholders(&mut defaults);
        return Ok(defaults);
    }
    let content = std::fs::read_to_string(&path).map_err(|e| format!("读取配置失败: {}", e))?;
    let mut value: serde_json::Value =
        serde_json::from_str(&content).map_err(|e| format!("配置文件解析失败: {}", e))?;
    commands::merge_defaults(&mut value, &defaults);
    let mut persist_migrated = value.clone();
    commands::strip_secrets_for_save(&mut persist_migrated)?;
    let migrated = persist_migrated != value;
    if migrated {
        let _ = crate::util::write_atomic(
            &path,
            &serde_json::to_string_pretty(&persist_migrated).unwrap_or_else(|_| content.clone()),
        );
        value = persist_migrated;
    }
    commands::hydrate_secret_placeholders(&mut value);
    Ok(value)
}

#[tauri::command]
pub async fn config_save(config: serde_json::Value, app: tauri::AppHandle) -> Result<(), String> {
    let dir = commands::jarvis_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建配置目录失败: {}", e))?;
    let path = commands::config_path();
    let mut sanitized = config;
    commands::strip_secrets_for_save(&mut sanitized)?;
    let content =
        serde_json::to_string_pretty(&sanitized).map_err(|e| format!("配置序列化失败: {}", e))?;
    {
        let _guard = crate::settings::CONFIG_WRITE_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::util::write_atomic(&path, &content).map_err(|e| format!("写入配置失败: {}", e))?;
    }

    // 通知所有窗口配置已变更
    let _ = app.emit("config-changed", ());
    Ok(())
}
