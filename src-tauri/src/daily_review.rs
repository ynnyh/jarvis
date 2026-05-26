// 日报合成。
//
// 移植自 src/services/daily-review-service.ts。输入是 commit-link 结果 +
// 任务列表，输出结构化日报 + 纯文本草稿。

#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use crate::commit_link::{CommitLink, CommitLinkResult};
use crate::git_scan::{clean_commit_title, DateRange};

// ============================================================================
// 类型
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewTaskInfo {
    pub id: String,
    pub name: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdvancedTask {
    pub task_id: String,
    pub task_name: String,
    pub status: String,
    pub commit_count: usize,
    pub commits: Vec<CommitLink>,
    pub business_line: String,
    pub effort: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggested_hours: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BusinessLineGroup {
    pub business_line: String,
    pub commits: Vec<CommitLink>,
    pub tasks: Vec<TaskRef>,
    pub effort: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggested_hours: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRef {
    pub task_id: String,
    pub task_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NeedsStatusUpdate {
    pub task_id: String,
    pub task_name: String,
    pub commit_count: usize,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrphanCommitGroup {
    pub business_line: String,
    pub commits: Vec<CommitLink>,
    pub effort: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggested_hours: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyReviewSummary {
    pub total_commits: usize,
    pub business_line_count: usize,
    pub tasks_advanced_count: usize,
    pub orphan_commit_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyReview {
    pub date: String,
    pub range: DateRange,
    pub summary: DailyReviewSummary,
    pub advanced_tasks: Vec<AdvancedTask>,
    pub by_business_line: Vec<BusinessLineGroup>,
    pub needs_status_update: Vec<NeedsStatusUpdate>,
    pub orphan_commits: Vec<OrphanCommitGroup>,
    pub total_hours_for_estimate: f64,
    pub plain_text: String,
}

pub struct BuildOptions<'a> {
    pub date: Option<&'a str>,
    pub hours_per_work_day: f64,
}

// ============================================================================
// 主入口
// ============================================================================

fn today_str() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

fn round_half(x: f64) -> f64 {
    (x * 2.0).round() / 2.0
}

/// 业务线工时按 0.5h 量化分配给任务（commit 数倒序）
fn allocate_hours_by_slots(business_hours: f64, task_count: usize) -> Vec<f64> {
    let slots = (business_hours * 2.0).round() as i64;
    if slots <= 0 || task_count == 0 {
        return vec![0.0; task_count];
    }
    let slots = slots as usize;
    if slots >= task_count {
        let base = slots / task_count;
        let extra = slots - base * task_count;
        (0..task_count)
            .map(|i| (base + if i < extra { 1 } else { 0 }) as f64 * 0.5)
            .collect()
    } else {
        (0..task_count).map(|i| if i < slots { 0.5 } else { 0.0 }).collect()
    }
}

pub fn build_daily_review(
    link_result: CommitLinkResult,
    tasks: &[ReviewTaskInfo],
    options: BuildOptions<'_>,
) -> DailyReview {
    let date = options.date.map(|s| s.to_string()).unwrap_or_else(today_str);
    let hours_per_work_day = options.hours_per_work_day;

    let task_by_id: HashMap<String, &ReviewTaskInfo> = tasks.iter().map(|t| (t.id.clone(), t)).collect();

    // ---- 推进的任务 ----
    let mut advanced_tasks: Vec<AdvancedTask> = link_result
        .tasks
        .iter()
        .filter(|t| !t.commits.is_empty())
        .map(|t| {
            // 主业务线：commit 最多的
            let mut bl_count: HashMap<String, usize> = HashMap::new();
            for c in &t.commits {
                *bl_count.entry(c.business_line.clone()).or_insert(0) += 1;
            }
            let business_line = bl_count
                .into_iter()
                .max_by_key(|(_, n)| *n)
                .map(|(k, _)| k)
                .unwrap_or_default();
            let effort: f64 = t.commits.iter().filter(|c| c.business_line == business_line).map(|c| c.effort).sum();
            let status = task_by_id
                .get(&t.task_id)
                .map(|x| x.status.clone())
                .unwrap_or_else(|| "unknown".to_string());
            AdvancedTask {
                task_id: t.task_id.clone(),
                task_name: t.task_name.clone(),
                status,
                commit_count: t.commits.len(),
                commits: t.commits.clone(),
                business_line,
                effort,
                suggested_hours: None,
            }
        })
        .collect();
    advanced_tasks.sort_by(|a, b| b.effort.partial_cmp(&a.effort).unwrap_or(std::cmp::Ordering::Equal));

    // ---- 按业务线分组 ----
    let mut by_line_map: HashMap<String, BusinessLineGroup> = HashMap::new();
    let ensure_group = |map: &mut HashMap<String, BusinessLineGroup>, bl: &str| {
        map.entry(bl.to_string()).or_insert_with(|| BusinessLineGroup {
            business_line: bl.to_string(),
            commits: Vec::new(),
            tasks: Vec::new(),
            effort: 0.0,
            suggested_hours: None,
        });
    };
    let collect_commit = |map: &mut HashMap<String, BusinessLineGroup>, bl: &str, c: &CommitLink| {
        ensure_group(map, bl);
        let g = map.get_mut(bl).unwrap();
        let dup = g.commits.iter().any(|x| x.sha == c.sha && x.repo_path == c.repo_path);
        if !dup {
            g.commits.push(c.clone());
            g.effort += c.effort;
        }
    };
    let collect_task = |map: &mut HashMap<String, BusinessLineGroup>, bl: &str, task_id: &str, task_name: &str| {
        ensure_group(map, bl);
        let g = map.get_mut(bl).unwrap();
        if !g.tasks.iter().any(|t| t.task_id == task_id) {
            g.tasks.push(TaskRef {
                task_id: task_id.to_string(),
                task_name: task_name.to_string(),
            });
        }
    };
    for t in &link_result.tasks {
        for c in &t.commits {
            collect_commit(&mut by_line_map, &c.business_line, c);
            collect_task(&mut by_line_map, &c.business_line, &t.task_id, &t.task_name);
        }
    }
    for o in &link_result.orphan_commits {
        for c in &o.commits {
            collect_commit(&mut by_line_map, &o.business_line, c);
        }
    }
    let mut by_business_line: Vec<BusinessLineGroup> = by_line_map.into_values().collect();
    for g in &mut by_business_line {
        g.commits.sort_by(|a, b| b.authored_date.cmp(&a.authored_date));
    }
    by_business_line.sort_by(|a, b| b.effort.partial_cmp(&a.effort).unwrap_or(std::cmp::Ordering::Equal));

    // ---- 工时分配 ----
    let total_effort: f64 = by_business_line.iter().map(|g| g.effort).sum();
    if hours_per_work_day > 0.0 && total_effort > 0.0 {
        for g in &mut by_business_line {
            let raw = g.effort / total_effort * hours_per_work_day;
            g.suggested_hours = Some(round_half(raw));
        }
        // 按 0.5h 槽位分到任务
        let lines_with_tasks: Vec<String> = by_business_line
            .iter()
            .filter(|g| !g.tasks.is_empty())
            .map(|g| g.business_line.clone())
            .collect();
        for bl in lines_with_tasks {
            let business_hours = by_business_line
                .iter()
                .find(|g| g.business_line == bl)
                .and_then(|g| g.suggested_hours)
                .unwrap_or(0.0);
            if business_hours <= 0.0 {
                continue;
            }
            let mut line_tasks_indices: Vec<usize> = advanced_tasks
                .iter()
                .enumerate()
                .filter(|(_, t)| t.business_line == bl)
                .map(|(i, _)| i)
                .collect();
            // 按 effort desc
            line_tasks_indices.sort_by(|&a, &b| {
                advanced_tasks[b]
                    .effort
                    .partial_cmp(&advanced_tasks[a].effort)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            let allocations = allocate_hours_by_slots(business_hours, line_tasks_indices.len());
            for (i, alloc) in line_tasks_indices.iter().zip(allocations.iter()) {
                advanced_tasks[*i].suggested_hours = Some(*alloc);
            }
        }
    }

    // ---- 状态需要更新的任务 ----
    let needs_status_update: Vec<NeedsStatusUpdate> = advanced_tasks
        .iter()
        .filter(|t| t.status == "wait")
        .map(|t| NeedsStatusUpdate {
            task_id: t.task_id.clone(),
            task_name: t.task_name.clone(),
            commit_count: t.commit_count,
            reason: format!("本地已有 {} 个 commit，但禅道状态仍是\"未开始\"", t.commit_count),
        })
        .collect();

    // ---- 概况 ----
    let summary = DailyReviewSummary {
        total_commits: link_result.total_commits,
        business_line_count: by_business_line.len(),
        tasks_advanced_count: advanced_tasks.len(),
        orphan_commit_count: link_result.orphan_commits.iter().map(|o| o.commits.len()).sum(),
    };

    // ---- 孤儿 commit 估工时 ----
    let orphan_commit_groups: Vec<OrphanCommitGroup> = link_result
        .orphan_commits
        .iter()
        .map(|o| {
            let effort: f64 = o.commits.iter().map(|c| c.effort).sum();
            let suggested_hours = if hours_per_work_day > 0.0 && total_effort > 0.0 {
                Some(round_half(effort / total_effort * hours_per_work_day))
            } else {
                None
            };
            OrphanCommitGroup {
                business_line: o.business_line.clone(),
                commits: o.commits.clone(),
                effort,
                suggested_hours,
            }
        })
        .collect();

    let plain_text = render_plain_text(
        &date,
        &summary,
        &advanced_tasks,
        &by_business_line,
        &needs_status_update,
        &orphan_commit_groups,
        hours_per_work_day,
    );

    DailyReview {
        date,
        range: link_result.range,
        summary,
        advanced_tasks,
        by_business_line,
        needs_status_update,
        orphan_commits: orphan_commit_groups,
        total_hours_for_estimate: hours_per_work_day,
        plain_text,
    }
}

// ============================================================================
// 纯文本渲染
// ============================================================================

fn render_plain_text(
    date: &str,
    summary: &DailyReviewSummary,
    advanced_tasks: &[AdvancedTask],
    by_line: &[BusinessLineGroup],
    needs_update: &[NeedsStatusUpdate],
    orphan_groups: &[OrphanCommitGroup],
    hours_per_work_day: f64,
) -> String {
    let mut lines: Vec<String> = Vec::new();
    lines.push(format!("工作日报 {}", date));
    lines.push(String::new());

    if summary.total_commits == 0 {
        lines.push("今天没有本地提交。如有未推送或外部协作，请手动补充。".into());
        return lines.join("\n");
    }

    lines.push(format!(
        "今天共提交 {} 个 commit，覆盖 {} 个业务线，推进 {} 个任务。",
        summary.total_commits, summary.business_line_count, summary.tasks_advanced_count
    ));
    lines.push(String::new());

    lines.push("【完成内容】".into());
    lines.push(String::new());
    for g in by_line {
        if g.commits.is_empty() {
            continue;
        }
        let mut seen: HashSet<String> = HashSet::new();
        let mut unique_by_title: Vec<(&CommitLink, String)> = Vec::new();
        for c in &g.commits {
            let cleaned = clean_commit_title(&c.title, 60);
            if cleaned.is_empty() || seen.contains(&cleaned) {
                continue;
            }
            seen.insert(cleaned.clone());
            unique_by_title.push((c, cleaned));
        }
        let dup_count = g.commits.len() - unique_by_title.len();
        let count_label = if dup_count > 0 {
            format!("{} 个主题 / 共 {} 次提交", unique_by_title.len(), g.commits.len())
        } else {
            format!("{} 个 commit", g.commits.len())
        };
        lines.push(format!("{}（{}）", g.business_line, count_label));
        for (c, cleaned) in &unique_by_title {
            lines.push(format!("  · {}  ({} · {})", cleaned, c.repo_name, c.short_sha));
        }
        if !g.tasks.is_empty() {
            lines.push("  推进任务：".into());
            for t in &g.tasks {
                let adv = advanced_tasks.iter().find(|a| a.task_id == t.task_id);
                let status_mark = if adv.map(|a| a.status == "wait").unwrap_or(false) {
                    "（未开始）"
                } else {
                    ""
                };
                lines.push(format!("    - #{} {}{}", t.task_id, t.task_name, status_mark));
            }
        }
        lines.push(String::new());
    }

    // 建议工时
    if hours_per_work_day > 0.0 {
        let lines_with_hours: Vec<&BusinessLineGroup> = by_line
            .iter()
            .filter(|g| g.suggested_hours.unwrap_or(0.0) > 0.0)
            .collect();
        if !lines_with_hours.is_empty() {
            lines.push("【建议工时分配】（最小粒度 0.5h，仅供禅道填报参考）".into());
            for g in &lines_with_hours {
                let line_tasks: Vec<&AdvancedTask> = advanced_tasks.iter().filter(|t| t.business_line == g.business_line).collect();
                let tasks_with_hours: Vec<&&AdvancedTask> = line_tasks.iter().filter(|t| t.suggested_hours.unwrap_or(0.0) > 0.0).collect();
                lines.push(format!(
                    "  · {}：{}h（{}/{} 个主要任务）",
                    g.business_line,
                    g.suggested_hours.unwrap_or(0.0),
                    tasks_with_hours.len(),
                    line_tasks.len()
                ));
                for t in tasks_with_hours {
                    lines.push(format!("    - #{} {}：{}h", t.task_id, t.task_name, t.suggested_hours.unwrap_or(0.0)));
                }
            }
            lines.push("  注：commit 数少的次要任务未分配工时，主要任务工时合计为日总工时。".into());
            lines.push(String::new());
        }
    }

    if !needs_update.is_empty() {
        lines.push("【需要在禅道更新状态的任务】".into());
        for t in needs_update {
            lines.push(format!("  · #{} {}：{}", t.task_id, t.task_name, t.reason));
        }
        lines.push(String::new());
    }

    let non_empty_orphans: Vec<&OrphanCommitGroup> = orphan_groups.iter().filter(|o| !o.commits.is_empty()).collect();
    if !non_empty_orphans.is_empty() {
        lines.push("【未关联禅道任务的提交】（建议补任务号或在日报中说明）".into());
        for g in non_empty_orphans {
            let mut seen: HashSet<String> = HashSet::new();
            let mut unique: Vec<(&CommitLink, String)> = Vec::new();
            for c in &g.commits {
                let cleaned = clean_commit_title(&c.title, 60);
                if cleaned.is_empty() || seen.contains(&cleaned) {
                    continue;
                }
                seen.insert(cleaned.clone());
                unique.push((c, cleaned));
            }
            let hours_label = match g.suggested_hours {
                Some(h) if h > 0.0 => format!("，建议 ~{}h", h),
                _ => String::new(),
            };
            lines.push(format!("  · {}（{} 个主题{}）", g.business_line, unique.len(), hours_label));
            for (c, cleaned) in &unique {
                lines.push(format!("    - {}  ({} · {})", cleaned, c.repo_name, c.short_sha));
            }
        }
        lines.push(String::new());
    }

    lines.join("\n").trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocate_slots() {
        // 业务线 3h，2 任务：slots=6，每任务 1.5h
        assert_eq!(allocate_hours_by_slots(3.0, 2), vec![1.5, 1.5]);
        // 1h，3 任务：slots=2，前两个 0.5h，第三个 0
        assert_eq!(allocate_hours_by_slots(1.0, 3), vec![0.5, 0.5, 0.0]);
        // 0h
        assert_eq!(allocate_hours_by_slots(0.0, 3), vec![0.0, 0.0, 0.0]);
    }

    #[test]
    fn round_half_basic() {
        assert_eq!(round_half(0.3), 0.5);
        assert_eq!(round_half(0.74), 0.5);
        assert_eq!(round_half(1.26), 1.5);
        assert_eq!(round_half(0.0), 0.0);
    }
}
