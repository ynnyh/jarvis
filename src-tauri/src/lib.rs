mod chat_agent;
mod commands;
mod commit_classifier;
mod commit_link;
mod conversations;
mod credentials;
mod daily_review;
mod git_scan;
mod llm;
mod repo_recommender;
mod settings;
mod settings_extras;
mod task_bindings;
mod task_snapshot;
mod tools;
mod zentao;

use tauri::Manager;
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

                // 屏幕选择优先 primary（系统设置里用户标的"主显示器"）→ current → 第一个可用。
                // 历史教训 1（已废）：用 current 优先，结果在「外接 1920 设为主屏 + mac 笔记本副屏」
                // 场景下，启动时窗口默认在 mac 自己屏上，current 拿到的就是 mac，定位完小人就跑
                // 副屏去了，用户看不见。
                // 历史教训 2：早期用 primary 优先，在「笔记本作为 primary + 用户在看外接」场景下也
                // 把窗口丢到笔记本屏。但那种场景下用户应该把外接设为主屏 —— 现在尊重用户的主屏选择。
                let monitor = window.primary_monitor().ok().flatten()
                    .or_else(|| window.current_monitor().ok().flatten())
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

                // 显式把 ignoreCursorEvents 钉死成 false —— macOS 上 transparent + macOSPrivateApi
                // 的窗口默认状态不保证，前端 useCursorPassthrough 假设初始是 false 后做早返回
                // 优化，若 OS 实际是 true，前端永远叫不动它，整个窗口被穿透，用户看到的就是
                // 无法拖动、无法点击、左右键都不行。在这里钉死避免该不变量被破坏。
                let _ = window.set_ignore_cursor_events(false);
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
            settings_extras::pick_directory,
            settings_extras::excluded_business_lines_load,
            settings_extras::excluded_business_lines_save,
            conversations::conversations_list,
            conversations::conversations_load,
            conversations::conversations_save,
            conversations::conversations_delete,
            task_bindings::task_bindings_load,
            task_bindings::task_bindings_get,
            task_bindings::task_bindings_set,
            task_bindings::task_bindings_delete,
            repo_recommender::recommend_repos_for_task,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app_handle, _event| {});
}
