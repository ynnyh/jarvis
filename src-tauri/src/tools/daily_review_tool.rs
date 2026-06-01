use serde::Deserialize;
use serde_json::{json, Value};

use crate::commit_link::{self, LinkCommitsOptions, TaskInput};
use crate::daily_review::{self, BuildOptions, ReviewTaskInfo};
use crate::git_scan::RangePreset;
use crate::llm::{self, ChatMessage, ChatRequest, Role};

use super::task_commits::value_to_id_string;

// ============================================================================
// get_daily_review
// ============================================================================

#[derive(Debug, Deserialize)]
struct GetDailyReviewInput {
    #[serde(default)]
    range: Option<String>,
    #[serde(default)]
    since: Option<String>,
    #[serde(default)]
    until: Option<String>,
    #[serde(default)]
    date: Option<String>,
    #[serde(default, rename = "hoursPerWorkDay")]
    hours_per_work_day: Option<f64>,
    #[serde(default, rename = "useLlm")]
    use_llm: Option<bool>,
}

pub(crate) async fn get_daily_review(input: Value) -> Result<Value, String> {
    let parsed: GetDailyReviewInput =
        serde_json::from_value(input).map_err(|e| format!("get_daily_review 入参错误: {}", e))?;

    // 1. 拉禅道任务
    let client = crate::zentao::ZentaoClient::from_settings()?;
    let all_tasks = client.get_my_tasks().await?;

    let task_inputs: Vec<TaskInput> = all_tasks
        .iter()
        .map(|t| TaskInput {
            id: t.get("id").map(value_to_id_string).unwrap_or_default(),
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
            id: t.get("id").map(value_to_id_string).unwrap_or_default(),
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

    // 2. 拉 commit + 关联
    let root_dirs = crate::git_scan::get_repo_roots();
    if root_dirs.is_empty() {
        return Err("缺少 rootDir。请在设置里配置代码根目录".into());
    }
    let range = RangePreset::parse(parsed.range.as_deref().unwrap_or("today"));
    let link_result = commit_link::link_tasks_with_commits(
        &task_inputs,
        LinkCommitsOptions {
            range,
            since: parsed.since.as_deref(),
            until: parsed.until.as_deref(),
            root_dirs: &root_dirs,
            include_body: true,
            // 同 get_task_commits：v0.6.0 起 LLM 默认开，确保多候选 commit 能归属
            use_llm: parsed.use_llm.unwrap_or(true),
            min_confidence: 0.4,
        },
    )
    .await?;

    // 3. 合成日报
    let review = daily_review::build_daily_review(
        link_result,
        &review_tasks,
        BuildOptions {
            date: parsed.date.as_deref(),
            hours_per_work_day: parsed.hours_per_work_day.unwrap_or(0.0),
        },
    );

    // 4. 可选 LLM 改写 plainText
    if parsed.use_llm.unwrap_or(false) {
        match rewrite_daily_with_llm(&review).await {
            Ok(text) => {
                let mut v = serde_json::to_value(&review).map_err(|e| e.to_string())?;
                if let Value::Object(map) = &mut v {
                    map.insert(
                        "plainTextHeuristic".into(),
                        Value::String(review.plain_text.clone()),
                    );
                    map.insert("plainText".into(), Value::String(text));
                    map.insert("llmUsed".into(), Value::Bool(true));
                }
                return Ok(v);
            }
            Err(e) => {
                let mut v = serde_json::to_value(&review).map_err(|e| e.to_string())?;
                if let Value::Object(map) = &mut v {
                    map.insert("llmUsed".into(), Value::Bool(false));
                    map.insert("llmError".into(), Value::String(e));
                }
                return Ok(v);
            }
        }
    }
    serde_json::to_value(&review).map_err(|e| format!("daily review 序列化失败: {}", e))
}

async fn rewrite_daily_with_llm(review: &daily_review::DailyReview) -> Result<String, String> {
    let summary_payload = json!({
        "date": review.date,
        "totalCommits": review.summary.total_commits,
        "advancedTasks": review.advanced_tasks.iter().map(|t| json!({
            "id": t.task_id,
            "name": t.task_name,
            "status": t.status,
            "businessLine": t.business_line,
            "commitCount": t.commit_count,
            "suggestedHours": t.suggested_hours,
        })).collect::<Vec<_>>(),
        "byBusinessLine": review.by_business_line.iter().map(|g| {
            let titles: std::collections::HashSet<String> = g.commits.iter().map(|c| c.title.clone()).collect();
            let titles_vec: Vec<String> = titles.into_iter().take(20).collect();
            json!({
                "businessLine": g.business_line,
                "commitCount": g.commits.len(),
                "taskCount": g.tasks.len(),
                "suggestedHours": g.suggested_hours,
                "commitTitles": titles_vec,
            })
        }).collect::<Vec<_>>(),
        "needsStatusUpdate": review.needs_status_update.iter().map(|n| json!({
            "id": n.task_id, "name": n.task_name, "commitCount": n.commit_count,
        })).collect::<Vec<_>>(),
        "orphanCommitCount": review.summary.orphan_commit_count,
        "totalHoursForEstimate": review.total_hours_for_estimate,
    });
    let messages = vec![
        ChatMessage {
            role: Role::System,
            content: "你是一个简洁的日报助手。基于结构化的当日工作数据生成自然语言日报。\n\
强约束：\n\
1. 完全去技术化——不出现 commit/sha/repo/PR/branch 等词，commit 标题原样使用（不要加技术修饰）\n\
2. 不要凭空发挥，所有内容必须能从输入数据里找到依据\n\
3. 用项目维度（业务线）+ 任务推进 + 需要补登/更新状态的事项 组织文章\n\
4. 整体写成一段话，不要换行分段或分节。多个并列事项用数字编号列举（如 1.xxx 2.xxx 3.xxx），不要用横杠或星号\n\
5. 输出纯文本，不要 Markdown 符号"
                .into(),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        },
        ChatMessage {
            role: Role::User,
            content: format!(
                "请基于以下结构化数据，写一份 {} 的工作日报（中文，纯文本）：\n\n```json\n{}\n```",
                review.date,
                serde_json::to_string_pretty(&summary_payload).unwrap_or_default()
            ),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        },
    ];
    let mut req = ChatRequest::new(messages);
    req.temperature = Some(0.4);
    req.max_tokens = Some(1500);
    let resp = llm::chat(req).await?;
    Ok(resp.text.trim().to_string())
}
