use serde::Deserialize;
use serde_json::Value;

use crate::commit_link::{self, LinkCommitsOptions, TaskInput};
use crate::git_scan::RangePreset;

// ============================================================================
// get_task_commits
// ============================================================================

#[derive(Debug, Deserialize)]
struct GetTaskCommitsInput {
    #[serde(default)]
    range: Option<String>,
    #[serde(default)]
    since: Option<String>,
    #[serde(default)]
    until: Option<String>,
    #[serde(default, rename = "rootDir")]
    root_dir: Option<Value>,
    #[serde(default, rename = "includeBody")]
    include_body: Option<bool>,
    #[serde(default, rename = "taskIds")]
    task_ids: Option<Vec<Value>>,
    #[serde(default, rename = "useLlm")]
    use_llm: Option<bool>,
}

fn coerce_root_dir(v: Option<Value>) -> Vec<String> {
    match v {
        Some(Value::String(s)) if !s.trim().is_empty() => vec![s.trim().to_string()],
        Some(Value::Array(arr)) => arr
            .into_iter()
            .filter_map(|x| x.as_str().map(|s| s.trim().to_string()))
            .filter(|s| !s.is_empty())
            .collect(),
        _ => Vec::new(),
    }
}

pub(crate) async fn get_task_commits(input: Value) -> Result<Value, String> {
    let parsed: GetTaskCommitsInput =
        serde_json::from_value(input).map_err(|e| format!("get_task_commits 入参错误: {}", e))?;

    // 1. 拉禅道任务（自己的全部）
    let client = crate::zentao::ZentaoClient::from_settings()?;
    let all_tasks = client.get_my_tasks().await?;

    // 2. 按 taskIds 过滤（如果给了）
    let filtered: Vec<Value> = if let Some(ids) = &parsed.task_ids {
        let id_strs: Vec<String> = ids.iter().map(value_to_id_string).collect();
        all_tasks
            .into_iter()
            .filter(|t| {
                let tid = t.get("id").map(value_to_id_string).unwrap_or_default();
                id_strs.iter().any(|x| x == &tid)
            })
            .collect()
    } else {
        all_tasks
    };

    let task_inputs: Vec<TaskInput> = filtered
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

    // 3. 解析 rootDir：入参优先，否则 settings.repoRoots
    let root_dirs = {
        let from_input = coerce_root_dir(parsed.root_dir.clone());
        if !from_input.is_empty() {
            from_input
        } else {
            crate::git_scan::get_repo_roots()
        }
    };
    if root_dirs.is_empty() {
        return Err("缺少 rootDir。请在设置里配置代码根目录或传 rootDir 参数".into());
    }

    let range = RangePreset::parse(parsed.range.as_deref().unwrap_or("today"));
    let options = LinkCommitsOptions {
        range,
        since: parsed.since.as_deref(),
        until: parsed.until.as_deref(),
        root_dirs: &root_dirs,
        include_body: parsed.include_body.unwrap_or(true),
        // v0.6.0 起默认 true：新流程的多候选场景只有走 LLM 才能正确归属，
        // 关掉就只剩单候选直归 + exact 显式 ID，对多任务并行的 repo 形同没用。
        // 没配 LLM 也安全 —— 分类失败时这些 commit 留作孤儿，不会乱关联。
        use_llm: parsed.use_llm.unwrap_or(true),
        min_confidence: 0.4,
    };
    let result = commit_link::link_tasks_with_commits(&task_inputs, options).await?;
    serde_json::to_value(result).map_err(|e| format!("commit_link 结果序列化失败: {}", e))
}

pub(super) fn value_to_id_string(v: &Value) -> String {
    if let Some(s) = v.as_str() {
        return s.to_string();
    }
    if let Some(n) = v.as_i64() {
        return n.to_string();
    }
    if let Some(f) = v.as_f64() {
        if f.fract() == 0.0 {
            return (f as i64).to_string();
        }
        return f.to_string();
    }
    v.to_string().trim_matches('"').to_string()
}
