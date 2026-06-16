use super::*;
use serde_json::Value;
use std::time::Duration;
use crate::settings::LlmCredentials;

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
pub(super) async fn chat_via_anthropic(
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
