use super::*;
use crate::channels::types::ChannelsConfig;
use crate::channels::router::effort_shortcuts::{is_effort_query, effort_query_range};
use crate::channels::router::message_handler::should_use_agent_tools;
use crate::channels::router::reminders::parse_reminder_input;

#[test]
fn notification_targets_prefer_explicit_notify_ids() {
    let mut cfg = ChannelsConfig::default();
    cfg.telegram.enabled = true;
    cfg.telegram.allow_chat_ids = vec!["allow-tg".to_string()];
    cfg.telegram.notify_chat_ids = vec!["notify-tg".to_string()];
    cfg.qqbot.enabled = true;
    cfg.qqbot.allow_user_ids = vec!["allow-user".to_string()];
    cfg.qqbot.allow_group_ids = vec!["allow-group".to_string()];
    cfg.qqbot.notify_user_ids = vec!["notify-user".to_string()];
    cfg.qqbot.notify_group_ids = vec!["notify-group".to_string()];

    let targets = notification_targets(&cfg);
    let pairs: Vec<_> = targets
        .iter()
        .map(|t| (t.channel.as_str(), t.chat_id.as_str()))
        .collect();

    assert_eq!(
        pairs,
        vec![
            ("telegram", "notify-tg"),
            ("qqbot", "c2c:notify-user"),
            ("qqbot", "group:notify-group"),
        ]
    );
}

#[test]
fn notification_targets_fallback_to_allow_ids_for_old_configs() {
    let mut cfg = ChannelsConfig::default();
    cfg.telegram.enabled = true;
    cfg.telegram.allow_chat_ids = vec!["allow-tg".to_string()];
    cfg.qqbot.enabled = true;
    cfg.qqbot.allow_user_ids = vec!["allow-user".to_string()];
    cfg.qqbot.allow_group_ids = vec!["allow-group".to_string()];

    let targets = notification_targets(&cfg);
    let pairs: Vec<_> = targets
        .iter()
        .map(|t| (t.channel.as_str(), t.chat_id.as_str()))
        .collect();

    assert_eq!(
        pairs,
        vec![
            ("telegram", "allow-tg"),
            ("qqbot", "c2c:allow-user"),
            ("qqbot", "group:allow-group"),
        ]
    );
}

#[test]
fn should_use_agent_tools_matches_keywords() {
    assert!(should_use_agent_tools("看看禅道上有什么任务"));
    assert!(should_use_agent_tools("今天工时写了吗"));
    assert!(should_use_agent_tools("这个 BUG 怎么修"));
    assert!(should_use_agent_tools("commit 信息看一下"));
}

#[test]
fn should_use_agent_tools_rejects_plain_chat() {
    assert!(!should_use_agent_tools("你好呀"));
    assert!(!should_use_agent_tools("哈哈哈哈哈"));
    assert!(!should_use_agent_tools("晚上吃什么"));
}

#[test]
fn is_effort_query_detects_query_not_write() {
    assert!(is_effort_query("查看本周工时"));
    assert!(is_effort_query("工时统计"));
    assert!(is_effort_query("今天耗时明细"));
    assert!(is_effort_query("上周工时"));
}

#[test]
fn is_effort_query_rejects_write_intent() {
    assert!(!is_effort_query("写入工时"));
    assert!(!is_effort_query("记录耗时"));
    assert!(!is_effort_query("补填小时"));
}

#[test]
fn is_effort_query_rejects_no_effort_word() {
    assert!(!is_effort_query("查看任务"));
}

#[test]
fn effort_query_range_defaults_to_week() {
    let (range, label) = effort_query_range("查看工时");
    assert_eq!(range, "thisWeek");
    assert_eq!(label, "本周");
}

#[test]
fn effort_query_range_detects_yesterday() {
    let (range, _) = effort_query_range("昨天工时");
    assert_eq!(range, "yesterday");
}

#[test]
fn effort_query_range_detects_today() {
    let (range, _) = effort_query_range("今日工时明细");
    assert_eq!(range, "today");
}

#[test]
fn effort_query_range_detects_month() {
    let (range, _) = effort_query_range("本月工时汇总");
    assert_eq!(range, "thisMonth");
}

#[test]
fn effort_query_range_detects_quarter() {
    let (range, _) = effort_query_range("本季度工时汇总");
    assert_eq!(range, "thisQuarter");
}

#[test]
fn effort_query_range_detects_half_year() {
    let (range, _) = effort_query_range("近半年工时统计");
    assert_eq!(range, "last6Months");
}

#[test]
fn effort_query_range_detects_year() {
    let (range, _) = effort_query_range("今年工时统计");
    assert_eq!(range, "thisYear");
}

#[test]
fn effort_query_range_detects_last_week() {
    let (range, label) = effort_query_range("上周工时");
    assert_eq!(range, "lastWeek");
    assert_eq!(label, "上周");
}

#[test]
fn effort_query_range_detects_last_week_alt() {
    let (range, _) = effort_query_range("查看上一周耗时");
    assert_eq!(range, "lastWeek");
}

#[test]
fn parse_reminder_cron_format() {
    let (cron, msg) = parse_reminder_input("0 9 * * * 开会");
    assert_eq!(cron, "0 9 * * *");
    assert_eq!(msg, "开会");
}

#[test]
fn parse_reminder_hhmm_format() {
    let (cron, msg) = parse_reminder_input("9:30 喝水");
    assert_eq!(cron, "30 9 * * *");
    assert_eq!(msg, "喝水");
}

#[test]
fn parse_reminder_invalid_returns_empty() {
    let (cron, msg) = parse_reminder_input("随便说句话");
    assert_eq!(cron, "");
    assert_eq!(msg, "");
}

#[test]
fn parse_reminder_rejects_invalid_time() {
    let (cron, msg) = parse_reminder_input("25:00 测试");
    assert_eq!(cron, "");
    assert_eq!(msg, "");
}
