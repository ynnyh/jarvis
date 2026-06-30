//! 窗口/可见性控制：拖拽、光标、avatar/chat/settings/todayPlan/writeHours/manualHours 开关

use tauri::Manager;

#[tauri::command]
pub async fn drag_window(window: tauri::WebviewWindow) -> Result<(), String> {
    window.start_dragging().map_err(|e| e.to_string())
}

/// 返回鼠标相对窗口左上角的逻辑坐标（CSS px），外加原始 OS 值用于诊断。
///
/// 返回元组：(css_x, css_y, cursor_x, cursor_y, win_x, win_y, scale)
///   - (css_x, css_y)：喂给 document.elementFromPoint 的 CSS 逻辑坐标
///   - 后 5 个：OS 原始值。macOS 上 tao 的 DPI 换算有隐患（见下），单靠源码推断
///     cursor 到底是 logical 还是 physical 不可靠，前端会把这些原始值打日志，
///     在真机上一眼定夺，避免再凭公式赌一次。
///
/// 为什么不靠 WebView 的 mousemove + :hover：windowed 透明窗口启用 ignoreCursorEvents
/// 之后，OS 不再向 WebView 派发鼠标事件，CSS :hover 卡在最后一次状态。
#[tauri::command]
pub fn cursor_pos_in_window(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
) -> Result<(f64, f64, f64, f64, i32, i32, f64), String> {
    let cursor = app.cursor_position().map_err(|e| e.to_string())?;
    let win_pos = window.outer_position().map_err(|e| e.to_string())?;
    let scale = window.scale_factor().map_err(|e| e.to_string())?;

    // macOS 坐标换算目前不可靠，正在排查整窗穿透回归（v0.5.4 引入的 "cursor 当
    // logical" 前提与 tao 0.35.2 源码 .to_physical(scale) 矛盾）。诊断期：macOS
    // 暂时和非 macOS 一样用 (cursor - win)/scale，同时把原始值一起返回给前端打
    // 日志，真机读数确认 cursor 真实量级后再定最终公式。
    //
    // 旧版"历史教训"注释（断言 macOS cursor 是 logical）保留作背景：
    //   Tauri 2.x 在 macOS 上 cursor_position() 标 PhysicalPosition，但 v0.5.4 实测
    //   认为返回 logical；outer_position 仍 physical。(cur-win)/scale 在 retina 副屏
    //   (scale=2) 上被指会把 cursor 多除一次 → CSS 坐标 ×2 → elementFromPoint 落窗外
    //   → 全窗判定为非 UI → setIgnoreCursorEvents(true) → 整窗穿透。主屏 scale=1 时
    //   logical==physical 歪打正着没事。但该结论与 tao 源码矛盾，需真机复核。
    #[cfg(target_os = "macos")]
    {
        // 诊断期：先用通用公式（与非 macOS 一致），靠原始值复核。
        let x = (cursor.x - win_pos.x as f64) / scale;
        let y = (cursor.y - win_pos.y as f64) / scale;
        Ok((x, y, cursor.x, cursor.y, win_pos.x, win_pos.y, scale))
    }

    #[cfg(not(target_os = "macos"))]
    {
        let x = (cursor.x - win_pos.x as f64) / scale;
        let y = (cursor.y - win_pos.y as f64) / scale;
        Ok((x, y, cursor.x, cursor.y, win_pos.x, win_pos.y, scale))
    }
}

// ===== 应用控制 =====

#[tauri::command]
pub fn quit_app(app: tauri::AppHandle) {
    app.exit(0);
}

