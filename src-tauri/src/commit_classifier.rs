// LLM 服务 B：批量把 commits 归类到一组候选任务里。
//
// 使用场景：commit_link 改造后（task #87），对一条 commit：
// 1. 先查 message 里显式的任务号 → 命中直接归属，不调 LLM
// 2. 否则，按 commit 所在 repo 反查 task_bindings 拿到候选任务集合
//    - 集合为空 → 归到"零散修复"桶
//    - 集合 == 1 → 直接归属（这是 1:1 绑定的常见路径，省 LLM 调用）
//    - 集合 > 1 → 调本模块，让 LLM 在多个候选里选
//
// 设计要点：
// - 批量调用：一次 prompt 塞多条 commit，输出 JSON 数组。N=30 commit × 10 task
//   候选≈9k tokens，DeepSeek 单次 128k 上下文绰绰有余。
// - "none" 兜底：当 LLM 觉得 commit 不属于任何候选任务时，返回 task_index=0。
//   commit_link 拿到 None 后归到孤儿桶 + 触发兜底建议事件。
// - confidence 字段保留给 #87，调用方可以按阈值过滤（比如 < 0.4 当 none 处理）。
//
// 性能注记：
// - 一次 LLM 调用 5-30s，所以"日报刷新"时这里会显著拖慢。
// - 建议在 commit_link 上层加缓存：key = sha + candidate_task_ids_set hash，
//   命中就直接读缓存。但缓存在 #87 里加，这层只做无状态的 LLM 调用。

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::commit_link::TaskInput;
use crate::git_scan::LocalCommit;
use crate::llm::{self, ChatMessage, ChatRequest, Role};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitClassification {
    /// None 表示 LLM 觉得这条 commit 不属于任何候选任务（孤儿）
    pub task_id: Option<String>,
    /// 0.0 ~ 1.0
    pub confidence: f64,
    pub reason: String,
}

/// 单次 LLM 调用承载的 commit 上限。超过就拆批。
const MAX_COMMITS_PER_CALL: usize = 30;

/// 主入口。返回 sha → ClassificationResult 的映射。
/// candidates 为空时直接返回空 map，不调 LLM。
#[allow(dead_code)]
pub async fn classify_commits_to_tasks(
    commits: &[LocalCommit],
    candidates: &[TaskInput],
) -> Result<HashMap<String, CommitClassification>, String> {
    if candidates.is_empty() || commits.is_empty() {
        return Ok(HashMap::new());
    }

    let mut result = HashMap::new();
    for batch in commits.chunks(MAX_COMMITS_PER_CALL) {
        match classify_batch(batch, candidates).await {
            Ok(map) => {
                for (k, v) in map {
                    result.insert(k, v);
                }
            }
            Err(e) => {
                eprintln!("[classify_commits_to_tasks] 批次失败，跳过: {}", e);
                // 失败的批次保留为"未分类"，调用方按需处理
            }
        }
    }
    Ok(result)
}

async fn classify_batch(
    commits: &[LocalCommit],
    candidates: &[TaskInput],
) -> Result<HashMap<String, CommitClassification>, String> {
    let prompt = build_prompt(commits, candidates);

    let req = ChatRequest {
        messages: vec![
            ChatMessage {
                role: Role::System,
                content: "你是代码提交与任务关联的分类助手。给定一组候选任务和一组 commit，\
判断每条 commit 最可能归属到哪个候选任务（或者归属不到任何任务，记为 0）。\
只输出 JSON 数组，不要 ```json 包裹，不要任何解释。"
                    .to_string(),
                tool_calls: None,
                tool_call_id: None,
                name: None,
            },
            ChatMessage {
                role: Role::User,
                content: prompt,
                tool_calls: None,
                tool_call_id: None,
                name: None,
            },
        ],
        temperature: Some(0.1),
        max_tokens: Some(((commits.len() as u32) * 80 + 200).min(4000)),
        model: None,
        timeout_ms: Some(60_000),
        tools: None,
        tool_choice: None,
    };

    let resp = llm::chat(req).await?;
    parse_response(&resp.text, candidates)
}

