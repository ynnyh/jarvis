//! 插件基座命令层（方案 C′）。
//!
//! 五个对外命令 + 两个内部设施：
//!   - plugin_list / plugin_set_enabled / plugin_reload / plugins_dir_open：设置页管理
//!   - app_event：前端广播应用事件（插件的事件血管，养成等官方插件未来也吃它）
//!   - emit_app_event：Rust 内部同逻辑 helper（worklog / deploy 成功路径调用）
//!   - plugin_tool_execute：插件 tool 动作的专用入口，带安全门禁
//!
//! enabled 状态存 config.disabledPlugins（manifest 只读）；改动后走标准
//! config-changed 广播，各窗 store 自动 load()，无需插件专属同步代码。

use serde::Serialize;
use serde_json::{json, Value};
use tauri::Emitter;

use super::ToolResult;
use crate::plugins as manifest;

// ===== 应用事件 =====

/// app-event 事件的信封（serde camelCase；name/payload 本就无 snake_case 字段）
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AppEventEnvelope {
    name: String,
    payload: Value,
}

/// 广播应用事件到所有窗口（前端 `listen('app-event')` 消费）。
/// 事件血管：v1 有 effort_written / deploy_done / task_overdue / commits_scanned /
/// review_saved；插件（含未来的养成插件）按 event 触发器订阅。
pub(crate) fn emit_app_event(app: &tauri::AppHandle, name: &str, payload: Value) {
    let _ = app.emit("app-event", AppEventEnvelope {
        name: name.to_string(),
        payload,
    });
}

/// 前端主动广播应用事件。payload 可缺省（null）。
#[tauri::command]
pub fn app_event(name: String, payload: Option<Value>, app: tauri::AppHandle) {
    emit_app_event(&app, &name, payload.unwrap_or(Value::Null));
}

// ===== 插件管理 =====

#[tauri::command]
pub fn plugin_list() -> Result<Vec<manifest::PluginInfo>, String> {
    // load_raw_config 只读不迁移不 hydrate，适合这里的纯读取场景；
    // 文件不存在/坏 → 空配置，插件列表照常返回（全部 enabled）。
    let config = crate::settings::load_raw_config().unwrap_or_else(|| json!({}));
    let disabled = manifest::parse_disabled_list(&config);
    Ok(manifest::collect_plugin_infos(&disabled))
}

/// 开关插件：写 config.disabledPlugins（manifest 只读），广播 config-changed
/// 让所有窗口（含 avatar 的插件引擎）同步。builtin 同样可禁用。
#[tauri::command]
pub fn plugin_set_enabled(id: String, enabled: bool, app: tauri::AppHandle) -> Result<(), String> {
    let mut config = crate::settings::load_raw_config().unwrap_or_else(super::default_config);
    let list = manifest::parse_disabled_list(&config);
    let next = manifest::apply_enabled_change(&list, &id, enabled);
    config["disabledPlugins"] = json!(next);

    // 与 config_save 同款落盘路径：脱敏 + 写锁 + 原子写 + 全窗广播
    let dir = super::jarvis_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建配置目录失败: {e}"))?;
    let mut sanitized = config;
    super::strip_secrets_for_save(&mut sanitized)?;
    let content = serde_json::to_string_pretty(&sanitized)
        .map_err(|e| format!("配置序列化失败: {e}"))?;
    {
        let _guard = crate::settings::CONFIG_WRITE_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::util::write_atomic(&super::config_path(), &content)
            .map_err(|e| format!("写入配置失败: {e}"))?;
    }
    let _ = app.emit("config-changed", ());
    Ok(())
}

/// 重扫用户插件并广播 plugins-changed。Rust 侧不缓存清单（plugin_list 每次
/// 现读），这里的「重扫」就是让各窗引擎重新拉列表。
#[tauri::command]
pub fn plugin_reload(app: tauri::AppHandle) -> Result<(), String> {
    let _ = app.emit("plugins-changed", ());
    Ok(())
}

/// 用文件管理器打开插件目录；目录不存在则先创建并放入 example 喝水提醒样例，
/// 用户改 plugin.json 后点「重新加载」即可生效。
#[tauri::command]
pub fn plugins_dir_open() -> Result<String, String> {
    let dir = manifest::plugins_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建插件目录失败: {e}"))?;
    let example_dir = dir.join("example");
    if !example_dir.exists() {
        std::fs::create_dir_all(&example_dir)
            .map_err(|e| format!("创建 example 目录失败: {e}"))?;
        let manifest_path = example_dir.join("plugin.json");
        if !manifest_path.exists() {
            crate::util::write_atomic(&manifest_path, manifest::EXAMPLE_MANIFEST)
                .map_err(|e| format!("写入示例插件失败: {e}"))?;
        }
    }
    let path_text = dir.to_string_lossy().to_string();
    open_in_file_manager(&dir)?;
    Ok(path_text)
}

