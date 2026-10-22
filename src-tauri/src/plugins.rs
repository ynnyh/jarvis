//! 声明式插件清单层（方案 C′，L2 插件基座）。
//!
//! 插件 = 一个 `plugin.json`：触发器（cron / 每日时间窗 / 应用事件）→ 动作
//! （气泡 / LLM 一句话 / 系统通知 / 渠道推送 / 调工具）。纯声明、零沙箱、
//! 不可执行任意代码；行为解释在前端引擎（usePluginHost.ts），本模块只负责：
//!   1. 清单 serde 模型（manifest 字段 camelCase、trigger/action 的 type 标签 snake_case）
//!   2. 结构校验（坏 JSON / 缺字段 / 未知 type / 非法 state、button 动作 → invalid + 原因）
//!   3. 内置清单嵌入（include_str!，id 自带 builtin- 前缀，只读）与用户目录扫描
//!   4. disabledPlugins 过滤/合并的纯函数（enabled 状态存 config，manifest 只读）
//!
//! cron 表达式的合法性不在这里校验——由前端 croner 在执行时校验（坏表达式跳过
//! 并 log，与 useScheduledReminders 同策略），Rust 侧只做结构校验。

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ===== 清单模型 =====

/// 触发器。type 标签 snake_case（与 plugin.json 文档一致），字段 camelCase。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PluginTrigger {
    /// 标准 5 段 cron；表达式合法性由前端 croner 执行时校验
    Cron {
        expr: String,
        /// 该触发器命中时执行的动作（缺省用插件级 actions）
        #[serde(default)]
        actions: Vec<PluginAction>,
    },
    /// 每日时间窗 [startMin, endMin)，分钟数 0-1439；每自然日最多触发一次
    #[serde(rename_all = "camelCase")]
    DailyWindow {
        start_min: u32,
        end_min: u32,
        /// 去重键：同插件多窗口时区分（缺省按下标）
        #[serde(default)]
        key: Option<String>,
        #[serde(default)]
        actions: Vec<PluginAction>,
    },
    /// 应用事件（app-event 广播，如 effort_written / deploy_done）；每次触发都执行
    Event {
        on: String,
        #[serde(default)]
        actions: Vec<PluginAction>,
    },
}

/// 气泡按钮：只允许白名单命名动作（安全考虑，不透传任意代码/路径）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginButton {
    pub label: String,
    pub action: String,
}

/// 动作。顺序执行；单条失败静默降级，不影响后续动作。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PluginAction {
    #[serde(rename_all = "camelCase")]
    Bubble {
        text: String,
        #[serde(default)]
        emoji: Option<String>,
        /// 现有 8 态之一（校验见 validate_manifest）
        #[serde(default)]
        state: Option<String>,
        /// 秒；0 = 持久气泡。缺省 12
        #[serde(default)]
        duration_sec: Option<u64>,
        #[serde(default)]
        buttons: Vec<PluginButton>,
    },
    #[serde(rename_all = "camelCase")]
    LlmLine {
        /// pet_chatter 的 kind；"custom" 表示用 instruction 当生成指令
        kind: String,
        /// LLM 失败/未配置/关闭时的固定文案兜底（会先弹，LLM 回来原位替换）
        fallback: String,
        /// kind=custom 时的生成指令（透传给 pet_chatter 的 context.instruction）
        #[serde(default)]
        instruction: Option<String>,
        #[serde(default)]
        emoji: Option<String>,
        #[serde(default)]
        state: Option<String>,
        #[serde(default)]
        duration_sec: Option<u64>,
    },
    #[serde(rename_all = "camelCase")]
    Notify { title: Option<String>, body: String },
    #[serde(rename_all = "camelCase")]
    Channel { text: String },
    /// 调工具。走 plugin_tool_execute 安全门禁：写操作工具与 confirm/blocked 级
    /// MCP 工具直接拒绝（插件只能调安全级工具），失败静默 log。
    #[serde(rename_all = "camelCase")]
    Tool {
        tool: String,
        #[serde(default)]
        args: Value,
    },
}

/// plugin.json 清单。字段 camelCase（serde 全局 rename_all）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_version")]
    pub version: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub triggers: Vec<PluginTrigger>,
    /// 插件级动作：触发器未自带 actions 时执行
    #[serde(default)]
    pub actions: Vec<PluginAction>,
    /// 与上次实际执行的最小间隔（秒），防事件触发风暴。缺省不限。
    #[serde(default)]
    pub min_interval_sec: Option<u64>,
}

fn default_version() -> String {
    "1.0.0".to_string()
}

