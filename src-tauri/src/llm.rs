// LLM 客户端：支持 Chat Completions、Responses 和 Anthropic Messages 三种 wire 协议。
//
// 对应原 TS 端 src/llm/client.ts。三种 wire 协议都支持：
//
//   wire_api='chat'      → POST <base>/chat/completions（或裸 host 补 /v1）
//                        OpenAI Chat Completions 规范，DeepSeek/Moonshot/Qwen/各种国产模型都兼容。
//
//   wire_api='responses' → POST <base>/responses（或裸 host 补 /v1）
//                        OpenAI Responses API（Codex CLI 协议）。请求/响应结构和 Chat
//                        Completions 完全不同：messages → input、choices[0].message → output[]、
//                        function 工具调用从嵌套结构变成扁平 type=function_call 项等。
//
//   wire_api='anthropic' → POST <base>/v1/messages
//                        Anthropic Messages 协议（Claude / cc-switch claude provider / 各种中转）。
//                        system 抽成顶层独立字段、max_tokens 必填、tool 结果以 user 角色的
//                        tool_result 块回传且相邻多条要折叠进同一条 user 消息。详见 chat_via_anthropic。
//
// 对外 ChatMessage / ChatRequest / ChatResponse 都用 OpenAI Chat Completions 风格——
// 上层（agent loop / ask-llm）不需要关心 wire 协议差异。Responses / Anthropic 适配只在本文件内。
//
// 不在这里做 prompt 拼装、retry、缓存——这些都是上层的事。
// 流式只 Chat Completions 走真 SSE；responses / anthropic 在 streaming_chat 里退化为非流式。

#![allow(dead_code)]
// M2 阶段先把库写好，wiring 在 M5 完成（chat-agent / ask-llm 工具迁移时接入）。

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::settings::{get_llm_credentials, LlmCredentials};