fn build_prompt(commits: &[LocalCommit], candidates: &[TaskInput]) -> String {
    let mut s = String::new();
    s.push_str("候选任务（taskIndex 0 表示「不属于任何候选任务」）：\n");
    for (i, t) in candidates.iter().enumerate() {
        let name: String = t.name.chars().take(80).collect();
        s.push_str(&format!("[{}] #{} {}\n", i + 1, t.id, name));
    }
    s.push_str("\n提交列表：\n");
    for c in commits {
        let title: String = c.title.chars().take(120).collect();
        s.push_str(&format!("- sha={} title=\"{}\"", c.short_sha, title));
        // 文件清单只取前 5 个，避免巨型变更把 prompt 撑爆
        if let Some(stat) = &c.stat {
            if !stat.files.is_empty() {
                let files: Vec<String> = stat
                    .files
                    .iter()
                    .take(5)
                    .map(|f| {
                        // 取相对路径的最后 2 段，足够 LLM 看出涉及什么模块
                        let parts: Vec<&str> = f.path.split('/').collect();
                        if parts.len() <= 2 {
                            f.path.clone()
                        } else {
                            parts[parts.len() - 2..].join("/")
                        }
                    })
                    .collect();
                s.push_str(&format!(" files=[{}]", files.join(",")));
            }
        }
        s.push('\n');
    }
    s.push_str(
        "\n请对每条提交返回一项 JSON：\n  - sha: 上面 sha 原文\n  - taskIndex: 1-based 候选序号（0 表示不属于任何候选任务）\n  - confidence: 0.0~1.0 两位小数\n  - reason: 一句话中文理由（<=25 字）\n\
示例：[{\"sha\":\"abc123\",\"taskIndex\":2,\"confidence\":0.78,\"reason\":\"修改了任务 2 提到的登录页\"}]\n",
    );
    s
}

fn parse_response(
    text: &str,
    candidates: &[TaskInput],
) -> Result<HashMap<String, CommitClassification>, String> {
    let trimmed = text.trim();
    // 容错：去掉 ```json fences
    let stripped = if trimmed.starts_with("```") {
        let after_first = trimmed
            .find('\n')
            .map(|i| &trimmed[i + 1..])
            .unwrap_or(trimmed);
        after_first
            .rfind("```")
            .map(|i| &after_first[..i])
            .unwrap_or(after_first)
            .trim()
    } else {
        trimmed
    };
    let (start, end) = (stripped.find('['), stripped.rfind(']'));
    let json_str = match (start, end) {
        (Some(s), Some(e)) if e > s => &stripped[s..=e],
        _ => {
            return Err(format!(
                "LLM 输出不含 JSON 数组: {}",
                crate::util::truncate_chars(&text, 200)
            ))
        }
    };
    // 截断的响应（只有 "[" 或内容被 max_tokens 切断）导致 JSON 解析失败时，
    // 返回空结果而不是报错——这些 commit 留为"未分类"，下次发版重试即可。
    let arr: Vec<serde_json::Value> = serde_json::from_str(json_str).unwrap_or_else(|e| {
        eprintln!(
            "[classify_commits_to_tasks] JSON 解析失败({}): {}",
            e,
            crate::util::truncate_chars(json_str, 200)
        );
        Vec::new()
    });

    let mut out = HashMap::new();
    for item in arr {
        let sha = item
            .get("sha")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if sha.is_empty() {
            continue;
        }
        let task_index = item.get("taskIndex").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let confidence = item
            .get("confidence")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0)
            .clamp(0.0, 1.0);
        let reason = item
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let task_id = if task_index == 0 || task_index > candidates.len() {
            None
        } else {
            Some(candidates[task_index - 1].id.clone())
        };
        out.insert(
            sha,
            CommitClassification {
                task_id,
                confidence,
                reason,
            },
        );
    }
    Ok(out)
}