#[tauri::command]
pub fn toggle_avatar_window(app: tauri::AppHandle) -> Result<(), String> {
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

// ===== Chat 窗口切换 =====

#[tauri::command]
pub async fn chat_open(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(chat) = app.get_webview_window("chat") {
        chat.show().map_err(|e| format!("show chat 失败: {}", e))?;
        chat.set_focus()
            .map_err(|e| format!("focus chat 失败: {}", e))?;
    } else {
        return Err("chat 窗口未注册".into());
    }
    // 故意不隐藏 avatar：聊天大窗是常驻可对话窗口，用户希望小人「不消失、保持可见」，
    // 跟其它一开就独占的工具窗（settings/todayPlan/writeHours/...）不同。小人 alwaysOnTop +
    // skipTaskbar，与聊天窗共存不抢任务栏、也不挡操作。其它工具窗仍维持各自的 hide avatar 行为。
    Ok(())
}

#[tauri::command]
pub async fn chat_close(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(chat) = app.get_webview_window("chat") {
        chat.hide().map_err(|e| format!("hide chat 失败: {}", e))?;
    }
    if let Some(avatar) = app.get_webview_window("avatar") {
        avatar
            .show()
            .map_err(|e| format!("show avatar 失败: {}", e))?;
        avatar.set_focus().ok();
    }
    Ok(())
}

// ===== Settings 详情窗口切换 =====

#[tauri::command]
pub async fn settings_open(app: tauri::AppHandle, page: Option<String>) -> Result<(), String> {
    if let Some(settings) = app.get_webview_window("settings") {
        let page = page.unwrap_or_else(|| "channels".to_string());
        let safe_page: String = page
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .collect();
        let safe_page = if safe_page.is_empty() {
            "channels".to_string()
        } else {
            safe_page
        };
        // 已开的窗口只更新 URL 不刷新，侧边栏 SPA 内切换
        let already = settings.is_visible().unwrap_or(false);
        let script = format!(
            "window.history.replaceState(null,'','settings.html?page={}');window.dispatchEvent(new Event('settings-page-changed'));",
            safe_page
        );
        let _ = settings.eval(&script);
        if !already {
            settings
                .show()
                .map_err(|e| format!("show settings 失败: {}", e))?;
        }
        settings
            .set_focus()
            .map_err(|e| format!("focus settings 失败: {}", e))?;
    } else {
        return Err("settings 窗口未注册".into());
    }
    Ok(())
}

#[tauri::command]
pub async fn settings_close(app: tauri::AppHandle) -> Result<(), String> {
    use tauri::Emitter;
    if let Some(settings) = app.get_webview_window("settings") {
        settings
            .hide()
            .map_err(|e| format!("hide settings 失败: {}", e))?;
    }
    if let Some(avatar) = app.get_webview_window("avatar") {
        avatar.show().ok();
        avatar.set_focus().ok();
    }
    app.emit_to("avatar", "settings-detail-closed", ())
        .map_err(|e| format!("emit settings-detail-closed 失败: {}", e))?;
    Ok(())
}

// ===== Today Plan 窗口 =====

#[tauri::command]
pub async fn today_plan_open(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("todayPlan") {
        w.unminimize().ok();
        w.show()
            .map_err(|e| format!("show todayPlan 失败: {}", e))?;
        w.set_focus().ok();
        let _ = w.eval("window.location.reload()");
    } else {
        return Err("todayPlan 窗口未注册".into());
    }
    if let Some(avatar) = app.get_webview_window("avatar") {
        avatar
            .hide()
            .map_err(|e| format!("hide avatar 失败: {}", e))?;
    }
    Ok(())
}

#[tauri::command]
pub async fn today_plan_close(app: tauri::AppHandle) -> Result<(), String> {
    use tauri::Emitter;
    if let Some(w) = app.get_webview_window("todayPlan") {
        w.hide()
            .map_err(|e| format!("hide todayPlan 失败: {}", e))?;
    }
    if let Some(avatar) = app.get_webview_window("avatar") {
        avatar.unminimize().ok();
        avatar
            .show()
            .map_err(|e| format!("show avatar 失败: {}", e))?;
        avatar.set_focus().ok();
    }
    let _ = app.emit_to("avatar", "today-plan-window-closed", ());
    Ok(())
}

// ===== 写工时独立窗口 =====

#[tauri::command]
pub async fn write_hours_open(
    app: tauri::AppHandle,
    payload: serde_json::Value,
) -> Result<(), String> {
    use crate::commands::WriteHoursState;
    {
        let state = app.state::<WriteHoursState>();
        let mut slot = state
            .payload
            .lock()
            .map_err(|e| format!("锁 payload 失败: {}", e))?;
        *slot = Some(payload);
    }
    if let Some(w) = app.get_webview_window("writeHours") {
        w.unminimize().ok();
        w.show()
            .map_err(|e| format!("show writeHours 失败: {}", e))?;
        w.set_focus().ok();
        let _ = w.eval("window.location.reload()");
    } else {
        return Err("writeHours 窗口未注册".into());
    }
    if let Some(avatar) = app.get_webview_window("avatar") {
        avatar
            .hide()
            .map_err(|e| format!("hide avatar 失败: {}", e))?;
    }
    Ok(())
}

#[tauri::command]
pub async fn write_hours_close(app: tauri::AppHandle) -> Result<(), String> {
    use crate::commands::WriteHoursState;
    if let Some(w) = app.get_webview_window("writeHours") {
        w.hide()
            .map_err(|e| format!("hide writeHours 失败: {}", e))?;
    }
    {
        let state = app.state::<WriteHoursState>();
        let lock_result = state.payload.lock();
        if let Ok(mut slot) = lock_result {
            *slot = None;
        }
    }
    if let Some(avatar) = app.get_webview_window("avatar") {
        avatar.unminimize().ok();
        avatar
            .show()
            .map_err(|e| format!("show avatar 失败: {}", e))?;
        avatar.set_focus().ok();
    }
    Ok(())
}

#[tauri::command]
pub async fn avatar_show_fallback(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(avatar) = app.get_webview_window("avatar") {
        avatar.unminimize().ok();
        avatar
            .show()
            .map_err(|e| format!("show avatar 失败: {}", e))?;
        avatar.set_focus().ok();
    }
    Ok(())
}

#[tauri::command]
pub async fn write_hours_take_payload(
    app: tauri::AppHandle,
) -> Result<Option<serde_json::Value>, String> {
    use crate::commands::WriteHoursState;
    let state = app.state::<WriteHoursState>();
    let slot = state
        .payload
        .lock()
        .map_err(|e| format!("锁 payload 失败: {}", e))?;
    Ok(slot.clone())
}

// ===== 批量写工时窗口 =====

#[tauri::command]
pub async fn batch_write_open(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("batchWrite") {
        w.unminimize().ok();
        w.show()
            .map_err(|e| format!("show batchWrite 失败: {}", e))?;
        w.set_focus().ok();
        let _ = w.eval("window.location.reload()");
    } else {
        return Err("batchWrite 窗口未注册".into());
    }
    if let Some(avatar) = app.get_webview_window("avatar") {
        avatar.hide().map_err(|e| format!("hide avatar 失败: {}", e))?;
    }
    Ok(())
}

#[tauri::command]
pub async fn batch_write_close(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("batchWrite") {
        w.hide().map_err(|e| format!("hide batchWrite 失败: {}", e))?;
    }
    if let Some(avatar) = app.get_webview_window("avatar") {
        avatar.unminimize().ok();
        avatar.show().map_err(|e| format!("show avatar 失败: {}", e))?;
        avatar.set_focus().ok();
    }
    Ok(())
}

// ===== Cost 窗口切换 =====

#[tauri::command]
pub async fn cost_open(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(cost) = app.get_webview_window("cost") {
        cost.show()
            .map_err(|e| format!("show cost 失败: {}", e))?;
        cost.set_focus()
            .map_err(|e| format!("focus cost 失败: {}", e))?;
    } else {
        return Err("cost 窗口未注册".into());
    }
    if let Some(avatar) = app.get_webview_window("avatar") {
        avatar
            .hide()
            .map_err(|e| format!("hide avatar 失败: {}", e))?;
    }
    Ok(())
}

#[tauri::command]
pub async fn cost_close(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(cost) = app.get_webview_window("cost") {
        cost.hide()
            .map_err(|e| format!("hide cost 失败: {}", e))?;
    }
    if let Some(avatar) = app.get_webview_window("avatar") {
        avatar.show().ok();
        avatar.set_focus().ok();
    }
    Ok(())
}

// ===== 手动写工时窗口 =====

#[tauri::command]
pub async fn manual_hours_open(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("manualHours") {
        w.unminimize().ok();
        w.show()
            .map_err(|e| format!("show manualHours 失败: {}", e))?;
        w.set_focus().ok();
        let _ = w.eval("window.location.reload()");
    } else {
        return Err("manualHours 窗口未注册".into());
    }
    if let Some(avatar) = app.get_webview_window("avatar") {
        avatar
            .hide()
            .map_err(|e| format!("hide avatar 失败: {}", e))?;
    }
    Ok(())
}

#[tauri::command]
pub async fn manual_hours_close(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("manualHours") {
        w.hide()
            .map_err(|e| format!("hide manualHours 失败: {}", e))?;
    }
    if let Some(avatar) = app.get_webview_window("avatar") {
        avatar.unminimize().ok();
        avatar
            .show()
            .map_err(|e| format!("show avatar 失败: {}", e))?;
        avatar.set_focus().ok();
    }
    Ok(())
}
