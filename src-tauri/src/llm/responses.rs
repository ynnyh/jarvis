use super::*;
use serde_json::Value;
use std::time::Duration;
use crate::settings::LlmCredentials;

// ============================================================================
// Responses API 实现
// ============================================================================

pub(super) async fn chat_via_responses(
    req: &ChatRequest,
    cred: &LlmCredentials,
) -> Result<ChatResponse, String> {
    let url = build_endpoint_url(&cred.base_url, "responses");
    let model = req.model.clone().unwrap_or_else(|| cred.model.clone());
    let timeout_ms = req.timeout_ms.unwrap_or(60_000);

    let mut body = serde_json::json!({
        "model": model,
        "input": messages_to_responses_input(&req.messages)?,
        "temperature": req.temperature.unwrap_or(0.3),
        "max_output_tokens": req.max_tokens.unwrap_or(1024),
    });
    if let Some(tools) = &req.tools {
        if !tools.is_empty() {
            body["tools"] = tools_to_responses_format(tools);
            body["tool_choice"] = tool_choice_to_responses(req.tool_choice.as_ref());
        }
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(timeout_ms))
        .build()
        .map_err(|e| format!("LLM client 构造失败: {}", e))?;

    let resp = client
        .post(&url)
        .bearer_auth(&cred.api_key)
        .json(&body)
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                format!("LLM 请求超时（{}ms）", timeout_ms)
            } else {
                format!("LLM 请求失败: {}", e)
            }
        })?;

    let status = resp.status();
    let text = resp.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!(
            "LLM HTTP {}: {}",
            status.as_u16(),
            crate::util::truncate_chars(&text, 400)
        ));
    }

    let data: Value = serde_json::from_str(&text).map_err(|_| {
        format!(
            "LLM 返回非 JSON: {}",
            crate::util::truncate_chars(&text, 200)
        )
    })?;

    parse_responses_output(&data, &model)
}

/// 把 ChatMessage[] 转成 Responses API 的 input 数组。
///
/// 规则：
/// - system/user/assistant(纯文本) → {type:"message", role, content}
/// - assistant 带 tool_calls → 先 emit message（若 content 非空），然后每个 tool call emit
///   {type:"function_call", call_id, name, arguments}
/// - tool 角色 → {type:"function_call_output", call_id, output}
fn messages_to_responses_input(messages: &[ChatMessage]) -> Result<Vec<Value>, String> {
    let mut out = Vec::with_capacity(messages.len());
    for m in messages {
        match m.role {
            Role::Tool => {
                let call_id = m
                    .tool_call_id
                    .as_ref()
                    .ok_or("tool 消息缺少 tool_call_id，Responses API 无法定位调用")?;
                out.push(serde_json::json!({
                    "type": "function_call_output",
                    "call_id": call_id,
                    "output": m.content,
                }));
            }
            Role::Assistant
                if m.tool_calls
                    .as_ref()
                    .map(|v| !v.is_empty())
                    .unwrap_or(false) =>
            {
                if !m.content.trim().is_empty() {
                    out.push(serde_json::json!({
                        "type": "message",
                        "role": "assistant",
                        "content": m.content,
                    }));
                }
                for tc in m.tool_calls.as_ref().unwrap() {
                    out.push(serde_json::json!({
                        "type": "function_call",
                        "call_id": tc.id,
                        "name": tc.function.name,
                        "arguments": tc.function.arguments,
                    }));
                }
            }
            _ => {
                let role_str = match m.role {
                    Role::System => "system",
                    Role::User => "user",
                    Role::Assistant => "assistant",
                    Role::Tool => unreachable!(),
                };
                out.push(serde_json::json!({
                    "type": "message",
                    "role": role_str,
                    "content": m.content,
                }));
            }
        }
    }
    Ok(out)
}

