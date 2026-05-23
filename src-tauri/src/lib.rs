mod commands;
mod credentials;
mod daemon_client;
mod settings_extras;

use tauri::Manager;
use tauri::RunEvent;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            // ===== 系统托盘 =====
            let show_i = MenuItem::with_id(app, "tray_show", "显示小人", true, None::<&str>)?;
            let hide_i = MenuItem::with_id(app, "tray_hide", "隐藏小人", true, None::<&str>)?;
            let sep = PredefinedMenuItem::separator(app)?;
            let quit_i = MenuItem::with_id(app, "tray_quit", "退出 Jarvis", true, None::<&str>)?;
            let tray_menu = Menu::with_items(app, &[&show_i, &hide_i, &sep, &quit_i])?;

            let _tray = TrayIconBuilder::with_id("main")
                .icon(app.default_window_icon().cloned().expect("missing tray icon"))
                .tooltip("Jarvis · 你的任务助手")
                .menu(&tray_menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "tray_quit" => app.exit(0),
                    "tray_show" => {
                        if let Some(w) = app.get_webview_window("avatar") {
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                    "tray_hide" => {
                        if let Some(w) = app.get_webview_window("avatar") {
                            let _ = w.hide();
                        }
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(w) = app.get_webview_window("avatar") {
                            if w.is_visible().unwrap_or(false) {
                                let _ = w.hide();
                            } else {
                                let _ = w.show();
                                let _ = w.set_focus();
                            }
                        }
                    }
                })
                .build(app)?;

            // ===== 小人窗口初始化 =====
            if let Some(window) = app.get_webview_window("avatar") {
                // 用 Logical 尺寸：CSS 拿到的就是这个数，跟 Windows 缩放无关。
                // 历史教训：之前用 Physical(300,400)，在 150% 缩放显示器（2K 笔记本默认）
                // 上 CSS 只剩 200×267，所有 panel 全挤爆 / 标题换行。
                let logical_w: f64 = 400.0;
                let logical_h: f64 = 560.0;
                let _ = window.set_size(tauri::Size::Logical(tauri::LogicalSize {
                    width: logical_w,
                    height: logical_h,
                }));

                // 获取主显示器信息并设置位置到右下角
                let monitor = window.primary_monitor().ok().flatten()
                    .or_else(|| window.current_monitor().ok().flatten())
                    .or_else(|| window.available_monitors().ok().and_then(|m| m.first().cloned()));

                if let Some(monitor) = monitor {
                    let monitor_size = monitor.size();
                    let monitor_position = monitor.position();
                    let scale = window.scale_factor().unwrap_or(1.0);

                    // monitor 返回的是 physical 坐标，窗口实际物理像素 = logical × scale
                    let physical_w = (logical_w * scale) as i32;
                    let physical_h = (logical_h * scale) as i32;
                    let margin = (20.0 * scale) as i32;
                    // Windows 任务栏在 150% 缩放下 ~60px，统一用 logical 50px 兜底
                    let taskbar = (50.0 * scale) as i32;

                    let x = monitor_position.x + monitor_size.width as i32 - physical_w - margin;
                    let y = monitor_position.y + monitor_size.height as i32 - physical_h - margin - taskbar;

                    let _ = window.set_position(tauri::Position::Physical(tauri::PhysicalPosition { x, y }));
                }

                // 确保窗口可见并聚焦
                let _ = window.show();
                let _ = window.set_focus();
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::drag_window,
            commands::cursor_pos_in_window,
            commands::tool_execute,
            commands::tool_list,
            commands::action_execute,
            commands::action_list,
            commands::memory_add,
            commands::memory_list,
            commands::agent_get_state,
            commands::scheduler_start,
            commands::scheduler_status,
            commands::context_build,
            commands::git_info,
            commands::get_proactive_reminders,
            commands::fetch_task_alerts,
            commands::open_zentao_task,
            commands::quit_app,
            commands::toggle_avatar_window,
            commands::config_load,
            commands::config_save,
            credentials::credentials_set,
            credentials::credentials_get,
            credentials::credentials_delete,
            credentials::zentao_test_connection,
            credentials::daemon_restart,
            settings_extras::pick_directory,
            settings_extras::excluded_business_lines_load,
            settings_extras::excluded_business_lines_save,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app_handle, event| {
            // 应用退出前优雅关闭守护进程，避免 Node 进程残留
            if let RunEvent::Exit = event {
                tauri::async_runtime::block_on(daemon_client::try_shutdown());
            }
        });
}
