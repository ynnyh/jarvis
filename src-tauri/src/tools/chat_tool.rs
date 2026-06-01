use serde::Deserialize;
use serde_json::{json, Value};

use crate::llm::{ChatMessage, Role};

// ============================================================================
// chat_send
// ============================================================================

#[derive(Debug, Deserialize)]
struct ChatSendInput {
    messages: Vec<ChatSendMessage>,
    #[serde(default, rename = "assistantName")]
    assistant_name: Option<String>,
    #[serde(default, rename = "userTitle")]
    user_title: Option<String>,
    #[serde(default, rename = "maxIterations")]
    max_iterations: Option<u32>,
    #[serde(default)]
    temperature: Option<f32>,
    #[serde(default, rename = "allowedTools")]
    allowed_tools: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct ChatSendMessage {
    role: String,
    content: String,
    #[serde(default)]
    tool_calls: Option<Vec<Value>>,
    #[serde(default)]
    tool_call_id: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

pub(crate) async fn chat_send(input: Value) -> Result<Value, String> {
    let parsed: ChatSendInput =
        serde_json::from_value(input).map_err(|e| format!("chat_send 入参错误: {}", e))?;
    if parsed.messages.is_empty() {
        return Err("messages 不能为空".into());
    }

    let allowed_tools: Vec<String> = parsed
        .allowed_tools
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| {
            crate::chat_agent::DEFAULT_AGENT_TOOLS
                .iter()
                .map(|s| s.to_string())
                .collect()
        });

    let has_system = parsed
        .messages
        .first()
        .map(|m| m.role == "system")
        .unwrap_or(false);
    let system_prompt = if has_system {
        None
    } else {
        Some(crate::chat_agent::default_system_prompt(
            parsed.assistant_name.as_deref().unwrap_or("Jarvis"),
            parsed.user_title.as_deref().unwrap_or("主人"),
        ))
    };

    let messages: Vec<ChatMessage> = parsed
        .messages
        .into_iter()
        .map(|m| ChatMessage {
            role: match m.role.as_str() {
                "system" => Role::System,
                "assistant" => Role::Assistant,
                "tool" => Role::Tool,
                _ => Role::User,
            },
            content: m.content,
            tool_calls: m
                .tool_calls
                .and_then(|tc| serde_json::from_value(Value::Array(tc)).ok()),
            tool_call_id: m.tool_call_id,
            name: m.name,
        })
        .collect();

    let result = crate::chat_agent::run_agent(crate::chat_agent::RunAgentOptions {
        messages,
        allowed_tools: &allowed_tools,
        max_iterations: parsed.max_iterations.unwrap_or(8),
        temperature: parsed.temperature.unwrap_or(0.3),
        max_tokens: 2048,
        system_prompt,
    })
    .await;

    Ok(json!({
        "newMessages": result.new_messages,
        "steps": result.steps,
        "tokensIn": result.tokens_in,
        "tokensOut": result.tokens_out,
        "truncated": result.truncated,
        "allowedTools": allowed_tools,
    }))
}
