mod commands;
mod conversations;
mod credentials;
mod daemon_client;
mod llm;
mod settings;
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

                // 优先窗口当前所在屏（current），fallback 系统主屏（primary），再 fallback 第一个可用屏。
                // 历史教训：用 primary 优先在「笔记本主屏 + 外接副屏」场景会把窗口丢到用户没在看的屏，
                // 用户感受是「欢迎页下一步按钮看不见」（窗口被定位到主屏右下，y 算出来超出当前屏底边）。
                let monitor = window.current_monitor().ok().flatten()
                    .or_else(|| window.primary_monitor().ok().flatten())
                    .or_else(|| window.available_monitors().ok().and_then(|m| m.first().cloned()));

                if let Some(monitor) = monitor {
                    // monitor.position() / size() 返回 PhysicalPosition/Size。
                    // 全部除以 scale_factor 转 logical 再算，最后用 LogicalPosition set 出去——
                    // macOS NSWindow 坐标系是 logical，Windows 也能直接用 logical 设置。
                    // 历史教训：之前用 PhysicalPosition + monitor 的 physical size 算位置，
                    // macOS 上 set_position(Physical) 行为和 Windows 不一致（macOS 把数当 logical 用），
                    // 高分屏笔记本上 y 算出 540 物理但被当 540 logical 用，导致 540+560>900 屏高，
                    // 窗口下半部分被裁，欢迎页"下一步"按钮看不见。
                    let scale = monitor.scale_factor();
                    let m_x = monitor.position().x as f64 / scale;
                    let m_y = monitor.position().y as f64 / scale;
                    let m_w = monitor.size().width as f64 / scale;
                    let m_h = monitor.size().height as f64 / scale;

                    let margin: f64 = 20.0;
                    // 底部留 80：涵盖 Windows 任务栏 (~40) 和 macOS Dock (~80)，宁多勿少
                    let bottom_pad: f64 = 80.0;

                    let x = m_x + m_w - logical_w - margin;
                    let y = m_y + m_h - logical_h - bottom_pad;

                    let _ = window.set_position(tauri::Position::Logical(tauri::LogicalPosition { x, y }));
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
            commands::get_proactive_reminders,
            commands::fetch_task_alerts,
            commands::open_zentao_task,
            commands::quit_app,
            commands::toggle_avatar_window,
            commands::config_load,
            commands::config_save,
            commands::chat_open,
            commands::chat_close,
            credentials::credentials_set,
            credentials::credentials_get,
            credentials::credentials_delete,
            credentials::zentao_test_connection,
            credentials::daemon_restart,
            settings_extras::pick_directory,
            settings_extras::excluded_business_lines_load,
            settings_extras::excluded_business_lines_save,
            conversations::conversations_list,
            conversations::conversations_load,
            conversations::conversations_save,
            conversations::conversations_delete,
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