/// Streaming text delta from the LLM.
#[derive(Clone, Serialize)]
pub struct StreamDelta {
    pub text: String,
}

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
        } else if cred.wire_api == "anthropic" {
            chat_via_anthropic(&req, &cred).await
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
// 流式 Chat Completions（stream: true）
// ============================================================================

/// 发起流式 Chat Completions 请求，每收到一个 text delta 就调用 `on_delta`。
/// 仅 Chat Completions 走真正的 SSE 流式；responses / anthropic 协议不支持标准 SSE，
/// 退化为非流式（内部转调 `chat_with_credentials`，跑完一次性把全文 emit 给 on_delta）。
/// 不重试——调用方（agent loop）自己处理错误恢复。
pub async fn streaming_chat<F>(
    req: &ChatRequest,
    cred: &LlmCredentials,
    on_delta: F,
) -> Result<ChatResponse, String>
where
    F: Fn(String) + Send,
{
    if cred.api_key.is_empty() {
        return Err(
            "LLM apiKey 未配置（检查 ~/.jarvis/config.json 的 llm.apiKey 或 env LLM_API_KEY）"
                .into(),
        );
    }
    if cred.base_url.is_empty() {
        return Err("LLM baseUrl 未配置".into());
    }
    // responses / anthropic 协议不做 SSE 流式，退化为非流式：跑完一次性把全文 emit 给 on_delta。
    // agent loop 用返回的 ChatResponse.text/.tool_calls 做全部逻辑，on_delta 只用于实时显示，
    // 退化后只是「整段一次性出现」而非逐字，对调用方透明（避免 active profile 为 responses/anthropic
    // 时 agent loop 因硬错误而死锁）。
    if cred.wire_api == "responses" || cred.wire_api == "anthropic" {
        let resp = chat_with_credentials(req.clone(), cred.clone()).await?;
        if !resp.text.is_empty() {
            on_delta(resp.text.clone());
        }
        return Ok(resp);
    }

    let url = build_endpoint_url(&cred.base_url, "chat/completions");
    let model = req.model.clone().unwrap_or_else(|| cred.model.clone());
    let timeout_ms = req.timeout_ms.unwrap_or(120_000);

    let mut body = serde_json::json!({
        "model": model,
        "messages": req.messages,
        "temperature": req.temperature.unwrap_or(0.3),
        "max_tokens": req.max_tokens.unwrap_or(1024),
        "stream": true,
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
        .header("Accept", "text/event-stream")
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                format!("LLM 流式请求超时（{}ms）", timeout_ms)
            } else {
                format!("LLM 流式请求失败: {}", e)
            }
        })?;

    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.map_err(|e| e.to_string())?;
        return Err(format!(
            "LLM HTTP {}: {}",
            status.as_u16(),
            crate::util::truncate_chars(&text, 400)
        ));
    }

    use futures_util::StreamExt;

    let mut full_text = String::new();
    let mut all_tool_calls: Vec<ToolCall> = Vec::new();
    let mut finish_reason = String::new();
    let mut model_out = model.clone();
    let mut tokens_in = 0u64;
    let mut tokens_out = 0u64;
    let mut buffer = String::new();
    let mut stream = resp.bytes_stream();

    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result.map_err(|e| format!("SSE 读取错误: {}", e))?;
        let chunk_str = String::from_utf8_lossy(&chunk);
        buffer.push_str(&chunk_str);

        loop {
            let dnl = match buffer.find("\n\n") {
                Some(p) => p,
                None => break,
            };
            let block = buffer[..dnl].to_string();
            buffer = buffer[dnl + 2..].to_string();

            for line in block.lines() {
                if !line.starts_with("data: ") {
                    continue;
                }
                let data = line[6..].trim();
                if data == "[DONE]" {
                    continue;
                }
                let parsed: Value = match serde_json::from_str(data) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                if let Some(choices) = parsed.get("choices").and_then(|v| v.as_array()) {
                    if let Some(choice) = choices.first() {
                        if let Some(delta) = choice.get("delta") {
                            // text content
                            if let Some(content) = delta.get("content").and_then(|v| v.as_str()) {
                                if !content.is_empty() {
                                    full_text.push_str(content);
                                    on_delta(content.to_string());
                                }
                            }
                            // reasoning_content fallback
                            if full_text.trim().is_empty() {
                                if let Some(rc) =
                                    delta.get("reasoning_content").and_then(|v| v.as_str())
                                {
                                    if !rc.is_empty() {
                                        full_text.push_str(rc);
                                        on_delta(rc.to_string());
                                    }
                                }
                            }
                            // tool calls (accumulate by index)
                            if let Some(tcs) = delta.get("tool_calls").and_then(|v| v.as_array()) {
                                for tc in tcs {
                                    let idx = tc
                                        .get("index")
                                        .and_then(|v| v.as_u64())
                                        .unwrap_or(0) as usize;
                                    while all_tool_calls.len() <= idx {
                                        all_tool_calls.push(ToolCall {
                                            id: String::new(),
                                            kind: "function".to_string(),
                                            function: ToolCallFunction {
                                                name: String::new(),
                                                arguments: String::new(),
                                            },
                                        });
                                    }
                                    if let Some(id) =
                                        tc.get("id").and_then(|v| v.as_str())
                                    {
                                        if !id.is_empty() {
                                            all_tool_calls[idx].id = id.to_string();
                                        }
                                    }
                                    if let Some(func) = tc.get("function") {
                                        if let Some(name) =
                                            func.get("name").and_then(|v| v.as_str())
                                        {
                                            if !name.is_empty() {
                                                all_tool_calls[idx].function.name = name.to_string();
                                            }
                                        }
                                        if let Some(args) =
                                            func.get("arguments").and_then(|v| v.as_str())
                                        {
                                            all_tool_calls[idx].function.arguments.push_str(args);
                                        }
                                    }
                                }
                            }
                        }
                        if let Some(fr) =
                            choice.get("finish_reason").and_then(|v| v.as_str())
                        {
                            if !fr.is_empty() {
                                finish_reason = fr.to_string();
                            }
                        }
                    }
                }
                if let Some(usage) = parsed.get("usage") {
                    tokens_in = usage
                        .get("prompt_tokens")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    tokens_out = usage
                        .get("completion_tokens")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                }
                if let Some(m) = parsed.get("model").and_then(|v| v.as_str()) {
                    model_out = m.to_string();
                }
            }
        }
    }

    let tool_calls: Vec<ToolCall> = all_tool_calls
        .into_iter()
        .filter(|tc| !tc.function.name.is_empty())
        .collect();

    if full_text.is_empty() && tool_calls.is_empty() {
        return Err("LLM 流式响应既无内容也无 tool_calls".into());
    }

    Ok(ChatResponse {
        text: full_text,
        tool_calls,
        finish_reason: if finish_reason.is_empty() {
            "stop".into()
        } else {
            finish_reason
        },
        tokens_in,
        tokens_out,
        model: model_out,
    })
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