/// Chat Completions 风格的 tools → Responses 扁平结构。
///   {type:"function", function:{name,description,parameters}}
///   → {type:"function", name, description, parameters}
fn tools_to_responses_format(tools: &[ToolDefinition]) -> Value {
    Value::Array(
        tools
            .iter()
            .map(|t| {
                serde_json::json!({
                    "type": "function",
                    "name": t.function.name,
                    "description": t.function.description,
                    "parameters": t.function.parameters,
                })
            })
            .collect(),
    )
}

fn tool_choice_to_responses(tc: Option<&ToolChoice>) -> Value {
    match tc {
        None | Some(ToolChoice::Auto) => Value::String("auto".into()),
        Some(ToolChoice::None) => Value::String("none".into()),
        Some(ToolChoice::Function(name)) => serde_json::json!({
            "type": "function",
            "name": name,
        }),
    }
}

/// 解析 Responses API 响应。output 是 item 数组，里面有 message 和 function_call 两类。
///
///   - message item: { type:"message", role:"assistant", content:[ {type:"output_text", text} ] }
///     content 也可能是字符串（部分实现），都兼容
///   - function_call item: { type:"function_call", call_id, name, arguments, status }
///     这里映射回 Chat Completions 的 ToolCall.id = call_id
///
/// usage 字段名是 input_tokens / output_tokens（不是 prompt_tokens）
fn parse_responses_output(data: &Value, fallback_model: &str) -> Result<ChatResponse, String> {
    let items = data.get("output").and_then(|v| v.as_array());
    let mut text = String::new();
    let mut tool_calls = Vec::new();

    if let Some(items) = items {
        for item in items {
            let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
            match item_type {
                "message" => {
                    if let Some(s) = item.get("content").and_then(|v| v.as_str()) {
                        text.push_str(s);
                    } else if let Some(arr) = item.get("content").and_then(|v| v.as_array()) {
                        for part in arr {
                            if let Some(s) = part.as_str() {
                                text.push_str(s);
                                continue;
                            }
                            let pt = part.get("type").and_then(|v| v.as_str()).unwrap_or("");
                            if pt == "output_text" {
                                if let Some(t) = part.get("text").and_then(|v| v.as_str()) {
                                    text.push_str(t);
                                }
                            }
                        }
                    }
                }
                "function_call" => {
                    let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    let call_id = item
                        .get("call_id")
                        .or_else(|| item.get("id"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let args = item
                        .get("arguments")
                        .map(|v| {
                            if let Some(s) = v.as_str() {
                                s.to_string()
                            } else {
                                v.to_string()
                            }
                        })
                        .unwrap_or_else(|| "{}".to_string());
                    if !name.is_empty() && !call_id.is_empty() {
                        tool_calls.push(ToolCall {
                            id: call_id.to_string(),
                            kind: "function".to_string(),
                            function: ToolCallFunction {
                                name: name.to_string(),
                                arguments: args,
                            },
                        });
                    }
                }
                _ => {} // 其它 item 类型（reasoning / refusal / web_search_call 等）忽略
            }
        }
    }

    // 兜底：data.output_text 字段
    if text.is_empty() {
        if let Some(s) = data.get("output_text").and_then(|v| v.as_str()) {
            text = s.to_string();
        }
    }

    if text.is_empty() && tool_calls.is_empty() {
        let snippet = data.to_string();
        return Err(format!(
            "Responses 响应既无文本也无 tool_calls: {}",
            crate::util::truncate_chars(&snippet, 300)
        ));
    }

    let finish_reason = if !tool_calls.is_empty() {
        "tool_calls".to_string()
    } else if data.get("status").and_then(|v| v.as_str()) == Some("incomplete") {
        "length".to_string()
    } else {
        "stop".to_string()
    };

    Ok(ChatResponse {
        text,
        tool_calls,
        finish_reason,
        tokens_in: data
            .get("usage")
            .and_then(|u| u.get("input_tokens"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        tokens_out: data
            .get("usage")
            .and_then(|u| u.get("output_tokens"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        model: data
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or(fallback_model)
            .to_string(),
    })
}
