use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

use crate::commit_link::{self, CommitLink, LinkCommitsOptions, TaskInput};
use crate::daily_review::{self, BuildOptions, ReviewTaskInfo};
use crate::git_scan::RangePreset;
use crate::settings::{config_path, jarvis_dir, load_raw_config, CONFIG_WRITE_LOCK};
use crate::task_bindings;
use crate::tools;
use crate::zentao::ZentaoClient;

const WORKLOG_SESSION_DIR: &str = "worklog-sessions";
const LOW_CONFIDENCE_THRESHOLD: f64 = 0.65;
const HISTORY_DAYS: i64 = 14;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TodayPlan {
    pub date: String,
    pub task_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub estimated_hours: HashMap<String, f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub custom_items: Vec<CustomPlanItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomPlanItem {
    pub id: String,
    pub name: String,
    #[serde(rename = "estimatedHours")]
    pub estimated_hours: f64,
    pub kind: String, // "custom" | "transaction"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidateTask {
    pub id: String,
    pub name: String,
    pub status: String,
    pub priority: String,
    pub score: f64,
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_hint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TodayPlanLoadResponse {
    pub date: String,
    pub task_ids: Vec<String>,
    pub work_style: String,
    pub candidate_tasks: Vec<CandidateTask>,
    #[serde(default)]
    pub estimated_hours: HashMap<String, f64>,
    #[serde(default)]
    pub custom_items: Vec<CustomPlanItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorklogCardSource {
    ReviewTask,
    TodayPlanTask,
    ManualTask,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorklogCardState {
    PendingBind,
    Ready,
    Writing,
    Written,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorklogCard {
    pub card_id: String,
    pub source: WorklogCardSource,
    pub task_id: String,
    pub task_name: String,
    pub hours: f64,
    pub work_content: String,
    pub binding_confidence: f64,
    pub binding_reason: String,
    pub evidence_summary: Vec<String>,
    pub state: WorklogCardState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorklogSummary {
    pub total_cards: usize,
    pub ready_cards: usize,
    pub pending_cards: usize,
    pub failed_cards: usize,
    pub written_cards: usize,
    pub total_hours: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorklogSession {
    pub date: String,
    #[serde(default)]
    pub work_style: String,
    #[serde(default)]
    pub transactional_emphasis: bool,
    pub today_plan: TodayPlan,
    pub cards: Vec<WorklogCard>,
    pub summary: WorklogSummary,
    pub plain_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorklogCardPatch {
    #[serde(default)]
    pub task_id: Option<String>,
    #[serde(default)]
    pub hours: Option<f64>,
    #[serde(default)]
    pub work_content: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorklogWriteResult {
    pub card_id: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorklogCardCreateInput {
    #[serde(default)]
    pub task_id: Option<String>,
    #[serde(default)]
    pub task_name: Option<String>,
    #[serde(default)]
    pub hours: Option<f64>,
    #[serde(default)]
    pub work_content: Option<String>,
}

fn today_str() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

fn session_path(date: &str) -> std::path::PathBuf {
    jarvis_dir()
        .join(WORKLOG_SESSION_DIR)
        .join(format!("{}.json", date))
}

fn read_today_plan() -> TodayPlan {
    let plan = load_raw_config()
        .and_then(|cfg| cfg.get("todayPlan").cloned())
        .and_then(|v| serde_json::from_value::<TodayPlan>(v).ok());
    match plan {
        Some(plan) if plan.date == today_str() => plan,
        _ => TodayPlan {
            date: today_str(),
            task_ids: Vec::new(),
            estimated_hours: HashMap::new(),
            custom_items: Vec::new(),
        },
    }
}

fn write_today_plan(plan: &TodayPlan) -> Result<(), String> {
    let path = config_path();
    let mut cfg = load_raw_config().unwrap_or_else(|| json!({}));
    if !cfg.is_object() {
        cfg = json!({});
    }
    cfg["todayPlan"] = serde_json::to_value(plan).map_err(|e| e.to_string())?;
    let raw = serde_json::to_string_pretty(&cfg).map_err(|e| e.to_string())?;
    crate::util::write_atomic(&path, &raw).map_err(|e| e.to_string())
}

/// 读已存档的完整会话（卡片 + 元信息）。summary 防御性重算，避免落盘值与卡片不一致。
fn read_stored_session(date: &str) -> Option<WorklogSession> {
    let path = session_path(date);
    let raw = std::fs::read_to_string(path).ok()?;
    let mut session = serde_json::from_str::<WorklogSession>(&raw).ok()?;
    session.summary = summarize(&session.cards);
    Some(session)
}

fn write_stored_session(session: &WorklogSession) -> Result<(), String> {
    let path = session_path(&session.date);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let raw = serde_json::to_string_pretty(session).map_err(|e| e.to_string())?;
    crate::util::write_atomic(&path, &raw).map_err(|e| e.to_string())
}

/// 给"编辑/写入"类命令用：有存档直接读（便宜，不打禅道 / 不跑 LLM），没有才 build 一次。
/// 全量刷新（重新关联 commit + 合并历史）只发生在 worklog_session_get。
async fn load_or_build_session(date: &str) -> Result<WorklogSession, String> {
    if let Some(session) = read_stored_session(date) {
        return Ok(session);
    }
    let session = build_session(date).await?;
    write_stored_session(&session)?;
    Ok(session)
}

fn summarize(cards: &[WorklogCard]) -> WorklogSummary {
    let total_hours = cards
        .iter()
        .map(|c| if c.hours.is_finite() && c.hours > 0.0 { c.hours } else { 0.0 })
        .sum();
    WorklogSummary {
        total_cards: cards.len(),
        ready_cards: cards
            .iter()
            .filter(|c| c.state == WorklogCardState::Ready)
            .count(),
        pending_cards: cards
            .iter()
            .filter(|c| c.state == WorklogCardState::PendingBind)
            .count(),
        failed_cards: cards
            .iter()
            .filter(|c| c.state == WorklogCardState::Failed)
            .count(),
        written_cards: cards
            .iter()
            .filter(|c| c.state == WorklogCardState::Written)
            .count(),
        total_hours,
    }
}

/// 工作定位画像。由 config.workStyle（用户在引导/设置里选的"大白话定位"）派生，
/// 用于复盘时是否侧重事务类录入。
struct WorkProfile {
    /// focused 专注写码 / multi 多线开发 / transactional 事务为主 / balanced 比较均衡
    style: String,
    /// 事务为主的人 commit 少，复盘要偏向事务类录入并提示补工时
    transactional_emphasis: bool,
}

fn work_profile_from_config(cfg: &Value) -> WorkProfile {
    let style = cfg
        .get("workStyle")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| matches!(s.as_str(), "focused" | "multi" | "transactional" | "balanced"))
        .unwrap_or_else(|| "balanced".to_string());
    let transactional_emphasis = matches!(style.as_str(), "transactional");
    WorkProfile {
        style,
        transactional_emphasis,
    }
}

fn config_hours_per_day(cfg: &Value) -> f64 {
    let periods = cfg
        .get("workSchedule")
        .and_then(|v| v.get("periods"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut total = 0i64;
    for p in periods {
        let start = p.get("start").and_then(|v| v.as_str()).unwrap_or("");
        let end = p.get("end").and_then(|v| v.as_str()).unwrap_or("");
        let parse = |hm: &str| -> Option<i64> {
            let parts: Vec<&str> = hm.split(':').collect();
            if parts.len() != 2 {
                return None;
            }
            Some(parts[0].parse::<i64>().ok()? * 60 + parts[1].parse::<i64>().ok()?)
        };
        if let (Some(s), Some(e)) = (parse(start), parse(end)) {
            total += (e - s).max(0);
        }
    }
    total as f64 / 60.0
}

fn recent_audit_hits() -> HashMap<String, usize> {
    let path = jarvis_dir().join("write-back.log");
    let raw = match std::fs::read_to_string(path) {
        Ok(v) => v,
        Err(_) => return HashMap::new(),
    };
    let cutoff = chrono::Local::now() - chrono::Duration::days(HISTORY_DAYS);
    let mut counts: HashMap<String, usize> = HashMap::new();
    for line in raw.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if v.get("ok").and_then(|x| x.as_bool()) != Some(true) {
            continue;
        }
        let Some(ts) = v.get("ts").and_then(|x| x.as_str()) else {
            continue;
        };
        let Ok(dt) = chrono::DateTime::parse_from_rfc3339(ts) else {
            continue;
        };
        if dt.with_timezone(&chrono::Local) < cutoff {
            continue;
        }
        if let Some(task_id) = v.get("taskId").and_then(|x| x.as_str()) {
            *counts.entry(task_id.to_string()).or_insert(0) += 1;
        }
    }
    counts
}

fn task_system_hint(task_id: &str) -> Option<String> {
    let binding = task_bindings::task_bindings_get(task_id.to_string()).ok().flatten()?;
    let first = binding.repo_roots.first()?;
    let path = first.replace('\\', "/");
    path.rsplit('/').nth(1).map(|s| s.to_string()).or_else(|| path.rsplit('/').next().map(|s| s.to_string()))
}

async fn candidate_tasks() -> Result<Vec<CandidateTask>, String> {
    let client = ZentaoClient::from_settings()?;
    let tasks = client.get_all_assigned_tasks().await?;
    let hits = recent_audit_hits();
    let mut out: Vec<CandidateTask> = tasks
        .into_iter()
        .filter(|t| {
            let status = t.get("status").and_then(|v| v.as_str()).unwrap_or("");
            status != "done" && status != "closed" && status != "cancel"
        })
        .map(|t| {
            let id = t
                .get("id")
                .map(|v| {
                    v.as_str()
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| v.to_string().trim_matches('"').to_string())
                })
                .unwrap_or_default();
            let name = t
                .get("name")
                .or_else(|| t.get("title"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let status = t.get("status").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let priority = t
                .get("priority")
                .and_then(|v| v.as_str())
                .unwrap_or("normal")
                .to_string();
            let mut score = *hits.get(&id).unwrap_or(&0) as f64;
            let hint = task_system_hint(&id);
            if hint.is_some() {
                score += 1.5;
            }
            CandidateTask {
                id,
                name,
                status,
                priority,
                score,
                reason: if score > 0.0 {
                    "recently used or bound task".to_string()
                } else {
                    "active task".to_string()
                },
                system_hint: hint,
            }
        })
        .collect();

    out.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.id.cmp(&b.id))
    });

    Ok(out)
}

async fn build_review_data() -> Result<daily_review::DailyReview, String> {
    let client = ZentaoClient::from_settings()?;
    let all_tasks = client.get_my_tasks().await?;

    let task_inputs: Vec<TaskInput> = all_tasks
        .iter()
        .map(|t| TaskInput {
            id: t
                .get("id")
                .map(|v| {
                    v.as_str()
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| v.to_string().trim_matches('"').to_string())
                })
                .unwrap_or_default(),
            name: t
                .get("name")
                .or(t.get("title"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
        })
        .collect();
    let review_tasks: Vec<ReviewTaskInfo> = all_tasks
        .iter()
        .map(|t| ReviewTaskInfo {
            id: t
                .get("id")
                .map(|v| {
                    v.as_str()
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| v.to_string().trim_matches('"').to_string())
                })
                .unwrap_or_default(),
            name: t
                .get("name")
                .or(t.get("title"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            status: t
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("wait")
                .to_string(),
        })
        .collect();
    let cfg = crate::settings::load_raw_config().unwrap_or_else(|| json!({}));
    let root_dirs = crate::git_scan::get_repo_roots();
    if root_dirs.is_empty() {
        return Err("missing repoRoots".into());
    }
    let hours_per_work_day = config_hours_per_day(&cfg);
    let link_result = commit_link::link_tasks_with_commits(
        &task_inputs,
        LinkCommitsOptions {
            range: RangePreset::Today,
            since: None,
            until: None,
            root_dirs: &root_dirs,
            include_body: true,
            use_llm: true,
            min_confidence: 0.4,
        },
    )
    .await?;
    Ok(daily_review::build_daily_review(
        link_result,
        &review_tasks,
        BuildOptions {
            date: Some(today_str().as_str()),
            hours_per_work_day,
        },
    ))
}

fn review_card_id(task_id: &str) -> String {
    format!("review:{}", task_id)
}

fn today_plan_card_id(task_id: &str) -> String {
    format!("plan:{}", task_id)
}

fn manual_card_id() -> String {
    format!("manual:{}", chrono::Local::now().timestamp_millis())
}

fn evidence_from_commits(commits: &[CommitLink]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut lines = Vec::new();
    for c in commits {
        let cleaned = crate::git_scan::clean_commit_title(&c.title, 200);
        if cleaned.is_empty() || seen.contains(&cleaned) {
            continue;
        }
        seen.insert(cleaned.clone());
        lines.push(cleaned);
    }
    lines
}

fn derive_card_state(card: &WorklogCard) -> WorklogCardState {
    if card.task_id.trim().is_empty() || card.binding_confidence < LOW_CONFIDENCE_THRESHOLD {
        return WorklogCardState::PendingBind;
    }
    if card.hours <= 0.0 || card.work_content.trim().is_empty() {
        return WorklogCardState::PendingBind;
    }
    WorklogCardState::Ready
}

fn merge_card(mut fresh: WorklogCard, saved: Option<&WorklogCard>) -> WorklogCard {
    if let Some(prev) = saved {
        if !prev.task_id.trim().is_empty() {
            fresh.task_id = prev.task_id.clone();
        }
        if prev.hours.is_finite() && prev.hours > 0.0 {
            fresh.hours = prev.hours;
        }
        if !prev.work_content.trim().is_empty() {
            fresh.work_content = prev.work_content.clone();
        }
        if matches!(prev.state, WorklogCardState::Written | WorklogCardState::Failed) {
            fresh.state = prev.state.clone();
            fresh.effort_id = prev.effort_id.clone();
            fresh.error = prev.error.clone();
            fresh.updated_at = prev.updated_at.clone();
        }
    }
    if !matches!(fresh.state, WorklogCardState::Written | WorklogCardState::Failed | WorklogCardState::Writing) {
        fresh.state = derive_card_state(&fresh);
    }
    fresh
}

async fn build_session(date: &str) -> Result<WorklogSession, String> {
    let cfg = crate::settings::load_raw_config().unwrap_or_else(|| json!({}));
    let profile = work_profile_from_config(&cfg);
    let today_plan = read_today_plan();
    let review = if date == today_str() {
        Some(build_review_data().await?)
    } else {
        None
    };
    let saved = read_stored_session(date);
    let saved_map: HashMap<String, WorklogCard> = saved
        .map(|s| s.cards.into_iter().map(|c| (c.card_id.clone(), c)).collect())
        .unwrap_or_default();

    let mut cards: Vec<WorklogCard> = Vec::new();

    if let Some(review) = &review {
        for t in &review.advanced_tasks {
            let confidence = if t.binding_confidence.is_finite() {
                t.binding_confidence
            } else {
                0.9
            };
            let fresh = WorklogCard {
                card_id: review_card_id(&t.task_id),
                source: WorklogCardSource::ReviewTask,
                task_id: t.task_id.clone(),
                task_name: t.task_name.clone(),
                hours: t.suggested_hours.unwrap_or(0.0),
                work_content: t.default_work_content.clone(),
                binding_confidence: confidence,
                binding_reason: t.binding_reason.clone(),
                evidence_summary: evidence_from_commits(&t.commits),
                state: if confidence < LOW_CONFIDENCE_THRESHOLD {
                    WorklogCardState::PendingBind
                } else {
                    WorklogCardState::Ready
                },
                effort_id: None,
                error: None,
                updated_at: None,
            };
            let merged = merge_card(fresh, saved_map.get(&review_card_id(&t.task_id)));
            cards.push(merged);
        }
    }

    if date == today_plan.date {
        let existing_task_ids: HashSet<String> = cards.iter().map(|c| c.task_id.clone()).collect();
        let candidate_map: HashMap<String, CandidateTask> = candidate_tasks()
            .await?
            .into_iter()
            .map(|t| (t.id.clone(), t))
            .collect();
        for task_id in &today_plan.task_ids {
            if existing_task_ids.contains(task_id) {
                continue;
            }
            let task_name = candidate_map
                .get(task_id)
                .map(|t| t.name.clone())
                .unwrap_or_else(|| format!("任务 #{}", task_id));
            let fresh = WorklogCard {
                card_id: today_plan_card_id(task_id),
                source: WorklogCardSource::TodayPlanTask,
                task_id: task_id.clone(),
                task_name,
                hours: 0.0,
                work_content: String::new(),
                binding_confidence: 1.0,
                binding_reason: "selected in today's plan".to_string(),
                evidence_summary: Vec::new(),
                state: WorklogCardState::Ready,
                effort_id: None,
                error: None,
                updated_at: None,
            };
            cards.push(merge_card(fresh, saved_map.get(&today_plan_card_id(task_id))));
        }
    }

    // 手动事务卡只存在于存档里（不由 review / today_plan 派生），全量重建时必须从存档捞回，
    // 否则刷新 / 重开复盘窗一次就丢。card_id 形如 manual:{ts}，不会与派生卡重名。
    let existing_ids: HashSet<String> = cards.iter().map(|c| c.card_id.clone()).collect();
    let mut recovered: Vec<WorklogCard> = saved_map
        .values()
        .filter(|c| c.source == WorklogCardSource::ManualTask && !existing_ids.contains(&c.card_id))
        .cloned()
        .collect();
    recovered.sort_by(|a, b| a.card_id.cmp(&b.card_id));
    cards.extend(recovered);

    cards.sort_by(|a, b| a.task_name.cmp(&b.task_name));
    let plain_text = review.map(|r| r.plain_text).unwrap_or_default();
    let summary = summarize(&cards);
    Ok(WorklogSession {
        date: date.to_string(),
        work_style: profile.style,
        transactional_emphasis: profile.transactional_emphasis,
        today_plan,
        cards,
        summary,
        plain_text,
    })
}

fn update_card_in_session(
    session: &mut WorklogSession,
    card_id: &str,
    patch: WorklogCardPatch,
) -> Result<WorklogCard, String> {
    let card = session
        .cards
        .iter_mut()
        .find(|c| c.card_id == card_id)
        .ok_or_else(|| format!("card {} not found", card_id))?;
    if let Some(task_id) = patch.task_id {
        card.task_id = task_id;
    }
    if let Some(hours) = patch.hours {
        card.hours = hours;
    }
    if let Some(work_content) = patch.work_content {
        card.work_content = work_content;
    }
    card.updated_at = Some(chrono::Local::now().to_rfc3339());
    if !matches!(card.state, WorklogCardState::Written | WorklogCardState::Writing) {
        card.state = derive_card_state(card);
    }
    Ok(card.clone())
}

async fn write_card_impl(session: &mut WorklogSession, card_id: &str) -> Result<WorklogWriteResult, String> {
    let idx = session
        .cards
        .iter()
        .position(|c| c.card_id == card_id)
        .ok_or_else(|| format!("card {} not found", card_id))?;
    let task_id = session.cards[idx].task_id.clone();
    let hours = session.cards[idx].hours;
    let work_content = session.cards[idx].work_content.clone();
    if task_id.trim().is_empty() {
        return Err("taskId is required".into());
    }
    if hours <= 0.0 {
        return Err("hours must be positive".into());
    }
    if work_content.trim().is_empty() {
        return Err("work content is required".into());
    }

    session.cards[idx].state = WorklogCardState::Writing;
    session.cards[idx].error = None;
    write_stored_session(session)?;

    // 稳定幂等键：同一天同一张卡重复写（哪怕 session 丢失后重建）都用同一个 key，
    // log-task-effort 侧据此查重，避免在禅道写出重复工时。
    let idempotency_key = format!("worklog:{}:{}", session.date, card_id);
    let input = json!({
        "taskId": task_id,
        "hours": hours,
        "work": work_content,
        "clientRequestId": idempotency_key,
    });
    let result = tools::dispatch("log-task-effort", input).await;

    match result {
        Ok(v) => {
            let effort_id = v
                .get("effortId")
                .map(|x| x.to_string().trim_matches('"').to_string());
            session.cards[idx].state = WorklogCardState::Written;
            session.cards[idx].effort_id = effort_id.clone();
            session.cards[idx].updated_at = Some(chrono::Local::now().to_rfc3339());
            session.cards[idx].error = None;
            write_stored_session(session)?;
            Ok(WorklogWriteResult {
                card_id: card_id.to_string(),
                ok: true,
                effort_id,
                error: None,
            })
        }
        Err(e) => {
            session.cards[idx].state = WorklogCardState::Failed;
            session.cards[idx].error = Some(e.clone());
            session.cards[idx].updated_at = Some(chrono::Local::now().to_rfc3339());
            write_stored_session(session)?;
            Ok(WorklogWriteResult {
                card_id: card_id.to_string(),
                ok: false,
                effort_id: None,
                error: Some(e),
            })
        }
    }
}

#[tauri::command]
pub async fn today_plan_load() -> Result<TodayPlanLoadResponse, String> {
    let cfg = crate::settings::load_raw_config().unwrap_or_else(|| json!({}));
    let profile = work_profile_from_config(&cfg);
    let plan = read_today_plan();
    let candidate_tasks = candidate_tasks().await?;
    Ok(TodayPlanLoadResponse {
        date: plan.date,
        task_ids: plan.task_ids,
        work_style: profile.style,
        candidate_tasks,
        estimated_hours: plan.estimated_hours,
        custom_items: plan.custom_items,
    })
}

#[tauri::command]
pub async fn today_plan_save(
    task_ids: Vec<String>,
    date: Option<String>,
    estimated_hours: Option<HashMap<String, f64>>,
    custom_items: Option<Vec<CustomPlanItem>>,
    app: tauri::AppHandle,
) -> Result<TodayPlanLoadResponse, String> {
    let next = TodayPlan {
        date: date.unwrap_or_else(today_str),
        task_ids,
        estimated_hours: estimated_hours.unwrap_or_default(),
        custom_items: custom_items.unwrap_or_default(),
    };
    let _guard = CONFIG_WRITE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    write_today_plan(&next)?;
    use tauri::Emitter;
    let _ = app.emit("config-changed", ());
    drop(_guard);
    today_plan_load().await
}

#[tauri::command]
pub async fn today_plan_clear(date: Option<String>, app: tauri::AppHandle) -> Result<TodayPlanLoadResponse, String> {
    let next = TodayPlan {
        date: date.unwrap_or_else(today_str),
        task_ids: Vec::new(),
        estimated_hours: HashMap::new(),
        custom_items: Vec::new(),
    };
    let _guard = CONFIG_WRITE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    write_today_plan(&next)?;
    use tauri::Emitter;
    let _ = app.emit("config-changed", ());
    drop(_guard);
    today_plan_load().await
}

#[tauri::command]
pub async fn today_plan_lookup_task(task_id: String) -> Result<CandidateTask, String> {
    let client = ZentaoClient::from_settings()?;
    let task = client
        .get_task(&task_id)
        .await?
        .ok_or_else(|| format!("任务 #{} 不存在", task_id))?;
    let id = task
        .get("id")
        .map(|v| {
            v.as_str()
                .map(|s| s.to_string())
                .unwrap_or_else(|| v.to_string().trim_matches('"').to_string())
        })
        .unwrap_or_default();
    let name = task
        .get("name")
        .or_else(|| task.get("title"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let status = task.get("status").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let priority = task
        .get("priority")
        .and_then(|v| v.as_str())
        .unwrap_or("normal")
        .to_string();
    Ok(CandidateTask {
        id,
        name,
        status,
        priority,
        score: 0.0,
        reason: "manually added by ID".to_string(),
        system_hint: None,
    })
}

#[tauri::command]
pub async fn worklog_session_get(date: Option<String>) -> Result<WorklogSession, String> {
    let date = date.unwrap_or_else(today_str);
    let session = build_session(&date).await?;
    write_stored_session(&session)?;
    Ok(session)
}

#[tauri::command]
pub async fn worklog_card_update(
    card_id: String,
    patch: WorklogCardPatch,
    date: Option<String>,
) -> Result<WorklogCard, String> {
    let date = date.unwrap_or_else(today_str);
    let mut session = load_or_build_session(&date).await?;
    let card = update_card_in_session(&mut session, &card_id, patch)?;
    write_stored_session(&session)?;
    Ok(card)
}

#[tauri::command]
pub async fn worklog_manual_card_add(
    input: WorklogCardCreateInput,
    date: Option<String>,
) -> Result<WorklogCard, String> {
    let date = date.unwrap_or_else(today_str);
    let mut session = load_or_build_session(&date).await?;
    let task_id = input.task_id.unwrap_or_default();
    let card = WorklogCard {
        card_id: manual_card_id(),
        source: WorklogCardSource::ManualTask,
        task_id: task_id.clone(),
        task_name: input
            .task_name
            .or_else(|| {
                if task_id.trim().is_empty() {
                    None
                } else {
                    Some(format!("事务类工作 {}", task_id))
                }
            })
            .unwrap_or_else(|| "事务类工作".to_string()),
        hours: input.hours.unwrap_or(0.0),
        work_content: input.work_content.unwrap_or_default(),
        binding_confidence: if task_id.trim().is_empty() { 0.0 } else { 1.0 },
        binding_reason: if task_id.trim().is_empty() {
            "手动添加，待选择任务".to_string()
        } else {
            "手动添加".to_string()
        },
        evidence_summary: Vec::new(),
        state: if task_id.trim().is_empty() {
            WorklogCardState::PendingBind
        } else {
            WorklogCardState::Ready
        },
        effort_id: None,
        error: None,
        updated_at: Some(chrono::Local::now().to_rfc3339()),
    };
    session.cards.push(card.clone());
    session.summary = summarize(&session.cards);
    write_stored_session(&session)?;
    Ok(card)
}

#[tauri::command]
pub async fn worklog_card_remove(
    card_id: String,
    date: Option<String>,
) -> Result<WorklogSession, String> {
    let date = date.unwrap_or_else(today_str);
    let mut session = load_or_build_session(&date).await?;
    session.cards.retain(|c| c.card_id != card_id);
    session.summary = summarize(&session.cards);
    write_stored_session(&session)?;
    Ok(session)
}

#[tauri::command]
pub async fn worklog_card_write(
    card_id: String,
    date: Option<String>,
) -> Result<WorklogWriteResult, String> {
    let date = date.unwrap_or_else(today_str);
    let mut session = load_or_build_session(&date).await?;
    let result = write_card_impl(&mut session, &card_id).await?;
    Ok(result)
}

#[tauri::command]
pub async fn worklog_session_write_confirmed(
    date: Option<String>,
) -> Result<Vec<WorklogWriteResult>, String> {
    let date = date.unwrap_or_else(today_str);
    let mut session = load_or_build_session(&date).await?;
    let card_ids: Vec<String> = session
        .cards
        .iter()
        .filter(|c| c.state == WorklogCardState::Ready)
        .map(|c| c.card_id.clone())
        .collect();
    let mut out = Vec::new();
    for card_id in card_ids {
        out.push(write_card_impl(&mut session, &card_id).await?);
    }
    Ok(out)
}
