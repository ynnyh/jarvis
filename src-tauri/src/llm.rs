// LLM 客户端：支持 Chat Completions 和 Responses 两种 wire 协议。
//
// 对应原 TS 端 src/llm/client.ts。两种 wire 协议都支持：
//
//   wire_api='chat'      → POST <base>/chat/completions（或裸 host 补 /v1）
//                        OpenAI Chat Completions 规范，DeepSeek/Moonshot/Qwen/各种国产模型都兼容。
//
//   wire_api='responses' → POST <base>/responses（或裸 host 补 /v1）
//                        OpenAI Responses API（Codex CLI 协议）。请求/响应结构和 Chat
//                        Completions 完全不同：messages → input、choices[0].message → output[]、
//                        function 工具调用从嵌套结构变成扁平 type=function_call 项等。
//
// 对外 ChatMessage / ChatRequest / ChatResponse 都用 OpenAI Chat Completions 风格——
// 上层（agent loop / ask-llm）不需要关心 wire 协议差异。Responses 适配只在本文件内。
//
// 不在这里做 prompt 拼装、retry、流式、缓存——这些都是上层的事。
// 不支持流式（现有调用方都是非流式）。

#![allow(dead_code)]
// M2 阶段先把库写好，wiring 在 M5 完成（chat-agent / ask-llm 工具迁移时接入）。

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::settings::{get_llm_credentials, LlmCredentials};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
    /// assistant 消息发起的 tool calls。仅 assistant 角色可用
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    /// tool 消息携带的 call id，必须匹配某个 assistant.tool_calls[i].id
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// tool 消息可选的工具名（部分厂商需要）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// OpenAI 风格的 tool call。type 当前只有 "function"
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String, // "function"
    pub function: ToolCallFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallFunction {
    pub name: String,
    /// JSON 字符串。模型可能产出不合法 JSON，调用方需 try parse
    pub arguments: String,
}

/// OpenAI tools 字段格式（Chat Completions 风格，内部按需转 Responses 扁平结构）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    #[serde(rename = "type")]
    pub kind: String, // "function"
    pub function: ToolDefinitionFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinitionFunction {
    pub name: String,
    pub description: String,
    pub parameters: Value, // JSON Schema
}

/// 'auto' / 'none' / 指定函数
#[derive(Debug, Clone)]
pub enum ToolChoice {
    Auto,
    None,
    Function(String),
}

#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub messages: Vec<ChatMessage>,
    /// 0~2，默认 0.3（偏确定性，日报这种场景不希望发散）
    pub temperature: Option<f32>,
    /// 单次最大返回 tokens。默认 1024，长日报可调到 2048+
    pub max_tokens: Option<u32>,
    /// 覆盖默认 model（从 config 读），少数场景需要强制用别的 model
    pub model: Option<String>,
    /// 超时毫秒，默认 60s。LLM 响应慢，DeepSeek 偶尔要 30s+
    pub timeout_ms: Option<u64>,
    /// 可用工具列表，触发 function calling
    pub tools: Option<Vec<ToolDefinition>>,
    /// 默认 Auto（仅 tools 存在时有意义）
    pub tool_choice: Option<ToolChoice>,
}

