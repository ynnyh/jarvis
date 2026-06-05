mod channels;
mod chat_agent;
mod commands;
mod commit_classifier;
mod commit_link;
mod conversations;
mod cost_rates;
mod credentials;
mod daily_review;
mod fine_report;
mod git_scan;
mod llm;
mod memory;
mod repo_recommender;
mod settings;
mod settings_extras;
mod task_bindings;
mod task_snapshot;
mod tools;
mod util;
mod worklog;
mod zentao;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::Emitter;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .manage(commands::WriteHoursState::default())
        .manage(channels::ChannelServiceState::default())
        .manage(memory::MemoryState::new(&memory::default_db_path()))
        .setup(|app| {
            // ===== 系统托盘 =====
            let show_i = MenuItem::with_id(app, "tray_show", "显示小人", true, None::<&str>)?;
            let hide_i = MenuItem::with_id(app, "tray_hide", "隐藏小人", true, None::<&str>)?;
            let sep = PredefinedMenuItem::separator(app)?;
            let quit_i = MenuItem::with_id(app, "tray_quit", "退出 Jarvis", true, None::<&str>)?;
            let tray_menu = Menu::with_items(app, &[&show_i, &hide_i, &sep, &quit_i])?;

            let _tray = TrayIconBuilder::with_id("main")
                .icon(
                    app.default_window_icon()
                        .cloned()
                        .expect("missing tray icon"),
                )
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
                let monitor = window
                    .primary_monitor()
                    .ok()
                    .flatten()
                    .or_else(|| window.current_monitor().ok().flatten())
                    .or_else(|| {
                        window
                            .available_monitors()
                            .ok()
                            .and_then(|m| m.first().cloned())
                    });

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

                    let _ = window
                        .set_position(tauri::Position::Logical(tauri::LogicalPosition { x, y }));
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

            // 对非 avatar 子窗口挂 CloseRequested：prevent_close + hide + show avatar，
            // 绕开 JS 端 onCloseRequested 的异步竞态（preventDefault 在异步分支里可能被 Tauri
            // 当作"未拦截"导致窗口销毁 → avatar 跟着失踪）。
            // extra 闭包跑窗口专属清理，如清空 state payload 或发事件通知前端。
            fn setup_close_requested(
                win: &tauri::WebviewWindow,
                app_handle: &tauri::AppHandle,
                extra: impl Fn() + Send + 'static,
            ) {
                let w = win.clone();
                let h = app_handle.clone();
                win.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = w.hide();
                        extra();
                        if let Some(avatar) = h.get_webview_window("avatar") {
                            avatar.unminimize().ok();
                            let _ = avatar.show();
                            avatar.set_focus().ok();
                        }
                    }
                });
            }

            if let Some(wh) = app.get_webview_window("writeHours") {
                let ah = app.handle().clone();
                setup_close_requested(&wh, &ah, {
                    let ah = ah.clone();
                    move || {
                        if let Some(state) = ah.try_state::<commands::WriteHoursState>() {
                            if let Ok(mut slot) = state.payload.lock() {
                                *slot = None;
                            }
                        }
                    }
                });
            }

            if let Some(mh) = app.get_webview_window("manualHours") {
                let app_handle = app.handle().clone();
                setup_close_requested(&mh, &app_handle, || {});
            }

            if let Some(tp) = app.get_webview_window("todayPlan") {
                let ah = app.handle().clone();
                setup_close_requested(&tp, &ah, {
                    let ah = ah.clone();
                    move || {
                        let _ = ah.emit_to("avatar", "today-plan-window-closed", ());
                    }
                });
            }

            if let Some(bw) = app.get_webview_window("batchWrite") {
                setup_close_requested(&bw, app.handle(), || {});
            }

            if let Some(cw) = app.get_webview_window("cost") {
                setup_close_requested(&cw, app.handle(), || {});
            }

            if channels::should_auto_start() {
                let app_handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = channels::start_gateway_background(app_handle) {
                        eprintln!("[channels] 自动启动失败: {}", e);
                    }
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::drag_window,
            commands::cursor_pos_in_window,
            commands::tool_execute,
            commands::chat_send_stream,
            commands::summarize_work_content,
            commands::get_proactive_reminders,
            commands::fetch_task_alerts,
            commands::list_projects,
            commands::open_zentao_task,
            commands::quit_app,
            commands::toggle_avatar_window,
            commands::config_load,
            commands::config_save,
            commands::llm_profile_save,
            commands::llm_profile_switch,
            commands::llm_profile_delete,
            commands::llm_profile_upsert,
            commands::llm_profile_test,
            commands::cc_switch_list_providers,
            commands::cc_switch_import_provider,
            commands::chat_open,
            commands::chat_close,
            commands::settings_open,
            commands::settings_close,
            commands::today_plan_open,
            commands::today_plan_close,
            commands::write_hours_open,
            commands::write_hours_close,
            commands::write_hours_take_payload,
            commands::avatar_show_fallback,
            commands::manual_hours_open,
            commands::manual_hours_close,
            commands::batch_write_open,
            commands::batch_write_close,
            commands::check_dirty_repos,
            commands::cost_open,
            commands::cost_close,
            channels::channels_start,
            channels::channels_stop,
            channels::channel_status,
            channels::channels_notify,
            channels::telegram_probe,
            channels::qqbot_probe,
            credentials::credentials_set,
            credentials::credentials_get,
            credentials::credentials_delete,
            credentials::zentao_test_connection,
            fine_report::credentials::finereport_credentials_set,
            fine_report::credentials::finereport_credentials_get,
            fine_report::credentials::finereport_credentials_delete,
            fine_report::commands::finereport_test_connection,
            fine_report::commands::finereport_get_efforts,
            cost_rates::cost_rates_load,
            cost_rates::cost_rates_save,
            cost_rates::cost_team_members,
            cost_rates::project_cost_summary,
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
            worklog::today_plan_load,
            worklog::today_plan_save,
            worklog::today_plan_clear,
            worklog::today_plan_lookup_task,
            worklog::worklog_session_get,
            worklog::worklog_card_update,
            worklog::worklog_manual_card_add,
            worklog::worklog_card_remove,
            worklog::worklog_card_write,
            worklog::worklog_session_write_confirmed,
            repo_recommender::recommend_repos_for_task,
        ])
        .build(tauri::generate_context!())?
        .run(|_app_handle, _event| {});
    Ok(())
}
