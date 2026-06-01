// Tool 实现 + dispatch。
//
// 这里收纳每个原 TS tool 的 Rust 版本，对应 src/tools/*.ts。dispatch 入口在
// commands::tool_execute，按 name 字段路由到本文件的具体函数。
//
// 已迁工具：
//   get_tasks         → zentao.get_my_tasks
//   get_today_tasks   → zentao.get_my_tasks + 今日截止过滤
//   get_task_detail   → zentao.get_task
//   get_task_commits  → commit_link::link_tasks_with_commits
//   get_daily_review  → daily_review::build_daily_review (+ 可选 LLM 改写)
//   analyze_risk      → zentao.get_my_tasks + heuristic (+ 可选 LLM 改写)
//   log-task-effort   → zentao.add_effort（带审计日志）
//   prepare-log-task-effort → 生成待确认写工时建议（不写入）
//   ask-llm           → llm.chat（直接转发）
//   cc_switch_import  → 读 ~/.cc-switch/{settings.json,cc-switch.db}
//   chat_send         → chat_agent::run_agent
//
// dispatch() 作为统一入口，供 commands::tool_execute 和 chat_agent loop 调用。

#![allow(dead_code)]

pub mod cc_switch_import;
pub mod chat_tool;
pub mod daily_review_tool;
pub mod effort_logging;
pub mod effort_report;
pub mod llm_passthrough;
pub mod risk_analysis;
pub mod task_commits;
pub mod task_queries;

use serde_json::Value;

// Re-export: commands.rs calls tools::get_tasks directly.
pub use task_queries::get_tasks;

// ============================================================================
// dispatch：所有 tool 的统一入口
// ============================================================================

pub async fn dispatch(name: &str, input: Value) -> Result<Value, String> {
    match name {
        "get_tasks" => self::task_queries::get_tasks(input).await,
        "get_today_tasks" => self::task_queries::get_today_tasks(input).await,
        "get_task_detail" => self::task_queries::get_task_detail(input).await,
        "get_task_commits" => self::task_commits::get_task_commits(input).await,
        "get_daily_review" => self::daily_review_tool::get_daily_review(input).await,
        "get_classified_tasks" => self::task_queries::get_classified_tasks(input).await,
        "get_efforts" => self::effort_report::get_efforts(input).await,
        "get_effort_report" => self::effort_report::get_effort_report(input).await,
        "analyze_risk" => self::risk_analysis::analyze_risk(input).await,
        "prepare-log-task-effort" => self::effort_logging::prepare_log_task_effort(input).await,
        "log-task-effort" => self::effort_logging::log_task_effort(input).await,
        "ask-llm" => self::llm_passthrough::ask_llm(input).await,
        "cc_switch_import" => self::cc_switch_import::cc_switch_import(input).await,
        // chat_send → run_agent → dispatch（递归调其它 tool）。Box::pin 打破
        // async fn 静态递归类型，否则 rustc 报"recursive async fn requires indirection"。
        // agent 自身不会再叫 chat_send（不在 DEFAULT_AGENT_TOOLS），这层 Pin 只为通过编译。
        "chat_send" => Box::pin(self::chat_tool::chat_send(input)).await,
        _ => Err(format!("未知工具: {}", name)),
    }
}