// ===== 校验 =====

/// bubble.state 允许的 8 态（与前端 stateMap 一致）
pub const BUBBLE_STATES: &[&str] = &[
    "idle", "thinking", "working", "warning", "happy", "morning", "coffee", "late",
];

/// bubble.buttons[].action 白名单（前端映射到既有菜单函数）
pub const BUTTON_ACTIONS: &[&str] = &[
    "open_today_plan",
    "open_manual_hours",
    "open_review",
    "open_tasks",
    "dismiss",
];

/// 解析并校验清单 JSON。任何失败返回人话原因（设置页红字展示），不影响其他插件。
pub fn validate_manifest(raw: &str) -> Result<PluginManifest, String> {
    let m: PluginManifest =
        serde_json::from_str(raw).map_err(|e| format!("清单解析失败: {e}"))?;
    validate(m)
}

/// 对已反序列化的清单做语义校验（serde 只管结构，这里管取值范围）
pub fn validate(m: PluginManifest) -> Result<PluginManifest, String> {
    let id = m.id.trim();
    if id.is_empty() {
        return Err("缺少 id".to_string());
    }
    if id != m.id || m.id.contains('/') || m.id.contains('\\') {
        return Err("id 不能含空白首尾或路径分隔符".to_string());
    }
    if m.name.trim().is_empty() {
        return Err("缺少 name".to_string());
    }
    if m.triggers.is_empty() {
        return Err("缺少 triggers（至少一个触发器）".to_string());
    }
    for t in &m.triggers {
        match t {
            PluginTrigger::Cron { expr, .. } => {
                if expr.trim().is_empty() {
                    return Err("cron 触发器缺少 expr".to_string());
                }
            }
            PluginTrigger::DailyWindow { start_min, end_min, .. } => {
                if *start_min >= *end_min || *end_min > 24 * 60 {
                    return Err(format!(
                        "daily_window 时间窗非法（{start_min}-{end_min}，需 startMin < endMin ≤ 1440）"
                    ));
                }
            }
            PluginTrigger::Event { on, .. } => {
                if on.trim().is_empty() {
                    return Err("event 触发器缺少 on（事件名）".to_string());
                }
            }
        }
    }
    for act in all_actions(&m) {
        check_action(act)?;
    }
    Ok(m)
}

/// 触发器级 actions + 插件级 actions 的并集（校验用；执行顺序由前端定）
fn all_actions(m: &PluginManifest) -> impl Iterator<Item = &PluginAction> {
    m.actions
        .iter()
        .chain(m.triggers.iter().flat_map(|t| match t {
            PluginTrigger::Cron { actions, .. } => actions,
            PluginTrigger::DailyWindow { actions, .. } => actions,
            PluginTrigger::Event { actions, .. } => actions,
        }))
}

fn check_action(act: &PluginAction) -> Result<(), String> {
    let bad_state = |s: &Option<String>| -> Result<(), String> {
        if let Some(s) = s {
            if !BUBBLE_STATES.contains(&s.as_str()) {
                return Err(format!(
                    "state「{s}」非法，可选：{}",
                    BUBBLE_STATES.join("/")
                ));
            }
        }
        Ok(())
    };
    match act {
        PluginAction::Bubble { state, buttons, .. } => {
            bad_state(state)?;
            for b in buttons {
                if b.label.trim().is_empty() {
                    return Err("bubble 按钮 label 不能为空".to_string());
                }
                if !BUTTON_ACTIONS.contains(&b.action.as_str()) {
                    return Err(format!(
                        "按钮动作「{}」非法，可选：{}",
                        b.action,
                        BUTTON_ACTIONS.join("/")
                    ));
                }
            }
            Ok(())
        }
        PluginAction::LlmLine { kind, fallback, instruction, state, .. } => {
            if kind.trim().is_empty() {
                return Err("llm_line 缺少 kind".to_string());
            }
            if fallback.trim().is_empty() {
                return Err("llm_line 缺少 fallback（固定文案兜底）".to_string());
            }
            bad_state(state)?;
            if kind == "custom" && instruction.as_deref().map(|s| s.trim().is_empty()).unwrap_or(true) {
                return Err("llm_line kind=custom 需要提供 instruction（生成指令）".to_string());
            }
            Ok(())
        }
        PluginAction::Notify { body, .. } => {
            if body.trim().is_empty() {
                return Err("notify 缺少 body".to_string());
            }
            Ok(())
        }
        PluginAction::Channel { text } => {
            if text.trim().is_empty() {
                return Err("channel 缺少 text".to_string());
            }
            Ok(())
        }
        PluginAction::Tool { tool, .. } => {
            if tool.trim().is_empty() {
                return Err("tool 缺少 tool（工具名）".to_string());
            }
            Ok(())
        }
    }
}

