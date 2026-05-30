// 任务 ↔ commit 关联服务（v2，绑定优先架构）。
//
// 重写理由：v1 用业务线（rootDir 下第一层目录名）的关键词去匹配任务标题，
// 命中率全靠运气，"瞎猜瞎关联"是用户原话。v2 引入显式的 task↔repo 绑定表
// （task_bindings 模块），消除关键词猜测。
//
// 两遍 + 可选第三遍匹配：
//   Pass 1 (Exact): commit message 含 #任务号 → 直接关联，最高优先级，不走 LLM
//   Pass 2 (Binding): commit 所在 repo 反查 task_bindings 拿到候选任务集
//                     - 集合 == 0 → 归零散修复桶
//                     - 集合 == 1 → 直接归属（1:1 绑定的常见路径）
//                     - 集合 > 1  → 走 Pass 3 LLM 分类
//   Pass 3 (LLM, 可选): 用 commit_classifier 对多候选场景做归属判定
//
// 旧的关键词/业务线软关联整套逻辑下线，避免错关联。

#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use crate::commit_classifier;
use crate::git_scan::{
    self, effort_for_commit, list_my_local_commits, DateRange, ListMyLocalCommitsInput,
    LocalCommit, MatchDimension, RangePreset,
};

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
    /// commit message 显式写了 #任务号
    Exact,
    /// 通过 task↔repo 绑定推断（可能含 LLM 二次判定）
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
    /// v2 不再使用关键词，保留字段是为了前端老组件平滑过渡（永远是 None）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matched_keywords: Option<Vec<String>>,
    pub effort: f64,
    /// LLM 给出的置信度 0~1（绑定+LLM 路径才有；纯绑定唯一候选给固定 0.9）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
    /// LLM 给的归属理由 / 绑定路径给的固定文案
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
    /// true 时多候选场景走 LLM 分类；false 时多候选 commit 直接归零散桶
    pub use_llm: bool,
    /// LLM 分类的置信度下限。低于此值视为"未归属"，落入零散桶
    pub min_confidence: f64,
}

// ============================================================================
// 路径与业务线
// ============================================================================

fn norm_path(p: &str) -> String {
    let s = p.replace('\\', "/");
    s.trim_end_matches('/').to_string()
}

fn basename(p: &str) -> String {
    let np = norm_path(p);
    np.rsplit('/').next().unwrap_or("").to_string()
}

/// 业务线 = rootDir 下第一层目录名（外接磁盘 `D:/coding/物流/foo` → "物流"）。
/// 仅用于零散修复桶的分组展示，不再参与匹配判定。
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

// ============================================================================
// 精确匹配
// ============================================================================

