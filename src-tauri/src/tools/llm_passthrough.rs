use serde::Deserialize;
use serde_json::{json, Value};

use crate::llm::{self, ChatMessage, ChatRequest, Role};

// ============================================================================
// ask-llm
// ============================================================================

#[derive(Debug, Deserialize)]
struct AskLlmInput {
    messages: Vec<AskLlmMessage>,
    temperature: Option<f32>,
    #[serde(rename = "maxTokens")]
    max_tokens: Option<u32>,
    model: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AskLlmMessage {
    role: String,
    content: String,
}

pub(crate) async fn ask_llm(input: Value) -> Result<Value, String> {
    let parsed: AskLlmInput =
        serde_json::from_value(input).map_err(|e| format!("ask-llm 入参错误: {}", e))?;
    if parsed.messages.is_empty() {
        return Err("messages 不能为空".into());
    }

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
            tool_calls: None,
            tool_call_id: None,
            name: None,
        })
        .collect();

    let mut req = ChatRequest::new(messages);
    req.temperature = parsed.temperature;
    req.max_tokens = parsed.max_tokens;
    req.model = parsed.model;

    let resp = llm::chat(req).await?;
    Ok(json!({
        "text": resp.text,
        "tokensIn": resp.tokens_in,
        "tokensOut": resp.tokens_out,
        "model": resp.model,
    }))
}