// ============================================================================
// Anthropic Messages 实现
// ============================================================================

/// 拼接 Anthropic Messages endpoint。
///
/// cc-switch 的 claude provider 存的 ANTHROPIC_BASE_URL（如 `https://api.anthropic.com`、
/// 各种中转）期望直接追加 `/v1/messages`（与 Claude Code 行为一致）。这里**不复用**
/// `build_endpoint_url`——后者对带 path 前缀的 host 只会追加 `/messages`、漏掉 `/v1`。
/// 规则：trim 尾部 `/`，若已以 `/v1` 结尾则只追加 `/messages`，否则追加 `/v1/messages`。
fn build_anthropic_url(raw_base: &str) -> String {
    let trimmed = raw_base.trim_end_matches('/');
    if trimmed.ends_with("/v1") {
        format!("{}/messages", trimmed)
    } else {
        format!("{}/v1/messages", trimmed)
    }
}

/// Anthropic Messages 协议（非流式）。
///
/// 与 Chat Completions 的主要差异（容易写错的坑都在这）：
///   - endpoint 固定 `<base>/v1/messages`，见 build_anthropic_url。
///   - auth：用 `Authorization: Bearer <token>`，**不是** `x-api-key`。因为 cc-switch 的
///     claude provider 存的是 ANTHROPIC_AUTH_TOKEN（Claude Code 用 AUTH_TOKEN 时正走 Bearer）。
///     若某 provider 改用 ANTHROPIC_API_KEY，规范上应走 `x-api-key`；当前 cc-switch claude
///     一律走 AUTH_TOKEN→Bearer，故这里固定 Bearer。
///   - `anthropic-version: 2023-06-01` 必带。
///   - `max_tokens` 必填（Anthropic 无默认值），用 req.max_tokens.unwrap_or(1024)。
///   - system 消息抽成顶层独立字段，不进 messages（多条用 \n\n 拼接）。
///   - tool 结果以 **user** 角色的 tool_result 块回传；agent 一轮调多个工具会产生连续多条
///     Role::Tool，必须折叠进**同一条** user 消息的 content 数组，否则 Anthropic 会因
///     「连续两条 user / role 不交替」报错。折叠逻辑见 messages_to_anthropic。
async fn chat_via_anthropic(
    req: &ChatRequest,
    cred: &LlmCredentials,
) -> Result<ChatResponse, String> {
    let url = build_anthropic_url(&cred.base_url);
    let model = req.model.clone().unwrap_or_else(|| cred.model.clone());
    let timeout_ms = req.timeout_ms.unwrap_or(60_000);

    let (system, messages) = messages_to_anthropic(&req.messages)?;

    let mut body = serde_json::json!({
        "model": model,
        // Anthropic 必填，无默认值
        "max_tokens": req.max_tokens.unwrap_or(1024),
        "temperature": req.temperature.unwrap_or(0.3),
        "messages": messages,
    });
    // system 为顶层独立字段，无 system 文本则省略
    if !system.is_empty() {
        body["system"] = Value::String(system);
    }
    if let Some(tools) = &req.tools {
        if !tools.is_empty() {
            body["tools"] = tools_to_anthropic_format(tools);
            body["tool_choice"] = tool_choice_to_anthropic(req.tool_choice.as_ref());
        }
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(timeout_ms))
        .build()
        .map_err(|e| format!("LLM client 构造失败: {}", e))?;

    let resp = client
        .post(&url)
        // cc-switch claude 走 ANTHROPIC_AUTH_TOKEN → Bearer（非 x-api-key）
        .bearer_auth(&cred.api_key)
        .header("anthropic-version", "2023-06-01")
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

    parse_anthropic_output(&data, &model)
}

