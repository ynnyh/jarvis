//! 窗口/可见性控制：拖拽、光标、avatar/chat/settings/todayPlan/writeHours/manualHours 开关

use tauri::Manager;

#[tauri::command]
pub async fn drag_window(window: tauri::WebviewWindow) -> Result<(), String> {
    window.start_dragging().map_err(|e| e.to_string())
}

/// 【临时诊断】avatar 拖拽上半屏失效排查：返回窗口 + 各屏幕的原始几何值，
/// 让前端打日志、真机读数定根因。排查完删除（连同 invoke_handler 注册）。
///
/// 返回 JSON：{ outer:{x,y,w,h}, scale, primary:{pos,size,scale}, current:{...} }
/// 关键验证点：primary.size.height ÷ scale 是否等于真实逻辑屏高（tao size() 有 unit 混用嫌疑）；
///            outerPosition.y ÷ scale 是否带 H_logical 偏移（tao outer_position Y 翻转混用）。
#[tauri::command]
pub fn diag_window_geom(window: tauri::WebviewWindow) -> Result<serde_json::Value, String> {
    use serde_json::json;
    let outer_pos = window.outer_position().map_err(|e| e.to_string())?;
    let outer_size = window.outer_size().map_err(|e| e.to_string())?;
    let scale = window.scale_factor().map_err(|e| e.to_string())?;

    fn monitor_json(m: Option<tauri::Monitor>) -> serde_json::Value {
        match m {
            Some(m) => json!({
                "pos": { "x": m.position().x, "y": m.position().y },
                "size": { "w": m.size().width, "h": m.size().height },
                "scale": m.scale_factor(),
            }),
            None => serde_json::Value::Null,
        }
    }

    Ok(json!({
        "outer": {
            "x": outer_pos.x, "y": outer_pos.y,
            "w": outer_size.width, "h": outer_size.height,
        },
        "scale": scale,
        "primary": monitor_json(window.primary_monitor().ok().flatten()),
        "current": monitor_json(window.current_monitor().ok().flatten()),
    }))
}

/// 返回鼠标相对窗口左上角的逻辑坐标（CSS px）。
///
/// 为什么不靠 WebView 的 mousemove + :hover：windowed 透明窗口启用 ignoreCursorEvents
/// 之后，OS 不再向 WebView 派发鼠标事件，CSS :hover 卡在最后一次状态。
///
/// 用 Tauri 的 cursor_position() 直接从 OS 取真实坐标，再换算到窗口本地 CSS 坐标，
/// 让前端 document.elementFromPoint(x, y) 自己判断鼠标下到底是不是可点击元素。
///
/// 历史教训：v0.5.4 曾以为 macOS 上 cursor_position() 返回 logical（与文档标的
/// PhysicalPosition 不符），单独给 macOS 写了"把 win 转 logical 再减 logical 的 cursor"
/// 分支。但 tao 0.35.2 源码（platform_impl/macos/util/mod.rs::cursor_position）末尾
/// 调了 .to_physical(scale)，cursor 和 outer_position 实际都是 physical，两者量纲一致，
/// 统一走 (cursor - win)/scale 即可。v0.5.4 的特殊分支反而导致 retina 屏整窗穿透。
/// v0.10.3 起去掉 macOS 分支统一公式，macOS 真机走完引导页正常，确认该公式正确。
#[tauri::command]
pub fn cursor_pos_in_window(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
) -> Result<(f64, f64), String> {
    let cursor = app.cursor_position().map_err(|e| e.to_string())?;
    let win_pos = window.outer_position().map_err(|e| e.to_string())?;
    let scale = window.scale_factor().map_err(|e| e.to_string())?;

    let x = (cursor.x - win_pos.x as f64) / scale;
    let y = (cursor.y - win_pos.y as f64) / scale;
    Ok((x, y))
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
