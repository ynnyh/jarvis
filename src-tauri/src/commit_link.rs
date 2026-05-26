// 任务 ↔ commit 关联服务。
//
// 移植自 src/services/commit-link-service.ts。
//
// 两遍匹配：
//   1. 精确匹配：commit message 含 #任务号 → 直接关联
//   2. 软关联：业务线（rootDir 下第一层目录名）的关键词命中任务名 → 候选关联
//
// 可选第三遍：用 LLM 给 soft 匹配评分，丢掉低置信度的（LLM 失败回退到规则结果）。

#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{HashMap, HashSet};

use crate::git_scan::{
    self, effort_for_commit, list_my_local_commits, DateRange,
    ListMyLocalCommitsInput, LocalCommit, MatchDimension, RangePreset,
};
use crate::llm::{self, ChatMessage, ChatRequest, Role};

// ============================================================================
// 类型
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskInput {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MatchType {
    Exact,
    Soft,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitLink {
    pub sha: String,
    pub short_sha: String,
    pub title: String,
    pub authored_date: String,
    pub repo_path: String,
    pub business_line: String,
    pub repo_name: String,
    pub match_type: MatchType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matched_keywords: Option<Vec<String>>,
    pub effort: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskCommitLinks {
    pub task_id: String,
    pub task_name: String,
    pub commits: Vec<CommitLink>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrphanGroup {
    pub business_line: String,
    pub commits: Vec<CommitLink>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitLinkResult {
    pub range: DateRange,
    pub scanned_repos: usize,
    pub total_commits: usize,
    pub tasks: Vec<TaskCommitLinks>,
    pub orphan_commits: Vec<OrphanGroup>,
}

pub struct LinkCommitsOptions<'a> {
    pub range: RangePreset,
    pub since: Option<&'a str>,
    pub until: Option<&'a str>,
    pub root_dirs: &'a [String],
    pub include_body: bool,
    pub use_llm: bool,
    pub min_confidence: f64,
}

// ============================================================================
// 工具：path 规整 / 业务线提取
// ============================================================================

fn norm_path(p: &str) -> String {
    let s = p.replace('\\', "/");
    s.trim_end_matches('/').to_string()
}

fn basename(p: &str) -> String {
    let np = norm_path(p);
    np.rsplit('/').next().unwrap_or("").to_string()
}

pub fn extract_business_line(repo_path: &str, root_dirs: &[String]) -> String {
    let np = norm_path(repo_path);
    for root in root_dirs {
        let nr = norm_path(root);
        if np == nr {
            return basename(&np);
        }
        let prefix = format!("{}/", nr);
        if np.starts_with(&prefix) {
            let rel = &np[prefix.len()..];
            if let Some(first) = rel.split('/').find(|s| !s.is_empty()) {
                return first.to_string();
            }
        }
    }
    basename(&np)
}

const TRIM_PREFIXES: &[&str] = &["示例公司", "胜利工贸", "钰海工贸", "通才铁前", "通才", "鸿丰达"];

pub fn extract_repo_keywords(business_line: &str, aliases: &HashMap<String, Vec<String>>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let push = |s: String, out: &mut Vec<String>, seen: &mut HashSet<String>| {
        if s.chars().count() >= 2 && !seen.contains(&s) {
            seen.insert(s.clone());
            out.push(s);
        }
    };
    push(business_line.to_string(), &mut out, &mut seen);
    for prefix in TRIM_PREFIXES {
        if business_line.starts_with(prefix) && business_line.len() > prefix.len() {
            let rest = business_line[prefix.len()..].to_string();
            push(rest, &mut out, &mut seen);
        }
    }
    // 末尾 mes 后缀
    let lower = business_line.to_lowercase();
    if lower.ends_with("mes") && lower.len() > 3 {
        let prefix_len = business_line.len() - 3;
        push(business_line[..prefix_len].to_string(), &mut out, &mut seen);
        push("mes".to_string(), &mut out, &mut seen);
    }
    if let Some(aliased) = aliases.get(business_line) {
        for a in aliased {
            push(a.clone(), &mut out, &mut seen);
        }
    }
    out
}

// ============================================================================
// 精确匹配
// ============================================================================

pub fn extract_task_ids_from_message(commit: &LocalCommit) -> Vec<String> {
    use regex::Regex;
    let text = format!("{}\n{}", commit.title, commit.body.clone().unwrap_or_default());
    let patterns: [&str; 3] = [
        r"#(\d{3,7})\b",
        r"(?i)\btask[-_\s]?#?(\d{3,7})\b",
        r"(?i)\bzentao[-_\s]?#?(\d{3,7})\b",
    ];
    let mut out: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for p in patterns {
        if let Ok(re) = Regex::new(p) {
            for cap in re.captures_iter(&text) {
                if let Some(m) = cap.get(1) {
                    let id = m.as_str().to_string();
                    if !seen.contains(&id) {
                        seen.insert(id.clone());
                        out.push(id);
                    }
                }
            }
        }
    }
    out
}

// ============================================================================
// 软关联
// ============================================================================

struct CommitItem {
    commit: LocalCommit,
    repo_path: String,
    repo_name: String,
}

struct BusinessGroup {
    business_line: String,
    keywords: Vec<String>,
    commits: Vec<CommitItem>,
}

fn match_task_against_group(task_name: &str, group: &BusinessGroup) -> Vec<String> {
    let lower = task_name.to_lowercase();
    group
        .keywords
        .iter()
        .filter(|kw| lower.contains(&kw.to_lowercase()))
        .cloned()
        .collect()
}

// ============================================================================
// 主入口
// ============================================================================

pub async fn link_tasks_with_commits(
    tasks: &[TaskInput],
    options: LinkCommitsOptions<'_>,
) -> Result<CommitLinkResult, String> {
    let raw = list_my_local_commits(ListMyLocalCommitsInput {
        root_dirs: options.root_dirs,
        range: options.range,
        since: options.since,
        until: options.until,
        author: None,
        match_mode: MatchDimension::Author,
        include_body: options.include_body,
        include_stat: true, // effort 估算需要
        max_depth: 5,
    })
    .await?;

    let aliases = git_scan::load_business_aliases();
    let excluded = git_scan::load_excluded_business_lines();
    let root_dirs = raw.root_dirs.clone();

    // 过滤排除的业务线
    let raw_repos: Vec<_> = raw
        .repos
        .into_iter()
        .filter(|r| !excluded.contains(&extract_business_line(&r.repo_path, &root_dirs)))
        .collect();

    let effective_total_commits: usize = raw_repos.iter().map(|r| r.commits.len()).sum();

    // 按业务线聚合
    let mut group_map: HashMap<String, BusinessGroup> = HashMap::new();
    for r in raw_repos {
        let business_line = extract_business_line(&r.repo_path, &root_dirs);
        let group = group_map.entry(business_line.clone()).or_insert_with(|| BusinessGroup {
            business_line: business_line.clone(),
            keywords: extract_repo_keywords(&business_line, &aliases),
            commits: Vec::new(),
        });
        let repo_name = basename(&r.repo_path);
        for c in r.commits {
            group.commits.push(CommitItem {
                commit: c,
                repo_path: r.repo_path.clone(),
                repo_name: repo_name.clone(),
            });
        }
    }
    let groups: Vec<BusinessGroup> = group_map.into_values().collect();

    let task_by_id: HashMap<String, &TaskInput> = tasks.iter().map(|t| (t.id.clone(), t)).collect();

    let mut task_links: HashMap<String, Vec<CommitLink>> = HashMap::new();
    let mut used_commit_keys: HashSet<String> = HashSet::new();

    let to_link = |item: &CommitItem, business_line: &str, match_type: MatchType, matched_keywords: Option<Vec<String>>| -> CommitLink {
        CommitLink {
            sha: item.commit.sha.clone(),
            short_sha: item.commit.short_sha.clone(),
            title: item.commit.title.clone(),
            authored_date: item.commit.authored_date.clone(),
            repo_path: item.repo_path.clone(),
            repo_name: item.repo_name.clone(),
            business_line: business_line.to_string(),
            match_type,
            matched_keywords,
            effort: effort_for_commit(&item.commit),
            confidence: None,
            reason: None,
        }
    };

    // 第一遍：精确匹配
    for group in &groups {
        for item in &group.commits {
            let ids = extract_task_ids_from_message(&item.commit);
            if ids.is_empty() {
                continue;
            }
            for id in ids {
                if !task_by_id.contains_key(&id) {
                    continue;
                }
                task_links
                    .entry(id)
                    .or_default()
                    .push(to_link(item, &group.business_line, MatchType::Exact, None));
                used_commit_keys.insert(format!("{}:{}", item.repo_path, item.commit.sha));
            }
        }
    }

    // 第二遍：软关联
    for task in tasks {
        let task_id = task.id.clone();
        for group in &groups {
            let hits = match_task_against_group(&task.name, group);
            if hits.is_empty() {
                continue;
            }
            for item in &group.commits {
                let key = format!("{}:{}", item.repo_path, item.commit.sha);
                if used_commit_keys.contains(&key) {
                    continue;
                }
                task_links
                    .entry(task_id.clone())
                    .or_default()
                    .push(to_link(item, &group.business_line, MatchType::Soft, Some(hits.clone())));
            }
        }
    }

    // 第三遍（可选）：LLM 评分
    if options.use_llm {
        if let Ok(score_map) = score_soft_matches_with_llm(tasks, &task_links).await {
            let threshold = options.min_confidence;
            for (task_id, links) in task_links.iter_mut() {
                let mut kept: Vec<CommitLink> = Vec::new();
                for link in links.drain(..) {
                    if link.match_type != MatchType::Soft {
                        kept.push(link);
                        continue;
                    }
                    let key = format!("{}|{}", task_id, link.sha);
                    match score_map.get(&key) {
                        None => kept.push(link), // 没评分：保守保留
                        Some((conf, reason)) => {
                            if *conf < threshold {
                                continue; // 丢弃
                            }
                            let mut l = link;
                            l.confidence = Some(*conf);
                            l.reason = Some(reason.clone());
                            kept.push(l);
                        }
                    }
                }
                *links = kept;
            }
        }
    }

    // 孤儿 commit：未被任何 task_links 认领的
    let mut all_used: HashSet<String> = used_commit_keys.clone();
    for links in task_links.values() {
        for l in links {
            all_used.insert(format!("{}:{}", l.repo_path, l.sha));
        }
    }
    let mut orphan_map: HashMap<String, Vec<CommitLink>> = HashMap::new();
    for group in &groups {
        for item in &group.commits {
            let key = format!("{}:{}", item.repo_path, item.commit.sha);
            if all_used.contains(&key) {
                continue;
            }
            orphan_map
                .entry(group.business_line.clone())
                .or_default()
                .push(to_link(item, &group.business_line, MatchType::Soft, None));
        }
    }

    // 组装 tasks 输出
    let mut tasks_out: Vec<TaskCommitLinks> = Vec::new();
    for (task_id, mut commits) in task_links {
        let task = match task_by_id.get(&task_id) {
            Some(t) => *t,
            None => continue,
        };
        commits.sort_by(|a, b| match (a.match_type, b.match_type) {
            (MatchType::Exact, MatchType::Soft) => std::cmp::Ordering::Less,
            (MatchType::Soft, MatchType::Exact) => std::cmp::Ordering::Greater,
            _ => b.authored_date.cmp(&a.authored_date),
        });
        tasks_out.push(TaskCommitLinks {
            task_id,
            task_name: task.name.clone(),
            commits,
        });
    }

    let orphan_commits: Vec<OrphanGroup> = orphan_map
        .into_iter()
        .map(|(business_line, commits)| OrphanGroup { business_line, commits })
        .collect();

    Ok(CommitLinkResult {
        range: raw.range,
        scanned_repos: raw.scanned_repos,
        total_commits: effective_total_commits,
        tasks: tasks_out,
        orphan_commits,
    })
}

// ============================================================================
// LLM 评分（对齐 TS scoreSoftMatchesWithLlm）
// ============================================================================

async fn score_soft_matches_with_llm(
    tasks: &[TaskInput],
    task_links: &HashMap<String, Vec<CommitLink>>,
) -> Result<HashMap<String, (f64, String)>, String> {
    let task_name_by_id: HashMap<String, String> = tasks.iter().map(|t| (t.id.clone(), t.name.clone())).collect();
    let mut candidates: Vec<serde_json::Value> = Vec::new();
    for (task_id, links) in task_links {
        let task_name = task_name_by_id.get(task_id).cloned().unwrap_or_default();
        for link in links {
            if link.match_type != MatchType::Soft {
                continue;
            }
            candidates.push(json!({
                "taskId": task_id,
                "taskName": task_name,
                "sha": link.sha,
                "title": link.title,
                "businessLine": link.business_line,
                "keywords": link.matched_keywords,
            }));
        }
    }
    if candidates.is_empty() {
        return Ok(HashMap::new());
    }

    let max_tokens = (200 + candidates.len() as u32 * 60).min(4000);
    let messages = vec![
        ChatMessage {
            role: Role::System,
            content: "你是一个代码提交与任务关联的评分助手。\n\
给定一组 (任务, commit) 候选对，判断这个 commit 是否真的在推进这个任务。\n\
严格按 JSON 输出，每项包含 taskId、sha、confidence (0~1, 两位小数)、reason (一句话中文)。\n\
评分参考：\n\
- 0.8~1.0：commit 标题直接指向任务的功能点\n\
- 0.5~0.79：commit 在同业务线下，且涉及任务相关的模块\n\
- 0.2~0.49：仅业务线匹配，但 commit 在做不相干的事\n\
- 0~0.19：明显无关\n\
只输出 JSON 数组本身，不要 ```json 包裹，不要解释。"
                .to_string(),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        },
        ChatMessage {
            role: Role::User,
            content: format!(
                "候选对：\n```json\n{}\n```\n请返回形如 [{{\"taskId\":\"123\",\"sha\":\"abc\",\"confidence\":0.8,\"reason\":\"...\"}}, ...] 的 JSON 数组。",
                serde_json::to_string_pretty(&candidates).unwrap_or_default()
            ),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        },
    ];
    let mut req = ChatRequest::new(messages);
    req.temperature = Some(0.1);
    req.max_tokens = Some(max_tokens);

    let resp = llm::chat(req).await?;
    let parsed = parse_llm_json_array(&resp.text)?;
    let mut out: HashMap<String, (f64, String)> = HashMap::new();
    for row in parsed {
        let task_id = row.get("taskId").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let sha = row.get("sha").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let confidence = row.get("confidence").and_then(|v| v.as_f64()).unwrap_or(f64::NAN);
        let reason = row.get("reason").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if task_id.is_empty() || sha.is_empty() || !confidence.is_finite() {
            continue;
        }
        out.insert(
            format!("{}|{}", task_id, sha),
            (confidence.max(0.0).min(1.0), reason),
        );
    }
    Ok(out)
}

fn parse_llm_json_array(text: &str) -> Result<Vec<serde_json::Value>, String> {
    let mut s = text.trim().to_string();
    // 剥围栏
    if let Some(start) = s.find("```") {
        if let Some(after) = s.get(start..) {
            if let Some(rest_start) = after.find('\n') {
                let rest = &after[rest_start + 1..];
                if let Some(end) = rest.rfind("```") {
                    s = rest[..end].trim().to_string();
                }
            }
        }
    }
    let start = s.find('[').ok_or_else(|| format!("LLM 输出不含 JSON 数组: {}", &text[..text.len().min(200)]))?;
    let end = s.rfind(']').ok_or_else(|| "LLM 输出缺 ]".to_string())?;
    if end <= start {
        return Err("LLM 输出 ] 在 [ 之前".to_string());
    }
    let slice = &s[start..=end];
    let parsed: serde_json::Value = serde_json::from_str(slice).map_err(|e| format!("LLM JSON 解析失败: {}", e))?;
    parsed
        .as_array()
        .cloned()
        .ok_or_else(|| "LLM JSON 不是数组".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn business_line_extraction() {
        let roots = vec!["D:/coding".to_string()];
        assert_eq!(extract_business_line("D:/coding/物流/logistics-web", &roots), "物流");
        assert_eq!(extract_business_line("D:/coding/deer-flow", &roots), "deer-flow");
        assert_eq!(extract_business_line("D:\\coding\\示例销售线\\example-sale-app", &roots), "示例销售线");
    }

    #[test]
    fn keywords_with_trim_prefix() {
        let aliases = HashMap::new();
        let kw = extract_repo_keywords("示例销售线", &aliases);
        assert!(kw.contains(&"示例销售线".to_string()));
        assert!(kw.contains(&"销售".to_string()));
    }

    #[test]
    fn keywords_with_aliases() {
        let mut aliases = HashMap::new();
        aliases.insert("示例业务线".to_string(), vec!["门禁".to_string(), "计量".to_string()]);
        let kw = extract_repo_keywords("示例业务线", &aliases);
        assert!(kw.contains(&"门禁".to_string()));
        assert!(kw.contains(&"计量".to_string()));
    }

    #[test]
    fn task_id_extraction() {
        let c = LocalCommit {
            sha: "abc".into(),
            short_sha: "abc".into(),
            author_name: "".into(),
            author_email: "".into(),
            authored_date: "".into(),
            committer_name: "".into(),
            committer_email: "".into(),
            committed_date: "".into(),
            title: "fix #10238 login bug".into(),
            body: Some("see task-99999".into()),
            stat: None,
        };
        let ids = extract_task_ids_from_message(&c);
        assert!(ids.contains(&"10238".to_string()));
        assert!(ids.contains(&"99999".to_string()));
    }
}