/// 把 ChatMessage[] 转成 (system 文本, Anthropic messages 数组)。
///
/// 规则：
/// - Role::System → 抽进顶层 system 字符串（多条用 \n\n 拼接），不进 messages。
/// - Role::User（纯文本）→ {role:"user", content: <字符串>}。
/// - Role::Assistant 纯文本 → {role:"assistant", content: <字符串>}。
/// - Role::Assistant 带 tool_calls → {role:"assistant", content:[ text块?, tool_use块... ]}；
///   tool_use.input 是**对象**，而内部 ToolCall.arguments 是**字符串**，须 from_str 转回对象
///   （parse 失败兜底 {}）。
/// - Role::Tool → user 角色的 tool_result 块；**相邻多条 Role::Tool 折叠进同一条 user 消息**
///   的 content 数组，避免连续 user / role 不交替报错。
fn messages_to_anthropic(messages: &[ChatMessage]) -> Result<(String, Vec<Value>), String> {
    let mut system_parts: Vec<String> = Vec::new();
    let mut out: Vec<Value> = Vec::with_capacity(messages.len());

    for m in messages {
        match m.role {
            Role::System => {
                if !m.content.trim().is_empty() {
                    system_parts.push(m.content.clone());
                }
            }
            Role::Tool => {
                let tool_use_id = m
                    .tool_call_id
                    .as_ref()
                    .ok_or("tool 消息缺少 tool_call_id，Anthropic 无法定位 tool_result")?;
                let block = serde_json::json!({
                    "type": "tool_result",
                    "tool_use_id": tool_use_id,
                    "content": m.content,
                });
                // 折叠：若上一条已是 user 且 content 为块数组（即上一条也是 tool_result），追加进去；
                // 否则新开一条 user 消息。
                if let Some(last) = out.last_mut() {
                    if last.get("role").and_then(|v| v.as_str()) == Some("user") {
                        if let Some(arr) = last.get_mut("content").and_then(|v| v.as_array_mut()) {
                            arr.push(block);
                            continue;
                        }
                    }
                }
                out.push(serde_json::json!({
                    "role": "user",
                    "content": [block],
                }));
            }
            Role::Assistant
                if m.tool_calls
                    .as_ref()
                    .map(|v| !v.is_empty())
                    .unwrap_or(false) =>
            {
                let mut blocks: Vec<Value> = Vec::new();
                if !m.content.trim().is_empty() {
                    blocks.push(serde_json::json!({
                        "type": "text",
                        "text": m.content,
                    }));
                }
                for tc in m.tool_calls.as_ref().unwrap() {
                    // arguments 是 JSON 字符串，tool_use.input 需要对象；parse 失败兜底 {}
                    let input: Value = serde_json::from_str(&tc.function.arguments)
                        .unwrap_or_else(|_| serde_json::json!({}));
                    blocks.push(serde_json::json!({
                        "type": "tool_use",
                        "id": tc.id,
                        "name": tc.function.name,
                        "input": input,
                    }));
                }
                out.push(serde_json::json!({
                    "role": "assistant",
                    "content": blocks,
                }));
            }
            Role::User => {
                out.push(serde_json::json!({
                    "role": "user",
                    "content": m.content,
                }));
            }
            Role::Assistant => {
                out.push(serde_json::json!({
                    "role": "assistant",
                    "content": m.content,
                }));
            }
        }
    }

    Ok((system_parts.join("\n\n"), out))
}

/// Chat Completions 风格 tools → Anthropic 格式。
///   {type:"function", function:{name,description,parameters}}
///   → {name, description, input_schema: <JSON Schema>}
/// 注意字段名是 `input_schema`（值就是 ToolDefinitionFunction.parameters）。
fn tools_to_anthropic_format(tools: &[ToolDefinition]) -> Value {
    Value::Array(
        tools
            .iter()
            .map(|t| {
                serde_json::json!({
                    "name": t.function.name,
                    "description": t.function.description,
                    "input_schema": t.function.parameters,
                })
            })
            .collect(),
    )
}

/// tool_choice → Anthropic 格式。
/// Auto/None → {type:"auto"}（Anthropic 的 none 类型并非所有中转都支持，None 也映射成 auto）；
/// Function(name) → {type:"tool", name}。
fn tool_choice_to_anthropic(tc: Option<&ToolChoice>) -> Value {
    match tc {
        Some(ToolChoice::Function(name)) => serde_json::json!({
            "type": "tool",
            "name": name,
        }),
        _ => serde_json::json!({ "type": "auto" }),
    }
}

