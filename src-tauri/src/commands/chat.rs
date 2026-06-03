/// chat_send_stream — 流式聊天命令，绕开 tool_execute 直接调 agent loop。
///
/// 与 chat_send tool 的输入相同，但额外接收 AppHandle 用于发射事件：
///   chat:stream — 事件体是 chat_agent::StreamEvent（JSON，含 type 标签）

use std::sync::Arc;

use serde::Deserialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, State};

use crate::chat_agent::{self, StreamEvent};
use crate::llm::{ChatMessage, Role};
use crate::memory::MemoryState;

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
    #[serde(default, rename = "conversationId")]
    conversation_id: Option<String>,
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
    memory_state: State<'_, MemoryState>,
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
    let base_prompt = if has_system {
        String::new()
    } else {
        chat_agent::default_system_prompt(
            parsed.assistant_name.as_deref().unwrap_or("Jarvis"),
            parsed.user_title.as_deref().unwrap_or("主人"),
        )
    };

    let messages: Vec<ChatMessage> = parsed
        .messages
        .iter()
        .map(|m| ChatMessage {
            role: match m.role.as_str() {
                "system" => Role::System,
                "assistant" => Role::Assistant,
                "tool" => Role::Tool,
                _ => Role::User,
            },
            content: m.content.clone(),
            tool_calls: m
                .tool_calls
                .as_ref()
                .and_then(|tc| serde_json::from_value(Value::Array(tc.clone())).ok()),
            tool_call_id: m.tool_call_id.clone(),
            name: m.name.clone(),
        })
        .collect();

    // 动态注入记忆到 system prompt
    let last_user_msg = messages
        .iter()
        .rev()
        .find(|m| matches!(m.role, Role::User))
        .map(|m| m.content.clone())
        .unwrap_or_default();

    // 1. 同步读 Core Memory
    let core_section = memory_state
        .db
        .as_ref()
        .and_then(|m| m.lock().ok())
        .map(|db| crate::memory::build_core_prompt(&db))
        .unwrap_or_default();

    // 2. 异步计算嵌入（不持锁）
    let query_embedding = crate::memory::compute_query_embedding(&last_user_msg).await;

    // 3. 用嵌入检索 Long-term Memory（短暂持锁）
    let longterm_section = match (&memory_state.db, &query_embedding) {
        (Some(m), Some(emb)) => m
            .lock()
            .ok()
            .map(|db| crate::memory::search_longterm_prompt(&db, emb, &last_user_msg, 5))
            .unwrap_or_default(),
        _ => String::new(),
    };

    let mut memory_prompt = core_section;
    if !longterm_section.is_empty() {
        if !memory_prompt.is_empty() {
            memory_prompt.push('\n');
        }
        memory_prompt.push_str(&longterm_section);
    }

    let system_prompt = if base_prompt.is_empty() {
        None
    } else if memory_prompt.is_empty() {
        Some(base_prompt)
    } else {
        Some(format!("{}\n\n{}", base_prompt, memory_prompt))
    };

    // Build the event emitter callback (Arc<dyn Fn>)
    let on_event: Arc<dyn Fn(StreamEvent) + Send + Sync> = {
        let app = app.clone();
        Arc::new(move |event| {
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

    // 异步提取记忆（分步：先提取事实 + 算嵌入，再持锁存储）
    if let Some((user_text, assistant_text)) =
        extract_last_exchange(&parsed.messages, &result.new_messages)
    {
        // 1. 提取事实（async，不持锁）
        let facts = crate::memory::extractor::extract_facts_only(&user_text, &assistant_text).await;

        // 2. 计算嵌入（async，不持锁）
        let mut fact_embeddings: Vec<(crate::memory::extractor::ExtractedFact, Vec<f32>)> = Vec::new();
        for fact in facts {
            if let Some(pair) = crate::memory::extractor::compute_fact_embedding(&fact).await {
                fact_embeddings.push(pair);
            }
        }

        // 3. 持锁存储（sync，短暂）
        if !fact_embeddings.is_empty() {
            if let Some(Ok(db)) = memory_state.db.as_ref().map(|m| m.lock()) {
                let conv_id = parsed.conversation_id.as_deref();
                for (fact, emb) in &fact_embeddings {
                    if let Err(e) = crate::memory::extractor::store_fact_sync(fact, emb, conv_id, &db) {
                        eprintln!("[memory] 存储失败: {}", e);
                    }
                }
            }
        }
    }

    Ok(json!({
        "newMessages": result.new_messages,
        "steps": result.steps,
        "tokensIn": result.tokens_in,
        "tokensOut": result.tokens_out,
        "truncated": result.truncated,
        "allowedTools": allowed_tools,
    }))
}

/// 将 commit 工作记录压缩为简洁的工时描述（调用 LLM，非流式）。
#[tauri::command]
pub async fn summarize_work_content(text: String) -> Result<String, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err("工作内容为空，无需精简".into());
    }

    let prompt = format!(
        "你是一个工时记录精简助手。请将以下 commit 工作记录压缩为简洁的工时描述。\n\n\
         要求：\n\
         - 保留关键工作内容和成果\n\
         - 去除重复、冗余信息\n\
         - 合并同类项\n\
         - 控制在 200 字以内\n\
         - 输出纯文本，不要 markdown 格式\n\n\
         原始记录：\n{}",
        trimmed
    );

    let req = crate::llm::ChatRequest::new(vec![
        crate::llm::ChatMessage {
            role: crate::llm::Role::System,
            content: "你是一个工时记录精简助手，只输出精简后的工时描述文本，不加任何前缀或解释。".to_string(),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        },
        crate::llm::ChatMessage {
            role: crate::llm::Role::User,
            content: prompt,
            tool_calls: None,
            tool_call_id: None,
            name: None,
        },
    ]);

    let resp = crate::llm::chat(req).await?;
    Ok(resp.text.trim().to_string())
}

/// 从输入消息和输出消息中提取最后一轮 user-assistant 对话。
fn extract_last_exchange(
    input_messages: &[ChatSendMessage],
    new_messages: &[ChatMessage],
) -> Option<(String, String)> {
    // 找最后一条 user 消息
    let user_text = input_messages
        .iter()
        .rev()
        .find(|m| m.role == "user")
        .map(|m| m.content.clone())?;

    // 找第一条 assistant 文本回复（不含 tool_calls 的）
    let assistant_text = new_messages
        .iter()
        .find(|m| matches!(m.role, Role::Assistant) && m.tool_calls.is_none() && !m.content.is_empty())
        .map(|m| m.content.clone())
        .or_else(|| {
            // fallback: 拼所有 assistant 消息（跳过 tool_calls 请求，只取有文本内容的）
            let texts: Vec<String> = new_messages
                .iter()
                .filter(|m| matches!(m.role, Role::Assistant) && m.tool_calls.is_none())
                .map(|m| m.content.clone())
                .filter(|c| !c.is_empty())
                .collect();
            if texts.is_empty() {
                None
            } else {
                Some(texts.join("\n"))
            }
        })?;

    if user_text.is_empty() || assistant_text.is_empty() {
        return None;
    }
    Some((user_text, assistant_text))
}
