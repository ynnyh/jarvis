use super::*;
use serde_json::Value;
use std::time::Duration;
use crate::settings::LlmCredentials;

// ============================================================================
// Chat Completions 实现
// ============================================================================

pub(super) async fn chat_via_chat_completions(
    req: &ChatRequest,
    cred: &LlmCredentials,
) -> Result<ChatResponse, String> {
    let url = build_endpoint_url(&cred.base_url, "chat/completions");
    let model = req.model.clone().unwrap_or_else(|| cred.model.clone());
    let timeout_ms = req.timeout_ms.unwrap_or(60_000);

    let mut body = serde_json::json!({
        "model": model,
        "messages": req.messages,
        "temperature": req.temperature.unwrap_or(0.3),
        "max_tokens": req.max_tokens.unwrap_or(1024),
    });
    if let Some(tools) = &req.tools {
        if !tools.is_empty() {
            body["tools"] = serde_json::to_value(tools).map_err(|e| e.to_string())?;
            body["tool_choice"] = tool_choice_to_chat(req.tool_choice.as_ref());
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

    let choice = data.get("choices").and_then(|v| v.get(0));
    let message = choice.and_then(|c| c.get("message")).ok_or_else(|| {
        format!(
            "LLM 响应缺 choices[0].message: {}",
            crate::util::truncate_chars(&text, 200)
        )
    })?;

    // content 是常规模型回答的所在；reasoning_content 是 reasoning 系列模型（DeepSeek
    // R1/V4-flash、OpenAI o-series 兼容厂商）的字段，常规 content 在这种情况下会是空串。
    // 优先用 content，content 为空时回退到 reasoning_content（部分厂商把答案全塞这里）。
    let content_field = message
        .get("content")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let content = if content_field.trim().is_empty() {
        message
            .get("reasoning_content")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    } else {
        content_field
    };

    let tool_calls = parse_tool_calls_from_message(message);

    if content.is_empty() && tool_calls.is_empty() {
        return Err(format!(
            "LLM 响应既无 content 也无 tool_calls: {}",
            crate::util::truncate_chars(&text, 200)
        ));
    }

    let finish_reason = choice
        .and_then(|c| c.get("finish_reason"))
        .and_then(|v| v.as_str())
        .unwrap_or("stop")
        .to_string();

    Ok(ChatResponse {
        text: content,
        tool_calls,
        finish_reason,
        tokens_in: data
            .get("usage")
            .and_then(|u| u.get("prompt_tokens"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        tokens_out: data
            .get("usage")
            .and_then(|u| u.get("completion_tokens"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        model: data
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or(&model)
            .to_string(),
    })
}

fn parse_tool_calls_from_message(message: &Value) -> Vec<ToolCall> {
    let arr = match message.get("tool_calls").and_then(|v| v.as_array()) {
        Some(a) => a,
        None => return vec![],
    };
    arr.iter()
        .filter_map(|tc| {
            let kind = tc.get("type").and_then(|v| v.as_str())?;
            if kind != "function" {
                return None;
            }
            let func = tc.get("function")?;
            let name = func.get("name").and_then(|v| v.as_str())?.to_string();
            let args = func
                .get("arguments")
                .map(|v| {
                    if let Some(s) = v.as_str() {
                        s.to_string()
                    } else {
                        v.to_string()
                    }
                })
                .unwrap_or_else(|| "{}".to_string());
            let id = tc
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            Some(ToolCall {
                id,
                kind: "function".to_string(),
                function: ToolCallFunction {
                    name,
                    arguments: args,
                },
            })
        })
        .collect()
}

pub(super) fn tool_choice_to_chat(tc: Option<&ToolChoice>) -> Value {
    match tc {
        None | Some(ToolChoice::Auto) => Value::String("auto".into()),
        Some(ToolChoice::None) => Value::String("none".into()),
        Some(ToolChoice::Function(name)) => serde_json::json!({
            "type": "function",
            "function": { "name": name },
        }),
    }
}