impl ChatRequest {
    pub fn new(messages: Vec<ChatMessage>) -> Self {
        Self {
            messages,
            temperature: None,
            max_tokens: None,
            model: None,
            timeout_ms: None,
            tools: None,
            tool_choice: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResponse {
    pub text: String,
    /// 模型决定调用的工具。无则空数组
    pub tool_calls: Vec<ToolCall>,
    /// stop / tool_calls / length 等
    pub finish_reason: String,
    /// 入参 tokens（OpenAI 兼容字段 usage.prompt_tokens）
    pub tokens_in: u64,
    /// 输出 tokens（usage.completion_tokens）
    pub tokens_out: u64,
    pub model: String,
}

/// 主入口。读 LLM 凭证，按 wire_api 派发。
pub async fn chat(req: ChatRequest) -> Result<ChatResponse, String> {
    let cred = get_llm_credentials();
    chat_with_credentials(req, cred).await
}

pub async fn chat_with_credentials(
    req: ChatRequest,
    cred: LlmCredentials,
) -> Result<ChatResponse, String> {
    if cred.api_key.is_empty() {
        return Err(
            "LLM apiKey 未配置（检查 ~/.jarvis/config.json 的 llm.apiKey 或 env LLM_API_KEY）"
                .into(),
        );
    }
    if cred.base_url.is_empty() {
        return Err("LLM baseUrl 未配置".into());
    }
    let mut last_error: Option<String> = None;
    for attempt in 0..3 {
        let result = if cred.wire_api == "responses" {
            chat_via_responses(&req, &cred).await
        } else {
            chat_via_chat_completions(&req, &cred).await
        };
        match result {
            Ok(resp) => return Ok(resp),
            Err(e) if should_retry_llm_error(&e) && attempt < 2 => {
                last_error = Some(e);
                tokio::time::sleep(Duration::from_millis(350 * (attempt + 1) as u64)).await;
            }
            Err(e) => return Err(e),
        }
    }
    Err(last_error.unwrap_or_else(|| "LLM 调用失败".to_string()))
}

fn should_retry_llm_error(e: &str) -> bool {
    e.contains("LLM HTTP 429")
        || e.contains("LLM HTTP 502")
        || e.contains("LLM HTTP 503")
        || e.contains("LLM HTTP 504")
        || e.contains("LLM 请求超时")
}

// ============================================================================
// URL 拼接
// ============================================================================

/// 拼接最终请求 URL。endpoint 形如 'chat/completions' 或 'responses'（不带前导 /）。
///
/// 启发式（与 TS 端 buildEndpointUrl 对齐）：
///   - URL 的 pathname 只有 `/` → 裸 host，补 `/v1/<endpoint>`（DeepSeek 风格）
///   - 其它 → 已含完整前缀，直接 append `/<endpoint>`（OpenAI `/v1`、Codex 反代 `/codex` 等）
fn build_endpoint_url(raw_base: &str, endpoint: &str) -> String {
    let trimmed = raw_base.trim_end_matches('/');
    let has_custom_prefix = match reqwest::Url::parse(trimmed) {
        Ok(u) => {
            let p = u.path();
            !(p.is_empty() || p == "/")
        }
        Err(_) => false,
    };
    if has_custom_prefix {
        format!("{}/{}", trimmed, endpoint)
    } else {
        format!("{}/v1/{}", trimmed, endpoint)
    }
}

// ============================================================================
// Chat Completions 实现
// ============================================================================

async fn chat_via_chat_completions(
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

fn tool_choice_to_chat(tc: Option<&ToolChoice>) -> Value {
    match tc {
        None | Some(ToolChoice::Auto) => Value::String("auto".into()),
        Some(ToolChoice::None) => Value::String("none".into()),
        Some(ToolChoice::Function(name)) => serde_json::json!({
            "type": "function",
            "function": { "name": name },
        }),
    }
}

// ============================================================================
// Responses API 实现
// ============================================================================

async fn chat_via_responses(
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_endpoint_url_bare_host() {
        assert_eq!(
            build_endpoint_url("https://api.deepseek.com", "chat/completions"),
            "https://api.deepseek.com/v1/chat/completions"
        );
        assert_eq!(
            build_endpoint_url("https://api.deepseek.com/", "responses"),
            "https://api.deepseek.com/v1/responses"
        );
    }

    #[test]
    fn build_endpoint_url_with_v1() {
        assert_eq!(
            build_endpoint_url("https://api.openai.com/v1", "chat/completions"),
            "https://api.openai.com/v1/chat/completions"
        );
    }

    #[test]
    fn build_endpoint_url_custom_prefix() {
        assert_eq!(
            build_endpoint_url("http://www.example.com:19504/codex", "chat/completions"),
            "http://www.example.com:19504/codex/chat/completions"
        );
        assert_eq!(
            build_endpoint_url("http://host/openai/v1", "responses"),
            "http://host/openai/v1/responses"
        );
    }
}