/// 解析 Anthropic Messages 响应，产出与其它实现一致的 ChatResponse。
///
///   { model, content:[ {type:"text", text}, {type:"tool_use", id, name, input:{...}} ],
///     stop_reason:"end_turn|tool_use|max_tokens|stop_sequence",
///     usage:{input_tokens, output_tokens} }
///
/// - text = 拼接所有 content[].type=="text" 的 text。
/// - tool_calls = 每个 content[].type=="tool_use" → ToolCall{ id, kind:"function",
///   function:{ name, arguments: input 序列化回 JSON 字符串 } }（input 是对象，要 to_string）。
/// - finish_reason：stop_reason=="tool_use" → "tool_calls"；"max_tokens" → "length"；其它 → "stop"。
fn parse_anthropic_output(data: &Value, fallback_model: &str) -> Result<ChatResponse, String> {
    let mut text = String::new();
    let mut tool_calls = Vec::new();

    if let Some(blocks) = data.get("content").and_then(|v| v.as_array()) {
        for block in blocks {
            let block_type = block.get("type").and_then(|v| v.as_str()).unwrap_or("");
            match block_type {
                "text" => {
                    if let Some(t) = block.get("text").and_then(|v| v.as_str()) {
                        text.push_str(t);
                    }
                }
                "tool_use" => {
                    let id = block.get("id").and_then(|v| v.as_str()).unwrap_or("");
                    let name = block.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    // input 是对象，序列化回 JSON 字符串塞进 arguments（无则兜底 {}）
                    let arguments = block
                        .get("input")
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "{}".to_string());
                    if !name.is_empty() && !id.is_empty() {
                        tool_calls.push(ToolCall {
                            id: id.to_string(),
                            kind: "function".to_string(),
                            function: ToolCallFunction {
                                name: name.to_string(),
                                arguments,
                            },
                        });
                    }
                }
                _ => {} // 其它块类型（thinking 等）忽略
            }
        }
    }

    if text.is_empty() && tool_calls.is_empty() {
        let snippet = data.to_string();
        return Err(format!(
            "Anthropic 响应既无文本也无 tool_calls: {}",
            crate::util::truncate_chars(&snippet, 300)
        ));
    }

    let finish_reason = match data.get("stop_reason").and_then(|v| v.as_str()) {
        Some("tool_use") => "tool_calls".to_string(),
        Some("max_tokens") => "length".to_string(),
        _ => "stop".to_string(),
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

    #[test]
    fn anthropic_url_bare_and_v1() {
        // 裸 host 补 /v1/messages
        assert_eq!(
            build_anthropic_url("https://api.anthropic.com"),
            "https://api.anthropic.com/v1/messages"
        );
        assert_eq!(
            build_anthropic_url("https://api.anthropic.com/"),
            "https://api.anthropic.com/v1/messages"
        );
        // 已带 /v1 只补 /messages，不重复 /v1
        assert_eq!(
            build_anthropic_url("https://relay.example.com/v1"),
            "https://relay.example.com/v1/messages"
        );
        assert_eq!(
            build_anthropic_url("https://relay.example.com/v1/"),
            "https://relay.example.com/v1/messages"
        );
    }

    #[test]
    fn anthropic_messages_extracts_system_and_keeps_alternation() {
        let msgs = vec![
            ChatMessage {
                role: Role::System,
                content: "你是助手".into(),
                tool_calls: None,
                tool_call_id: None,
                name: None,
            },
            ChatMessage {
                role: Role::System,
                content: "请用中文".into(),
                tool_calls: None,
                tool_call_id: None,
                name: None,
            },
            ChatMessage {
                role: Role::User,
                content: "你好".into(),
                tool_calls: None,
                tool_call_id: None,
                name: None,
            },
        ];
        let (system, out) = messages_to_anthropic(&msgs).unwrap();
        // 多条 system 用 \n\n 拼接，不进 messages
        assert_eq!(system, "你是助手\n\n请用中文");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["role"], "user");
        assert_eq!(out[0]["content"], "你好");
    }

    #[test]
    fn anthropic_assistant_tool_calls_become_blocks_with_parsed_input() {
        let msgs = vec![ChatMessage {
            role: Role::Assistant,
            content: "我来查一下".into(),
            tool_calls: Some(vec![ToolCall {
                id: "toolu_1".into(),
                kind: "function".into(),
                function: ToolCallFunction {
                    name: "search".into(),
                    arguments: r#"{"q":"禅道"}"#.into(),
                },
            }]),
            tool_call_id: None,
            name: None,
        }];
        let (_system, out) = messages_to_anthropic(&msgs).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["role"], "assistant");
        let blocks = out[0]["content"].as_array().unwrap();
        // 先 text 块，再 tool_use 块
        assert_eq!(blocks[0]["type"], "text");
        assert_eq!(blocks[0]["text"], "我来查一下");
        assert_eq!(blocks[1]["type"], "tool_use");
        assert_eq!(blocks[1]["id"], "toolu_1");
        assert_eq!(blocks[1]["name"], "search");
        // input 必须是对象（由 arguments 字符串 parse 回来），不是字符串
        assert_eq!(blocks[1]["input"]["q"], "禅道");
    }

    #[test]
    fn anthropic_consecutive_tool_results_collapse_into_one_user_message() {
        // agent 一轮调多个工具 → 连续多条 Role::Tool，必须折叠进同一条 user 消息
        let msgs = vec![
            ChatMessage {
                role: Role::Assistant,
                content: "".into(),
                tool_calls: Some(vec![
                    ToolCall {
                        id: "toolu_a".into(),
                        kind: "function".into(),
                        function: ToolCallFunction {
                            name: "f1".into(),
                            arguments: "{}".into(),
                        },
                    },
                    ToolCall {
                        id: "toolu_b".into(),
                        kind: "function".into(),
                        function: ToolCallFunction {
                            name: "f2".into(),
                            arguments: "{}".into(),
                        },
                    },
                ]),
                tool_call_id: None,
                name: None,
            },
            ChatMessage {
                role: Role::Tool,
                content: "结果A".into(),
                tool_calls: None,
                tool_call_id: Some("toolu_a".into()),
                name: Some("f1".into()),
            },
            ChatMessage {
                role: Role::Tool,
                content: "结果B".into(),
                tool_calls: None,
                tool_call_id: Some("toolu_b".into()),
                name: Some("f2".into()),
            },
        ];
        let (_system, out) = messages_to_anthropic(&msgs).unwrap();
        // assistant 一条 + 折叠后的 user 一条 = 2 条，role 严格交替
        assert_eq!(out.len(), 2);
        assert_eq!(out[0]["role"], "assistant");
        assert_eq!(out[1]["role"], "user");
        let results = out[1]["content"].as_array().unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0]["type"], "tool_result");
        assert_eq!(results[0]["tool_use_id"], "toolu_a");
        assert_eq!(results[0]["content"], "结果A");
        assert_eq!(results[1]["tool_use_id"], "toolu_b");
        assert_eq!(results[1]["content"], "结果B");
    }

    #[test]
    fn anthropic_tools_use_input_schema_field() {
        let tools = vec![ToolDefinition {
            kind: "function".into(),
            function: ToolDefinitionFunction {
                name: "search".into(),
                description: "搜索".into(),
                parameters: serde_json::json!({"type":"object","properties":{}}),
            },
        }];
        let v = tools_to_anthropic_format(&tools);
        let arr = v.as_array().unwrap();
        assert_eq!(arr[0]["name"], "search");
        assert_eq!(arr[0]["description"], "搜索");
        // 字段名是 input_schema，值就是 JSON Schema
        assert_eq!(arr[0]["input_schema"]["type"], "object");
    }

    #[test]
    fn anthropic_tool_choice_mapping() {
        assert_eq!(tool_choice_to_anthropic(None)["type"], "auto");
        assert_eq!(
            tool_choice_to_anthropic(Some(&ToolChoice::None))["type"],
            "auto"
        );
        let f = tool_choice_to_anthropic(Some(&ToolChoice::Function("search".into())));
        assert_eq!(f["type"], "tool");
        assert_eq!(f["name"], "search");
    }

    #[test]
    fn anthropic_parse_text_and_tool_use() {
        let data = serde_json::json!({
            "model": "claude-x",
            "content": [
                {"type": "text", "text": "好的"},
                {"type": "tool_use", "id": "toolu_9", "name": "search", "input": {"q": "x"}}
            ],
            "stop_reason": "tool_use",
            "usage": {"input_tokens": 11, "output_tokens": 22}
        });
        let resp = parse_anthropic_output(&data, "fallback").unwrap();
        assert_eq!(resp.text, "好的");
        assert_eq!(resp.tool_calls.len(), 1);
        assert_eq!(resp.tool_calls[0].id, "toolu_9");
        assert_eq!(resp.tool_calls[0].function.name, "search");
        // input 对象被序列化回 JSON 字符串塞进 arguments
        let parsed: Value =
            serde_json::from_str(&resp.tool_calls[0].function.arguments).unwrap();
        assert_eq!(parsed["q"], "x");
        // stop_reason=tool_use → finish_reason=tool_calls
        assert_eq!(resp.finish_reason, "tool_calls");
        assert_eq!(resp.tokens_in, 11);
        assert_eq!(resp.tokens_out, 22);
        assert_eq!(resp.model, "claude-x");
    }

    #[test]
    fn anthropic_parse_max_tokens_maps_to_length() {
        let data = serde_json::json!({
            "model": "claude-x",
            "content": [{"type": "text", "text": "截断了"}],
            "stop_reason": "max_tokens",
            "usage": {"input_tokens": 1, "output_tokens": 2}
        });
        let resp = parse_anthropic_output(&data, "fallback").unwrap();
        assert_eq!(resp.finish_reason, "length");
    }

    #[test]
    fn anthropic_parse_empty_is_err() {
        let data = serde_json::json!({
            "model": "claude-x",
            "content": [],
            "stop_reason": "end_turn"
        });
        assert!(parse_anthropic_output(&data, "fallback").is_err());
    }

    #[test]
    fn anthropic_tool_result_after_assistant_opens_new_user_not_fold_into_assistant() {
        // 正常 agent loop 首条 tool 结果：上一条是 assistant（tool_use 块数组），
        // 折叠守卫只在「上一条 role==user 且 content 为数组」时追加，否则新开 user。
        // 这里必须新开一条 user，绝不能把 tool_result 误塞进 assistant 的 content 数组里。
        let msgs = vec![
            ChatMessage {
                role: Role::Assistant,
                content: "".into(),
                tool_calls: Some(vec![ToolCall {
                    id: "toolu_a".into(),
                    kind: "function".into(),
                    function: ToolCallFunction {
                        name: "f1".into(),
                        arguments: "{}".into(),
                    },
                }]),
                tool_call_id: None,
                name: None,
            },
            ChatMessage {
                role: Role::Tool,
                content: "结果A".into(),
                tool_calls: None,
                tool_call_id: Some("toolu_a".into()),
                name: Some("f1".into()),
            },
        ];
        let (_system, out) = messages_to_anthropic(&msgs).unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0]["role"], "assistant");
        // assistant 块仍只含 tool_use，没被 tool_result 污染
        let asst_blocks = out[0]["content"].as_array().unwrap();
        assert_eq!(asst_blocks.len(), 1);
        assert_eq!(asst_blocks[0]["type"], "tool_use");
        // tool_result 在新开的 user 消息里
        assert_eq!(out[1]["role"], "user");
        assert_eq!(out[1]["content"].as_array().unwrap()[0]["type"], "tool_result");
    }

    #[test]
    fn anthropic_tool_result_after_string_user_does_not_corrupt_that_user_turn() {
        // 异常顺序边界：紧跟在「字符串 content 的普通 user 回合」后面来一条 Role::Tool。
        // 折叠守卫用 as_array_mut() 判定——字符串 content 取不到数组，守卫落空，
        // 于是新开一条 user 消息（产出连续两条 user，是退化形态而非把字符串挤成数组破坏原回合）。
        let msgs = vec![
            ChatMessage {
                role: Role::User,
                content: "普通提问".into(),
                tool_calls: None,
                tool_call_id: None,
                name: None,
            },
            ChatMessage {
                role: Role::Tool,
                content: "孤儿结果".into(),
                tool_calls: None,
                tool_call_id: Some("toolu_x".into()),
                name: None,
            },
        ];
        let (_system, out) = messages_to_anthropic(&msgs).unwrap();
        assert_eq!(out.len(), 2);
        // 第一条 user 仍是原字符串，未被改成数组
        assert_eq!(out[0]["role"], "user");
        assert_eq!(out[0]["content"], "普通提问");
        // tool_result 独立新开一条 user
        assert_eq!(out[1]["role"], "user");
        assert_eq!(out[1]["content"].as_array().unwrap()[0]["tool_use_id"], "toolu_x");
    }

    #[test]
    fn anthropic_tool_message_without_tool_call_id_is_err() {
        // tool_call_id 缺失时 Anthropic 无法定位 tool_result，应显式 Err 而非静默丢块
        let msgs = vec![ChatMessage {
            role: Role::Tool,
            content: "无 id 的结果".into(),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }];
        assert!(messages_to_anthropic(&msgs).is_err());
    }
}