// ===== 触发器摘要（设置页展示，Rust 拼好字符串） =====

/// 分钟数 → HH:MM（450 → "07:30"）
pub fn fmt_min(min: u32) -> String {
    format!("{:02}:{:02}", min / 60, min % 60)
}

pub fn trigger_summary(t: &PluginTrigger) -> String {
    match t {
        PluginTrigger::Cron { expr, .. } => format!("cron {expr}"),
        PluginTrigger::DailyWindow { start_min, end_min, .. } => {
            format!("窗口 {}-{}", fmt_min(*start_min), fmt_min(*end_min))
        }
        PluginTrigger::Event { on, .. } => format!("事件 {on}"),
    }
}

pub fn trigger_summaries(m: &PluginManifest) -> Vec<String> {
    m.triggers.iter().map(trigger_summary).collect()
}

// ===== disabledPlugins 过滤/合并（纯函数） =====

/// 从 config JSON 里取 disabledPlugins（容忍缺失/类型不对，老配置不炸）
pub fn parse_disabled_list(config: &Value) -> Vec<String> {
    config
        .get("disabledPlugins")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

/// 按开关语义合并 disabledPlugins：enabled → 移除；禁用 → 去重后追加。
pub fn apply_enabled_change(list: &[String], id: &str, enabled: bool) -> Vec<String> {
    let mut out: Vec<String> = list.to_vec();
    if enabled {
        out.retain(|x| x != id);
    } else if !out.iter().any(|x| x == id) {
        out.push(id.to_string());
    }
    out
}

// ===== 内置插件 + 用户目录 =====

/// 内置清单：id 写在 JSON 里（builtin- 前缀），include_str! 嵌入二进制，只读。
pub const BUILTIN_MANIFESTS: &[&str] = &[
    include_str!("builtin_plugins/greetings.json"),
];

/// 用户插件目录 ~/.jarvis/plugins/（每个插件一个子目录，含 plugin.json）
pub fn plugins_dir() -> std::path::PathBuf {
    crate::settings::jarvis_dir().join("plugins")
}

/// 首次打开插件目录时写入的示例（喝水提醒），可直接改 manifest 后 reload 生效
pub const EXAMPLE_MANIFEST: &str = r#"{
  "id": "example-drink-water",
  "name": "喝水提醒",
  "description": "工作日上班时间每 45 分钟提醒喝口水（示例插件，可随意修改）",
  "version": "1.0.0",
  "author": "you",
  "triggers": [
    { "type": "cron", "expr": "*/45 9-18 * * 1-5" }
  ],
  "actions": [
    { "type": "bubble", "text": "{userTitle}，喝口水休息一下吧", "emoji": "💧", "state": "happy", "durationSec": 12 }
  ]
}
"#;

/// 单个插件的展示信息（plugin_list 返回）。manifest 带全量结构给前端引擎，
/// triggerSummaries 是设置页展示用的人话摘要。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    /// builtin | user
    pub source: &'static str,
    pub enabled: bool,
    pub trigger_summaries: Vec<String>,
    /// 校验失败原因；None = 有效
    pub invalid_reason: Option<String>,
    /// 全量清单（invalid 时为 None）
    pub manifest: Option<PluginManifest>,
}

/// 扫描用户插件目录（~/.jarvis/plugins/<子目录>/plugin.json）。
/// 目录不存在 → 空；单个清单坏 → 以目录名为 id 标 invalid，不影响其他插件。
pub fn scan_user_plugins() -> Vec<(String, Result<PluginManifest, String>)> {
    let dir = plugins_dir();
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return out;
    };
    let mut paths: Vec<std::path::PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    paths.sort();
    for p in paths {
        let dir_name = p
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let manifest_path = p.join("plugin.json");
        let parsed = match std::fs::read_to_string(&manifest_path) {
            Ok(raw) => validate_manifest(&raw),
            Err(e) => Err(format!("读取 {} 失败: {e}", manifest_path.display())),
        };
        out.push((dir_name, parsed));
    }
    out
}

