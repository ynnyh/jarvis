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
pub(super) fn build_endpoint_url(raw_base: &str, endpoint: &str) -> String {
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

mod chat;
mod responses;
mod anthropic;

use chat::{chat_via_chat_completions, tool_choice_to_chat};
use responses::chat_via_responses;
use anthropic::chat_via_anthropic;

// ============================================================================
// 流式 Chat Completions（stream: true）
// ============================================================================

/// 发起流式 Chat Completions 请求，每收到一个 text delta 就调用 `on_delta`。
/// 仅 Chat Completions 走真正的 SSE 流式；responses / anthropic 协议不支持标准 SSE，
/// 退化为非流式（内部转调 `chat_with_credentials`，跑完一次性把全文 emit 给 on_delta）。
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
