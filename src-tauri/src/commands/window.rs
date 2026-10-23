//! 窗口/可见性控制：拖拽、光标、avatar/chat/settings/todayPlan/writeHours/manualHours 开关

use tauri::Manager;

#[tauri::command]
pub async fn drag_window(window: tauri::WebviewWindow) -> Result<(), String> {
    window.start_dragging().map_err(|e| e.to_string())
}

/// 把 app.cursor_position()（物理像素，按「主屏」scale 量化）换成全局逻辑坐标。
///
/// 关键：tao 的 util::cursor_position() 末尾 .to_physical(primary_monitor().scale_factor())，
/// 用的是「主屏」scale，**不是光标当前所在屏**的 scale。混合 DPI 多屏（retina 笔记本 scale=2
/// + 外接 scale=1）下，光标在外接屏时这个物理值仍按主屏 scale 放大，量纲和窗口所在屏不一致。
/// 必须用主屏 scale 把它还原成逻辑坐标，才能和窗口逻辑坐标（按窗口屏 scale 算）对齐。
#[cfg(target_os = "macos")]
fn cursor_logical(app: &tauri::AppHandle) -> Result<(f64, f64), String> {
    let cursor = app.cursor_position().map_err(|e| e.to_string())?;
    let primary_scale = app
        .primary_monitor()
        .map_err(|e| e.to_string())?
        .map(|m| m.scale_factor())
        .unwrap_or(1.0);
    Ok((cursor.x / primary_scale, cursor.y / primary_scale))
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
/// 调了 .to_physical(scale)，cursor 和 outer_position 实际都是 physical。
///
/// v0.10.x 教训补充：cursor_position() 的 to_physical 用的是「主屏」scale，outer_position()
/// 用「窗口屏」scale。早先 (cursor - win)/窗口scale 在单屏/同 DPI 多屏下恰好成立（两 scale 相
/// 等），但混合 DPI 多屏（retina 笔记本 + 外接普通屏）下两者量纲不一致 → 命中测算到窗外 →
/// 小人收不到任何事件（既点不了也拖不动）。正解：cursor 按主屏 scale、win 按窗口屏 scale
/// 各自还原成逻辑坐标再相减。单屏退化为原公式，不影响已验证场景。
///
/// v0.10.6 教训：上面这套「cursor 按主屏 scale 还原」公式仅对 macOS 成立——只有 tao 的
/// macOS util::cursor_position() 末尾做了 .to_physical(主屏 scale)。Windows 的
/// cursor_position() 是 GetCursorPos 纯物理像素，与 outer_position() 同处一个全局物理
/// 坐标系，从未按主屏 scale 量化。在 Windows 上除以主屏 scale 属于凭空引入的错误：主屏上
/// 因「主屏 scale == 窗口屏 scale」两处错误恰好抵消而正常，副屏缩放不同时换算错位 →
/// elementFromPoint 永远打不中 → 小人永久穿透。故此处按平台分叉。
#[tauri::command]
pub fn cursor_pos_in_window(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
) -> Result<(f64, f64), String> {
    let win_pos = window.outer_position().map_err(|e| e.to_string())?;
    let win_scale = window.scale_factor().map_err(|e| e.to_string())?;

    // macOS: tao cursor_position() 按「主屏」scale 量化过 → 先还原全局逻辑再相减（v0.10.4 真机验证）
    #[cfg(target_os = "macos")]
    {
        let (cursor_lx, cursor_ly) = cursor_logical(&app)?;
        Ok((
            cursor_lx - win_pos.x as f64 / win_scale,
            cursor_ly - win_pos.y as f64 / win_scale,
        ))
    }
    // Windows/Linux: GetCursorPos 与 outer_position 同一全局物理坐标系，
    // 差值直接除以窗口屏 scale 即窗口内逻辑坐标；鼠标与窗口同屏时恒正确，
    // 鼠标在别的屏时结果只用于「在窗外」判定，精度无关紧要。
    #[cfg(not(target_os = "macos"))]
    {
        let cursor = app.cursor_position().map_err(|e| e.to_string())?;
        Ok((
            (cursor.x - win_pos.x as f64) / win_scale,
            (cursor.y - win_pos.y as f64) / win_scale,
        ))
    }
}

// ===== Windows 专用：解除原生移动循环对 avatar 窗口顶边的钳制 =====
//
// Windows 原生移动循环（startDragging）会把窗口顶边钳在屏幕上沿，小人（居
// 900 窗中心，头顶 450px 窗体）最高只能拖到屏幕中线。
//
// 2026-07 多轮尝试绕过记录（防后人重复试错）：
// ① WM_MOVING 子类钩子改写系统建议位置 → 系统在改写之上再钳制，无效。
// ② WM_WINDOWPOSCHANGING 子类钩子改写实际落地位置 → 干扰跨屏 DPI 变更。
// ③ 前端 onMoved 里 setPosition(cursor-offset) 校正 → 频率不够打不过 60fps
// 移动循环，跨屏偏移因 DPI 缩放不同而错位。
//
// 最终解法（2026-07-06）：放弃 startDragging，Rust 线程每帧 SetWindowPos 自驱
// 移窗（见 manual_drag.rs），不进 NC move loop → 无内核钳制 → 小人可拖到任意屏幕位置。
// 跨多屏 DPI 按窗口当前物理半宽重算窗口中心对齐光标。macOS 仍走原生 startDragging。

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

/// 统一显隐 avatar 小人窗口，并收敛焦点行为。
///
/// 会藏小人的独立工具窗（todayPlan/writeHours/batchWrite/cost/manualHours）
/// 打开时隐藏小人、关闭时恢复，集中在此避免每条命令重复写 hide/show/focus 三件套，
/// 也避免日后新增窗口漏掉显隐逻辑。chat 自 2026-10 起与小人共存（见 chat_open）；
/// settings 自 2026-10-22 起也与小人共存（设置里拖大小滑杆要实时看到宠物变大小，
/// 见 settings_open）。avatar 窗口不存在时静默跳过（不报错）。
fn set_avatar_visible(app: &tauri::AppHandle, visible: bool) -> Result<(), String> {
    let Some(avatar) = app.get_webview_window("avatar") else {
        return Ok(());
    };
    if visible {
        let _ = avatar.unminimize();
        avatar
            .show()
            .map_err(|e| format!("show avatar 失败: {}", e))?;
        let _ = avatar.set_focus();
    } else {
        avatar
            .hide()
            .map_err(|e| format!("hide avatar 失败: {}", e))?;
    }
    Ok(())
}

// ===== Chat 窗口切换 =====

/// 会藏小人的独立工具窗（todayPlan/writeHours/batchWrite/cost/manualHours）
/// 是否有任一可见。这些窗开着期间小人保持隐藏（v0.10.11 策略，见
/// set_avatar_visible 注释），恢复小人显示前必须先确认没有别的窗仍在藏它。
/// settings 自 2026-10-22 起不再藏小人（见 settings_open），**不在清单内**——
/// 否则「settings 开着时关聊天/关设置」会误判「有工具窗在场」而不点亮小人；
/// settings_close 也用本函数做守卫，与 chat_close 同款模式。
fn any_tool_window_visible(app: &tauri::AppHandle) -> bool {
    ["todayPlan", "writeHours", "batchWrite", "cost", "manualHours"]
        .iter()
        .any(|label| {
            app.get_webview_window(label)
                .map(|w| w.is_visible().unwrap_or(false))
                .unwrap_or(false)
        })
}

#[tauri::command]
pub async fn chat_open(app: tauri::AppHandle) -> Result<(), String> {
    use tauri::Emitter;
    if let Some(chat) = app.get_webview_window("chat") {
        chat.show().map_err(|e| format!("show chat 失败: {}", e))?;
        chat.set_focus()
            .map_err(|e| format!("focus chat 失败: {}", e))?;
    } else {
        return Err("chat 窗口未注册".into());
    }
    // 聊天大窗与 avatar 小人共存（2026-10 活物感改造）：小人不再随 chat 打开而
    // 隐藏，留在桌面上实时反映 agent 状态——前端监听 chat:stream 事件驱动动画
    // 状态机（thinking/working/happy 状态色与 motion）。其余大窗
    // （todayPlan/cost 等）维持「打开即隐藏」策略不变（settings 自 2026-10-22
    // 起也与小人共存，见 settings_open）。chat_close 恢复显示前会先确认没有
    // 其它藏小人的窗仍开着（any_tool_window_visible），不会把它们藏着的小人点亮。
    // 同步前端大窗状态：聊天窗打开 → isChatOpen=true，用于"聊天窗打开期间不弹气泡"等联动。
    app.emit_to("avatar", "chat-window-state", true)
        .map_err(|e| format!("emit chat-window-state 失败: {}", e))?;
    Ok(())
}

#[tauri::command]
pub async fn chat_close(app: tauri::AppHandle) -> Result<(), String> {
    use tauri::Emitter;
    if let Some(chat) = app.get_webview_window("chat") {
        chat.hide().map_err(|e| format!("hide chat 失败: {}", e))?;
    }
    if !any_tool_window_visible(&app) {
        set_avatar_visible(&app, true)?;
    }
    app.emit_to("avatar", "chat-window-state", false)
        .map_err(|e| format!("emit chat-window-state 失败: {}", e))?;
    Ok(())
}

// ===== Settings 详情窗口切换 =====

#[tauri::command]
pub async fn settings_open(app: tauri::AppHandle, page: Option<String>) -> Result<(), String> {
    use tauri::Emitter;
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
            // settings 不再隐藏 avatar 小人（2026-10-22 真机反馈）：小人窗置顶透
            // 明，浮在设置窗上方——用户在设置里拖大小滑杆时能实时看到宠物变大变
            // 小。这里只同步前端状态（isSettingsOpen），供「设置开着不弹临时气
            // 泡」等联动使用；关闭（settings_close）按守卫恢复显示。
            app.emit_to("avatar", "settings-window-state", true)
                .map_err(|e| format!("emit settings-window-state 失败: {}", e))?;
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
    // settings 不藏小人后，这里只是「归还」自己从未拿走的显示权：必须守卫，
    // 否则「todayPlan 等大窗开着时关设置」会把被大窗藏住的小人点亮。对齐
    // chat_close 的 any_tool_window_visible 守卫模式。
    if !any_tool_window_visible(&app) {
        set_avatar_visible(&app, true)?;
    }
    app.emit_to("avatar", "settings-detail-closed", ())
        .map_err(|e| format!("emit settings-detail-closed 失败: {}", e))?;
    // 同步前端：设置窗口已关闭 → isSettingsOpen=false。
    app.emit_to("avatar", "settings-window-state", false)
        .map_err(|e| format!("emit settings-window-state 失败: {}", e))?;
    Ok(())
}

// ===== Today Plan 窗口 =====

#[tauri::command]
pub async fn today_plan_open(app: tauri::AppHandle) -> Result<(), String> {
    use tauri::Emitter;
    if let Some(w) = app.get_webview_window("todayPlan") {
        w.unminimize().ok();
        w.show()
            .map_err(|e| format!("show todayPlan 失败: {}", e))?;
        w.set_focus().ok();
        let _ = w.eval("window.location.reload()");
    } else {
        return Err("todayPlan 窗口未注册".into());
    }
    set_avatar_visible(&app, false)?;
    app.emit_to("avatar", "big-window-state", true)
        .map_err(|e| format!("emit big-window-state 失败: {}", e))?;
    Ok(())
}

#[tauri::command]
pub async fn today_plan_close(app: tauri::AppHandle) -> Result<(), String> {
    use tauri::Emitter;
    if let Some(w) = app.get_webview_window("todayPlan") {
        w.hide()
            .map_err(|e| format!("hide todayPlan 失败: {}", e))?;
    }
    set_avatar_visible(&app, true)?;
    let _ = app.emit_to("avatar", "today-plan-window-closed", ());
    app.emit_to("avatar", "big-window-state", false)
        .map_err(|e| format!("emit big-window-state 失败: {}", e))?;
    Ok(())
}

// ===== 写工时独立窗口 =====

#[tauri::command]
pub async fn write_hours_open(
    app: tauri::AppHandle,
    payload: serde_json::Value,
) -> Result<(), String> {
    use crate::commands::WriteHoursState;
    use tauri::Emitter;
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
    set_avatar_visible(&app, false)?;
    app.emit_to("avatar", "big-window-state", true)
        .map_err(|e| format!("emit big-window-state 失败: {}", e))?;
    Ok(())
}

#[tauri::command]
pub async fn write_hours_close(app: tauri::AppHandle) -> Result<(), String> {
    use crate::commands::WriteHoursState;
    use tauri::Emitter;
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
    set_avatar_visible(&app, true)?;
    app.emit_to("avatar", "big-window-state", false)
        .map_err(|e| format!("emit big-window-state 失败: {}", e))?;
    Ok(())
}

#[tauri::command]
pub async fn avatar_show_fallback(app: tauri::AppHandle) -> Result<(), String> {
    set_avatar_visible(&app, true)?;
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
    use tauri::Emitter;
    if let Some(w) = app.get_webview_window("batchWrite") {
        w.unminimize().ok();
        w.show()
            .map_err(|e| format!("show batchWrite 失败: {}", e))?;
        w.set_focus().ok();
        let _ = w.eval("window.location.reload()");
    } else {
        return Err("batchWrite 窗口未注册".into());
    }
    set_avatar_visible(&app, false)?;
    app.emit_to("avatar", "big-window-state", true)
        .map_err(|e| format!("emit big-window-state 失败: {}", e))?;
    Ok(())
}

#[tauri::command]
pub async fn batch_write_close(app: tauri::AppHandle) -> Result<(), String> {
    use tauri::Emitter;
    if let Some(w) = app.get_webview_window("batchWrite") {
        w.hide().map_err(|e| format!("hide batchWrite 失败: {}", e))?;
    }
    set_avatar_visible(&app, true)?;
    app.emit_to("avatar", "big-window-state", false)
        .map_err(|e| format!("emit big-window-state 失败: {}", e))?;
    Ok(())
}

// ===== Cost 窗口切换 =====

#[tauri::command]
pub async fn cost_open(app: tauri::AppHandle) -> Result<(), String> {
    use tauri::Emitter;
    if let Some(cost) = app.get_webview_window("cost") {
        cost.show()
            .map_err(|e| format!("show cost 失败: {}", e))?;
        cost.set_focus()
            .map_err(|e| format!("focus cost 失败: {}", e))?;
    } else {
        return Err("cost 窗口未注册".into());
    }
    set_avatar_visible(&app, false)?;
    app.emit_to("avatar", "big-window-state", true)
        .map_err(|e| format!("emit big-window-state 失败: {}", e))?;
    Ok(())
}

#[tauri::command]
pub async fn cost_close(app: tauri::AppHandle) -> Result<(), String> {
    use tauri::Emitter;
    if let Some(cost) = app.get_webview_window("cost") {
        cost.hide()
            .map_err(|e| format!("hide cost 失败: {}", e))?;
    }
    set_avatar_visible(&app, true).ok();
    app.emit_to("avatar", "big-window-state", false)
        .map_err(|e| format!("emit big-window-state 失败: {}", e))?;
    Ok(())
}

// ===== 手动写工时窗口 =====

#[tauri::command]
pub async fn manual_hours_open(app: tauri::AppHandle) -> Result<(), String> {
    use tauri::Emitter;
    if let Some(w) = app.get_webview_window("manualHours") {
        w.unminimize().ok();
        w.show()
            .map_err(|e| format!("show manualHours 失败: {}", e))?;
        w.set_focus().ok();
        let _ = w.eval("window.location.reload()");
    } else {
        return Err("manualHours 窗口未注册".into());
    }
    set_avatar_visible(&app, false)?;
    app.emit_to("avatar", "big-window-state", true)
        .map_err(|e| format!("emit big-window-state 失败: {}", e))?;
    Ok(())
}

#[tauri::command]
pub async fn manual_hours_close(app: tauri::AppHandle) -> Result<(), String> {
    use tauri::Emitter;
    if let Some(w) = app.get_webview_window("manualHours") {
        w.hide()
            .map_err(|e| format!("hide manualHours 失败: {}", e))?;
    }
    set_avatar_visible(&app, true)?;
    app.emit_to("avatar", "big-window-state", false)
        .map_err(|e| format!("emit big-window-state 失败: {}", e))?;
    Ok(())
}