/// 汇总内置 + 用户插件为 PluginInfo 列表（builtin 在前，各自按 id 排序）
pub fn collect_plugin_infos(disabled: &[String]) -> Vec<PluginInfo> {
    let mut out = Vec::new();

    for raw in BUILTIN_MANIFESTS {
        out.push(match validate_manifest(raw) {
            Ok(m) => valid_info(m, "builtin", disabled),
            Err(reason) => {
                // 内置清单损坏属发版事故（BUILTIN_MANIFESTS 有测试兜着），
                // 兜底展示原因而不是 panic，保住设置页其他插件。
                let name = serde_json::from_str::<Value>(raw)
                    .ok()
                    .and_then(|v| v.get("name").and_then(|x| x.as_str()).map(String::from))
                    .unwrap_or_else(|| "内置插件".to_string());
                invalid_info("<builtin>", &name, "builtin", reason)
            }
        });
    }

    for (dir_name, parsed) in scan_user_plugins() {
        out.push(match parsed {
            Ok(m) => valid_info(m, "user", disabled),
            Err(reason) => invalid_info(&dir_name, &format!("{dir_name}（invalid）"), "user", reason),
        });
    }
    out
}

fn valid_info(m: PluginManifest, source: &'static str, disabled: &[String]) -> PluginInfo {
    PluginInfo {
        enabled: !disabled.contains(&m.id),
        trigger_summaries: trigger_summaries(&m),
        invalid_reason: None,
        id: m.id.clone(),
        name: m.name.clone(),
        description: m.description.clone(),
        version: m.version.clone(),
        source,
        manifest: Some(m),
    }
}

fn invalid_info(id: &str, name: &str, source: &'static str, reason: String) -> PluginInfo {
    PluginInfo {
        id: id.to_string(),
        name: name.to_string(),
        description: String::new(),
        version: String::new(),
        source,
        enabled: false,
        trigger_summaries: Vec::new(),
        invalid_reason: Some(reason),
        manifest: None,
    }
}

