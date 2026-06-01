/// chat_send_stream — 流式聊天命令，绕开 tool_execute 直接调 agent loop。
///
/// 与 chat_send tool 的输入相同，但额外接收 AppHandle 用于发射事件：
///   chat:stream — 事件体是 chat_agent::StreamEvent（JSON，含 type 标签）

use std::sync::Arc;

use serde::Deserialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

use crate::chat_agent::{self, StreamEvent};
use crate::llm::{ChatMessage, Role};

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

#[tauri::command]
pub async fn chat_send_stream(
    app: AppHandle,
    input: Value,
) -> Result<Value, String> {
    let parsed: ChatSendInput =
        serde_json::from_value(input).map_err(|e| format!("chat_send_stream 入参错误: {}", e))?;
    if parsed.messages.is_empty() {
        return Err("messages 不能为空".into());
    }

    let allowed_tools: Vec<String> = parsed
        .allowed_tools
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| {
            chat_agent::DEFAULT_AGENT_TOOLS
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
        Some(chat_agent::default_system_prompt(
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

    // Build the event emitter callback (Arc<dyn Fn>)
    let on_event: Arc<dyn Fn(StreamEvent) + Send + Sync> = {
        let app = app.clone();
        Arc::new(move |event| {
            // 事件名固定为 chat:stream；前端用 listen('chat:stream', ...) 监听
            let _ = app.emit("chat:stream", &event);
        })
    };

    let result = chat_agent::run_agent_streaming(
        chat_agent::RunAgentOptions {
            messages,
            allowed_tools: &allowed_tools,
            max_iterations: parsed.max_iterations.unwrap_or(8),
            temperature: parsed.temperature.unwrap_or(0.3),
            max_tokens: 2048,
            system_prompt,
        },
        on_event,
    )
    .await;

    // Emit done event
    let _ = app.emit(
        "chat:stream",
        StreamEvent::Done {
            tokens_in: result.tokens_in,
            tokens_out: result.tokens_out,
            truncated: result.truncated,
        },
    );

    Ok(json!({
        "newMessages": result.new_messages,
        "steps": result.steps,
        "tokensIn": result.tokens_in,
        "tokensOut": result.tokens_out,
        "truncated": result.truncated,
        "allowedTools": allowed_tools,
    }))
}