fn open_in_file_manager(path: &std::path::Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        // explorer 退出码不可靠（成功也可能非 0），只看能否 spawn
        super::silent_command("explorer")
            .arg(path)
            .spawn()
            .map_err(|e| format!("打开文件管理器失败: {e}"))?;
        Ok(())
    }
    #[cfg(target_os = "macos")]
    {
        super::silent_command("open")
            .arg(path)
            .spawn()
            .map_err(|e| format!("打开 Finder 失败: {e}"))?;
        Ok(())
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        super::silent_command("xdg-open")
            .arg(path)
            .spawn()
            .map_err(|e| format!("打开文件管理器失败: {e}"))?;
        Ok(())
    }
}

// ===== 插件调工具的安全门禁 =====

/// 插件 tool 动作的专用入口。PRD 红线「插件只能调安全级工具」在 Rust 侧强制：
///   - 写操作工具（log-task-effort / confirm-deploy）→ 拒绝。这些工具的既有
///     用户确认流都在前端窗口里（tool_execute 直连），插件没有确认 UI，
///     所以必须在这里拦，绝不能让插件清单静默触发发版/写工时。
///   - native 只读工具 → 限 DEFAULT_AGENT_TOOLS 白名单（与 agent loop 同一张表）。
///   - MCP 工具（mcp__ 前缀）→ 按动态安全分类，仅 auto 放行；confirm/blocked
///     拒绝（与 agent loop 的 mcp_gate_refusal 同语义，失败由引擎静默 log）。
/// 返回 ToolResult（success=false）而不是 Err，前端静默降级不弹错误框。
#[tauri::command]
pub async fn plugin_tool_execute(
    name: String,
    input: Option<Value>,
) -> Result<ToolResult, String> {
    use crate::chat_agent::{AGENT_FORBIDDEN_TOOLS, DEFAULT_AGENT_TOOLS};
    use crate::mcp_client::{self, ToolLevel};

    // 写操作红线：agent 不能直调的，插件更不能（插件连"用户在场确认"都没有）
    if AGENT_FORBIDDEN_TOOLS.contains(&name.as_str()) {
        tracing::info!(target: "plugins", "插件调用被拒绝（写操作工具）: {name}");
        return Ok(ToolResult {
            success: false,
            data: None,
            error: Some(format!("插件不允许调用写操作工具 {name}（插件只能调安全级工具）")),
        });
    }

    // MCP 工具：执行时重新定级（与 agent loop 同策略——门禁以调用时分类为准）
    if mcp_client::parse_namespaced_tool_name(&name).is_some() {
        let Some((server, tool)) = mcp_client::parse_namespaced_tool_name(&name) else {
            return Ok(ToolResult { success: false, data: None, error: Some(format!("无法解析 MCP 工具名 {name}")) });
        };
        let level = mcp_client::manager()
            .discover_for_agent()
            .await
            .into_iter()
            .find(|(s, t, _)| s == &server && t.name.as_ref() == tool)
            .map(|(_, _, lvl)| lvl);
        let refusal = match level {
            None => Some(format!("MCP 工具 {name} 当前不可用（server 未连接或工具不存在）")),
            // confirm/blocked 一律不执行；confirm 的用户确认流在聊天窗口里，插件场景无确认 UI
            Some(lvl) if lvl != ToolLevel::Auto => Some(format!(
                "MCP 工具 {name} 需要用户确认或已被禁用，插件不允许调用"
            )),
            Some(_) => None,
        };
        if let Some(err) = refusal {
            tracing::info!(target: "plugins", "插件调用被拒绝（MCP 门禁）: {name}");
            return Ok(ToolResult { success: false, data: None, error: Some(err) });
        }
        let arguments = match input {
            Some(Value::Object(map)) => Some(map),
            Some(Value::Null) | None => None,
            _ => return Ok(ToolResult { success: false, data: None, error: Some("MCP 工具参数必须是 JSON 对象".to_string()) }),
        };
        return match mcp_client::manager().call_tool(&server, &tool, arguments).await {
            Ok(result) => {
                let text = mcp_client::first_text(&result).unwrap_or_default();
                let ok = result.is_error != Some(true);
                Ok(ToolResult {
                    success: ok,
                    data: Some(json!({ "text": text })),
                    error: if ok { None } else { Some(if text.is_empty() { format!("MCP 工具 {name} 执行失败") } else { text }) },
                })
            }
            Err(e) => Ok(ToolResult { success: false, data: None, error: Some(e) }),
        };
    }

    // native 工具：只放行 agent 默认白名单（查询/建议类；写操作已在上面的红线拦掉）
    if !DEFAULT_AGENT_TOOLS.contains(&name.as_str()) {
        tracing::info!(target: "plugins", "插件调用被拒绝（不在安全工具白名单）: {name}");
        return Ok(ToolResult {
            success: false,
            data: None,
            error: Some(format!("插件只能调安全级工具，{name} 不在白名单中")),
        });
    }

    let input = input.unwrap_or(json!({}));
    match crate::tools::dispatch(&name, input).await {
        Ok(data) => Ok(ToolResult { success: true, data: Some(data), error: None }),
        Err(e) => Ok(ToolResult { success: false, data: None, error: Some(e) }),
    }
}