// ===== 测试 =====
// 本机 cargo test 因存量环境问题无法运行，测试留给 CI；本机用
// cargo clippy --all-targets 保证编译通过。

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = r#"{
        "id": "standup-reminder",
        "name": "久坐提醒",
        "description": "工作时间每 45 分钟提醒起身活动",
        "version": "1.0.0",
        "author": "you",
        "triggers": [
            { "type": "cron", "expr": "*/45 9-18 * * 1-5" }
        ],
        "actions": [
            { "type": "bubble", "text": "{userTitle}，起来动一动！", "emoji": "🧘", "state": "happy", "durationSec": 12,
              "buttons": [ { "label": "歇会儿", "action": "dismiss" } ] },
            { "type": "llm_line", "kind": "custom", "instruction": "提醒用户起身活动，语气关心", "fallback": "活动一下吧" },
            { "type": "notify", "title": "Jarvis", "body": "该活动了" },
            { "type": "channel", "text": "⏰ 久坐提醒" },
            { "type": "tool", "tool": "get_tasks", "args": {} }
        ],
        "minIntervalSec": 300
    }"#;

    #[test]
    fn valid_manifest_parses_with_camel_case_fields() {
        let m = validate_manifest(VALID).expect("合法清单应通过校验");
        assert_eq!(m.id, "standup-reminder");
        assert_eq!(m.min_interval_sec, Some(300));
        match &m.triggers[0] {
            PluginTrigger::Cron { expr, .. } => assert_eq!(expr, "*/45 9-18 * * 1-5"),
            other => panic!("应为 cron 触发器，实际 {other:?}"),
        }
    }

    #[test]
    fn daily_window_parses_snake_case_type_and_camel_case_fields() {
        let m = validate_manifest(
            r#"{ "id": "x", "name": "X", "triggers": [ { "type": "daily_window", "startMin": 450, "endMin": 540 } ] }"#,
        )
        .expect("daily_window 应解析");
        match &m.triggers[0] {
            PluginTrigger::DailyWindow { start_min, end_min, key, .. } => {
                assert_eq!((*start_min, *end_min), (450, 540));
                assert!(key.is_none());
            }
            other => panic!("应为 daily_window，实际 {other:?}"),
        }
    }

    #[test]
    fn bad_json_is_invalid() {
        assert!(validate_manifest("{ not json").is_err());
    }

    #[test]
    fn missing_id_or_name_is_invalid() {
        assert!(validate_manifest(r#"{ "name": "X", "triggers": [] }"#).is_err());
        assert!(validate_manifest(r#"{ "id": "x", "triggers": [] }"#).is_err());
    }

    #[test]
    fn unknown_trigger_or_action_type_is_invalid() {
        let err = validate_manifest(
            r#"{ "id": "x", "name": "X", "triggers": [ { "type": "magic" } ] }"#,
        )
        .unwrap_err();
        assert!(err.contains("解析失败"), "未知 type 应报解析错误: {err}");
        assert!(validate_manifest(
            r#"{ "id": "x", "name": "X", "triggers": [ { "type": "event", "on": "e" } ], "actions": [ { "type": "dance" } ] }"#,
        )
        .is_err());
    }

    #[test]
    fn empty_triggers_is_invalid() {
        assert!(validate_manifest(r#"{ "id": "x", "name": "X", "triggers": [] }"#).is_err());
    }

    #[test]
    fn invalid_bubble_state_is_rejected() {
        let err = validate_manifest(
            r#"{ "id": "x", "name": "X", "triggers": [ { "type": "event", "on": "e" } ],
                 "actions": [ { "type": "bubble", "text": "hi", "state": "dancing" } ] }"#,
        )
        .unwrap_err();
        assert!(err.contains("dancing"), "应指出非法 state: {err}");
    }

    #[test]
    fn invalid_button_action_is_rejected() {
        let err = validate_manifest(
            r#"{ "id": "x", "name": "X", "triggers": [ { "type": "event", "on": "e" } ],
                 "actions": [ { "type": "bubble", "text": "hi", "buttons": [ { "label": "走", "action": "rm_rf" } ] } ] }"#,
        )
        .unwrap_err();
        assert!(err.contains("rm_rf"), "应指出非法按钮动作: {err}");
    }

    #[test]
    fn custom_llm_line_requires_instruction() {
        assert!(validate_manifest(
            r#"{ "id": "x", "name": "X", "triggers": [ { "type": "event", "on": "e" } ],
                 "actions": [ { "type": "llm_line", "kind": "custom", "fallback": "好" } ] }"#,
        )
        .is_err());
        assert!(validate_manifest(
            r#"{ "id": "x", "name": "X", "triggers": [ { "type": "event", "on": "e" } ],
                 "actions": [ { "type": "llm_line", "kind": "custom", "instruction": "说句话", "fallback": "好" } ] }"#,
        )
        .is_ok());
    }

    #[test]
    fn trigger_summaries_format() {
        let m = validate_manifest(
            r#"{ "id": "x", "name": "X", "triggers": [
                 { "type": "cron", "expr": "*/45 9-18 * * 1-5" },
                 { "type": "daily_window", "startMin": 450, "endMin": 540 },
                 { "type": "event", "on": "effort_written" } ] }"#,
        )
        .unwrap();
        assert_eq!(trigger_summaries(&m), vec![
            "cron */45 9-18 * * 1-5".to_string(),
            "窗口 07:30-09:00".to_string(),
            "事件 effort_written".to_string(),
        ]);
    }

    #[test]
    fn parse_disabled_list_tolerates_missing_and_non_strings() {
        assert!(parse_disabled_list(&serde_json::json!({})).is_empty());
        assert_eq!(
            parse_disabled_list(&serde_json::json!({ "disabledPlugins": ["a", 1, null, "b"] })),
            vec!["a".to_string(), "b".to_string()]
        );
    }

    #[test]
    fn apply_enabled_change_adds_once_and_removes() {
        let list = vec!["a".to_string()];
        // 禁用：追加且去重
        assert_eq!(
            apply_enabled_change(&list, "b", false),
            vec!["a".to_string(), "b".to_string()]
        );
        assert_eq!(
            apply_enabled_change(&apply_enabled_change(&list, "b", false), "b", false),
            vec!["a".to_string(), "b".to_string()],
            "重复禁用不产生重复项"
        );
        // 启用：移除（不存在的 id 也无害）
        assert_eq!(apply_enabled_change(&list, "b", true), list);
        assert_eq!(apply_enabled_change(&["a".to_string(), "b".to_string()], "a", true), vec!["b".to_string()]);
    }

    #[test]
    fn builtin_manifests_all_valid() {
        assert!(!BUILTIN_MANIFESTS.is_empty());
        for raw in BUILTIN_MANIFESTS {
            let m = validate_manifest(raw)
                .unwrap_or_else(|e| panic!("内置清单应为合法: {e}\n{raw}"));
            assert!(m.id.starts_with("builtin-"), "内置插件 id 必须 builtin- 前缀: {}", m.id);
        }
    }

    #[test]
    fn example_manifest_is_valid() {
        validate_manifest(EXAMPLE_MANIFEST).expect("示例清单应为合法");
    }

    #[test]
    fn fmt_min_pads() {
        assert_eq!(fmt_min(450), "07:30");
        assert_eq!(fmt_min(1410), "23:30");
    }
}