pub fn extract_task_ids_from_message(commit: &LocalCommit) -> Vec<String> {
    use regex::Regex;
    let text = format!(
        "{}\n{}",
        commit.title,
        commit.body.clone().unwrap_or_default()
    );
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
// 主入口
// ============================================================================

struct CommitItem {
    commit: LocalCommit,
    repo_path: String,
    repo_name: String,
    business_line: String,
}

fn make_link(
    item: &CommitItem,
    match_type: MatchType,
    confidence: Option<f64>,
    reason: Option<String>,
) -> CommitLink {
    CommitLink {
        sha: item.commit.sha.clone(),
        short_sha: item.commit.short_sha.clone(),
        title: item.commit.title.clone(),
        authored_date: item.commit.authored_date.clone(),
        repo_path: item.repo_path.clone(),
        repo_name: item.repo_name.clone(),
        business_line: item.business_line.clone(),
        match_type,
        matched_keywords: None,
        effort: effort_for_commit(&item.commit),
        confidence,
        reason,
    }
}

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
        include_stat: true,
        max_depth: 5,
    })
    .await?;

    let excluded = git_scan::load_excluded_business_lines();
    let root_dirs = raw.root_dirs.clone();

    // 过滤排除业务线后的 repo 列表
    let raw_repos: Vec<_> = raw
        .repos
        .into_iter()
        .filter(|r| !excluded.contains(&extract_business_line(&r.repo_path, &root_dirs)))
        .collect();

    let effective_total_commits: usize = raw_repos.iter().map(|r| r.commits.len()).sum();

    // 按 repo 维度展平所有 commit
    let mut items_by_repo: HashMap<String, Vec<CommitItem>> = HashMap::new();
    for r in raw_repos {
        let business_line = extract_business_line(&r.repo_path, &root_dirs);
        let repo_name = basename(&r.repo_path);
        let bucket = items_by_repo.entry(r.repo_path.clone()).or_default();
        for c in r.commits {
            bucket.push(CommitItem {
                commit: c,
                repo_path: r.repo_path.clone(),
                repo_name: repo_name.clone(),
                business_line: business_line.clone(),
            });
        }
    }

    let task_by_id: HashMap<String, &TaskInput> = tasks.iter().map(|t| (t.id.clone(), t)).collect();

    let mut task_links: HashMap<String, Vec<CommitLink>> = HashMap::new();
    // 用 (repo_path, sha) 唯一标识一条 commit；走完所有 pass 后剩下的就是孤儿
    let mut used_keys: HashSet<String> = HashSet::new();
    let make_key = |item: &CommitItem| format!("{}:{}", item.repo_path, item.commit.sha);

    // ---- Pass 1: 精确匹配 ----
    for items in items_by_repo.values() {
        for item in items {
            let ids = extract_task_ids_from_message(&item.commit);
            if ids.is_empty() {
                continue;
            }
            let mut hit_any = false;
            for id in ids {
                if !task_by_id.contains_key(&id) {
                    continue;
                }
                task_links.entry(id).or_default().push(make_link(
                    item,
                    MatchType::Exact,
                    None,
                    None,
                ));
                hit_any = true;
            }
            if hit_any {
                used_keys.insert(make_key(item));
            }
        }
    }

    // ---- Pass 2 + 3: 绑定匹配（必要时 LLM 多候选判定）----
    // 按 repo 处理，每个 repo 单独决定走哪条路径
    for (repo_path, items) in &items_by_repo {
        // 候选任务 = 绑定到该 repo 的 task_id ∩ 当前活跃任务集
        let bound_task_ids: Vec<String> = crate::task_bindings::task_ids_for_repo(repo_path)
            .into_iter()
            .filter(|tid| task_by_id.contains_key(tid))
            .collect();

        if bound_task_ids.is_empty() {
            // 该 repo 没有任何绑定任务 → Pass 1 没命中的 commit 全部进入孤儿桶
            continue;
        }

        // 这个 repo 的"待归属"commits = 全部 commits - Pass 1 已用 - 已属其它绑定任务
        let pending_items: Vec<&CommitItem> = items
            .iter()
            .filter(|it| !used_keys.contains(&make_key(it)))
            .collect();
        if pending_items.is_empty() {
            continue;
        }

        if bound_task_ids.len() == 1 {
            // 唯一候选：直接全归过去（1:1 的典型场景，省 LLM 调用）
            let task_id = bound_task_ids.into_iter().next().unwrap();
            for it in pending_items {
                task_links
                    .entry(task_id.clone())
                    .or_default()
                    .push(make_link(
                        it,
                        MatchType::Soft,
                        Some(0.9),
                        Some("绑定唯一候选任务".to_string()),
                    ));
                used_keys.insert(make_key(it));
            }
            continue;
        }

        // 多候选：尽量走 LLM；如果用户关了 use_llm，就保守留作孤儿
        if !options.use_llm {
            continue;
        }

        // 构造 LLM 候选 + 输入
        let candidates: Vec<TaskInput> = bound_task_ids
            .iter()
            .filter_map(|tid| task_by_id.get(tid).map(|t| (*t).clone()))
            .collect();
        let pending_commits: Vec<LocalCommit> =
            pending_items.iter().map(|it| it.commit.clone()).collect();

        let classification =
            match commit_classifier::classify_commits_to_tasks(&pending_commits, &candidates).await
            {
                Ok(m) => m,
                Err(e) => {
                    eprintln!(
                        "[commit_link] repo={} LLM 分类失败，归零散桶: {}",
                        repo_path, e
                    );
                    HashMap::new()
                }
            };

        for it in pending_items {
            let sha = &it.commit.sha;
            let Some(res) = classification.get(sha) else {
                continue; // LLM 没给出该 sha 的判定 → 留作孤儿
            };
            let Some(task_id) = &res.task_id else {
                continue; // LLM 说不属于任何候选 → 孤儿
            };
            if res.confidence < options.min_confidence {
                continue; // 低置信度也丢
            }
            task_links
                .entry(task_id.clone())
                .or_default()
                .push(make_link(
                    it,
                    MatchType::Soft,
                    Some(res.confidence),
                    Some(res.reason.clone()),
                ));
            used_keys.insert(make_key(it));
        }
    }

    // ---- 孤儿桶：按业务线分组 ----
    let mut orphan_map: HashMap<String, Vec<CommitLink>> = HashMap::new();
    for items in items_by_repo.values() {
        for item in items {
            if used_keys.contains(&make_key(item)) {
                continue;
            }
            orphan_map
                .entry(item.business_line.clone())
                .or_default()
                .push(make_link(item, MatchType::Soft, None, None));
        }
    }

    // 组装 tasks 输出
    let mut tasks_out: Vec<TaskCommitLinks> = Vec::new();
    for (task_id, mut commits) in task_links {
        let task = match task_by_id.get(&task_id) {
            Some(t) => *t,
            None => continue,
        };
        // exact 排前面，同类按时间倒序
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
        .map(|(business_line, commits)| OrphanGroup {
            business_line,
            commits,
        })
        .collect();

    Ok(CommitLinkResult {
        range: raw.range,
        scanned_repos: raw.scanned_repos,
        total_commits: effective_total_commits,
        tasks: tasks_out,
        orphan_commits,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn business_line_extraction() {
        let roots = vec!["D:/coding".to_string()];
        assert_eq!(
            extract_business_line("D:/coding/物流/logistics-web", &roots),
            "物流"
        );
        assert_eq!(
            extract_business_line("D:/coding/deer-flow", &roots),
            "deer-flow"
        );
        assert_eq!(
            extract_business_line("D:\\coding\\示例销售线\\example-sale-app", &roots),
            "示例销售线"
        );
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
