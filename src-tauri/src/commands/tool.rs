/// Tool 调度入口

use crate::commands::ToolResult;

#[tauri::command]
pub async fn tool_execute(
    name: String,
    input: Option<serde_json::Value>,
) -> Result<ToolResult, String> {
    let input = input.unwrap_or(serde_json::json!({}));
    match crate::tools::dispatch(&name, input).await {
        Ok(data) => Ok(ToolResult {
            success: true,
            data: Some(data),
            error: None,
        }),
        Err(e) => Ok(ToolResult {
            success: false,
            data: None,
            error: Some(e),
        }),
    }
}
