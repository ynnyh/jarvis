use serde::Deserialize;
use serde_json::Value;

use crate::zentao::ZentaoClient;

// ============================================================================
// get_tasks
// ============================================================================

pub async fn get_tasks(_input: Value) -> Result<Value, String> {
    let client = ZentaoClient::from_settings()?;
    let tasks = client.get_my_tasks().await?;
    Ok(Value::Array(tasks))
}

// ============================================================================
// get_classified_tasks：拉任务并按工时分类（运维 / 日常事务 / 新增功能）
// ============================================================================

pub async fn get_classified_tasks(_input: Value) -> Result<Value, String> {
    let client = ZentaoClient::from_settings()?;
    let classified = client.get_classified_tasks().await?;
    serde_json::to_value(&classified).map_err(|e| format!("序列化分类任务失败: {}", e))
}

// ============================================================================
// get_today_tasks
// ============================================================================

pub async fn get_today_tasks(_input: Value) -> Result<Value, String> {
    let client = ZentaoClient::from_settings()?;
    let tasks = client.get_my_tasks().await?;
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let filtered: Vec<Value> = tasks
        .into_iter()
        .filter(|t| {
            let dl = t.get("deadline").and_then(|v| v.as_str()).unwrap_or("");
            dl.len() >= 10 && &dl[..10] == today
        })
        .collect();
    Ok(Value::Array(filtered))
}

// ============================================================================
// get_task_detail
// ============================================================================

#[derive(Debug, Deserialize)]
struct GetTaskDetailInput {
    id: String,
}

pub async fn get_task_detail(input: Value) -> Result<Value, String> {
    let parsed: GetTaskDetailInput =
        serde_json::from_value(input).map_err(|e| format!("get_task_detail 入参错误: {}", e))?;
    if parsed.id.is_empty() {
        return Err("id 不能为空".into());
    }
    let client = ZentaoClient::from_settings()?;
    let task = client.get_task(&parsed.id).await?;
    Ok(task.unwrap_or(Value::Null))
}
