// clippy 风格类 lint 的 crate 级 allow。
// 这些是"风格偏好"而非"潜在 bug"。只 allow 这些具体 lint,不 allow clippy::all。
#![allow(
    clippy::doc_lazy_continuation,
    clippy::empty_line_after_doc_comments,
    // 以下为代码风格偏好,逐个修收益低;设计上已确认这些写法可接受
    clippy::while_let_loop,            // for + if let Ok 模式,业务逻辑需保留迭代器语义
    clippy::too_many_arguments,        // write_card_impl 等业务函数参数多,拆 context struct 收益不抵风险
    clippy::enum_variant_names,        // WorklogCardSource 的 *Task 后缀是语义清晰的命名约定
    clippy::missing_transmute_annotations,  // memory/db.rs 的 sqlite-vec 扩展注册,unsafe 已有注释论证
    clippy::manual_map,                // 个别 filter_map 写法,改 map 降可读性
    clippy::unnecessary_unwrap,
)]

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
mod logging;
mod mcp_client;
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
    // 日志必须最先初始化 —— 后续所有 plugin spawn / 业务逻辑都依赖 tracing 已就绪。
    // WorkerGuard 必须保活到进程结束(appender drop 时 flush 尾部日志);
    // forget 等同 leak,生命周期等于进程,可接受(全局单例,不应多个)。
    if let Some(guard) = logging::init_logging() {
        std::mem::forget(guard);
    }

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
                        tracing::error!(target: "channels", "自动启动失败: {e}");
                    }
                });
            }

            // 存储 AppHandle 供 deploy 轮询任务 emit 事件到前端。
            tools::deploy::init_app_handle(app.handle().clone());

            // 启动全局 MCP client 管理器：读 ~/.jarvis/mcp-servers.json，spawn 所有 enabled
            // 的 stdio MCP server。没配文件 → Ok([])，正常启动（无 MCP server）。单个 server
            // 起不来不阻断 app（spawn_all_from_config 内部已逐个打日志）。
            tauri::async_runtime::spawn(async move {
                match crate::mcp_client::manager().spawn_all_from_config().await {
                    Ok(started) if !started.is_empty() => {
                        tracing::info!(target: "mcp_client", "已启动 MCP server: {started:?}");
                    }
                    Ok(_) => {}
                    Err(e) => tracing::error!(target: "mcp_client", "启动 MCP server 失败: {e}"),
                }
            });
            // 语音功能已下线：一次性清理遗留的本地模型目录(~/.jarvis/voice/)与云端 token。
            // 幂等——目录/钥匙串条目不存在则跳过，清理过的老用户后续启动直接 noop。
            let voice_dir = crate::settings::jarvis_dir().join("voice");
            if voice_dir.exists() {
                match std::fs::remove_dir_all(&voice_dir) {
                    Ok(_) => tracing::info!(target: "cleanup", "已清理废弃语音目录 {}", voice_dir.display()),
                    Err(e) => tracing::warn!(target: "cleanup", "清理废弃语音目录失败: {e}"),
                }
            }
            let _ = crate::settings::secret_clear("voice.cloud.volcAccessToken");

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
            commands::custom_pet_list,
            commands::custom_pet_save,
            commands::custom_pet_delete,
            commands::deploy_config_get,
            commands::deploy_config_save,
            commands::deploy_test_connection,
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
            commands::export_diagnostic_logs,
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
