// Chat agent loop。
//
// 移植自 src/agent/chat-agent.ts + src/tools/chat-send.ts。设计同 TS 版：
//   - LLM 决定调哪些工具，循环执行直到模型不再请求
//   - maxIterations 默认 8，硬截断
//   - 工具失败把 error 作为 tool 消息返回，让 LLM 自己决定下一步
//   - 输出新增消息列表（不含输入）
//
// 写工具（log-task-effort）显式不放进默认白名单——红线：agent 不能直接写禅道。

#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::llm::{self, ChatMessage, ChatRequest, Role, ToolCall, ToolDefinition, ToolDefinitionFunction};
use crate::tools;

pub const DEFAULT_AGENT_TOOLS: &[&str] = &[
    "get_tasks",
    "get_today_tasks",
    "get_task_detail",
    "get_task_commits",
    "analyze_risk",
    "get_daily_review",
    "get_efforts",
    "prepare-log-task-effort",
];

pub fn default_system_prompt(assistant_name: &str, user_title: &str) -> String {
    format!(
        "你是 {}，{}的个人任务助手。在对话里称呼用户为「{}」。\n\
你可以调用工具查询禅道任务、本地 commit、今日复盘、风险分析、工时报表等。\n\
你还可以帮用户准备给禅道任务登记工时（prepare-log-task-effort），但必须等用户确认后才会真正写入。\n\
原则：\n\
1. 用户问到任务/工时/风险/复盘等具体业务问题时，先调相关工具拿真实数据，再回答。不要凭空编。\n\
2. 工具不可用或失败时，明确告诉用户失败原因，不要装作有数据。\n\
3. 回答要简洁直接。日报、风险类的输出去技术化——不要出现 commit/sha/repo 这种词，用项目名 + 任务名组织。\n\
4. 写工时时需要用户提供任务 ID、工时数和工作内容。如果信息不全，主动追问；信息完整后调用 prepare-log-task-effort 生成待确认写入建议，不要直接写入。\n\
5. 查工时时使用 get_efforts 工具，按日期范围查询。用户说今天、本周、本月等模糊范围时，自行计算具体日期。",
        assistant_name, user_title, user_title
    )
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStep {
    pub assistant_message: ChatMessage,
    pub tool_results: Vec<ChatMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunAgentResult {
    pub new_messages: Vec<ChatMessage>,
    pub steps: Vec<AgentStep>,
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub truncated: bool,
}

pub struct RunAgentOptions<'a> {
    pub messages: Vec<ChatMessage>,
    pub allowed_tools: &'a [String],
    pub max_iterations: u32,
    pub temperature: f32,
    pub max_tokens: u32,
    pub system_prompt: Option<String>,
}

pub async fn run_agent(opts: RunAgentOptions<'_>) -> RunAgentResult {
    let mut messages: Vec<ChatMessage> = Vec::new();
    if let Some(sp) = opts.system_prompt {
        messages.push(ChatMessage {
            role: Role::System,
            content: sp,
            tool_calls: None,
            tool_call_id: None,
            name: None,
        });
    }
    messages.extend(opts.messages.into_iter());

    let tools = build_tool_definitions(opts.allowed_tools);
    let mut new_messages: Vec<ChatMessage> = Vec::new();
    let mut steps: Vec<AgentStep> = Vec::new();
    let mut tokens_in = 0u64;
    let mut tokens_out = 0u64;

    for _ in 0..opts.max_iterations {
        let mut req = ChatRequest::new(messages.clone());
        req.temperature = Some(opts.temperature);
        req.max_tokens = Some(opts.max_tokens);
        if !tools.is_empty() {
            req.tools = Some(tools.clone());
        }

        let res = match llm::chat(req).await {
            Ok(r) => r,
            Err(e) => {
                let content = if e.contains("LLM HTTP 502") {
                    "模型服务现在返回 502，通常是上游 LLM 网关临时不可用或线路抖动。请稍后重试；如果大窗仍然正常，重启一下渠道服务再试。".to_string()
                } else {
                    format!("LLM 调用失败：{}", e)
                };
                let err_msg = ChatMessage {
                    role: Role::Assistant,
                    content,
                    tool_calls: None,
                    tool_call_id: None,
                    name: None,
                };
                new_messages.push(err_msg);
                return RunAgentResult {
                    new_messages,
                    steps,
                    tokens_in,
                    tokens_out,
                    truncated: false,
                };
            }
        };
        tokens_in += res.tokens_in;
        tokens_out += res.tokens_out;

        let assistant_msg = ChatMessage {
            role: Role::Assistant,
            content: res.text.clone(),
            tool_calls: if res.tool_calls.is_empty() { None } else { Some(res.tool_calls.clone()) },
            tool_call_id: None,
            name: None,
        };
        messages.push(assistant_msg.clone());
        new_messages.push(assistant_msg.clone());

        if res.tool_calls.is_empty() {
            steps.push(AgentStep {
                assistant_message: assistant_msg,
                tool_results: Vec::new(),
            });
            return RunAgentResult {
                new_messages,
                steps,
                tokens_in,
                tokens_out,
                truncated: false,
            };
        }

        let mut tool_results: Vec<ChatMessage> = Vec::new();
        for call in &res.tool_calls {
            let tool_msg = execute_tool_call(call, opts.allowed_tools).await;
            tool_results.push(tool_msg.clone());
            messages.push(tool_msg.clone());
            new_messages.push(tool_msg);
        }
        steps.push(AgentStep {
            assistant_message: assistant_msg,
            tool_results,
        });
    }

    // 达到 maxIterations
    let stop_msg = ChatMessage {
        role: Role::Assistant,
        content: format!(
            "（达到最大工具调用轮数 {}，强制停止。可能任务过于复杂，或工具结果反复无法收敛。）",
            opts.max_iterations
        ),
        tool_calls: None,
        tool_call_id: None,
        name: None,
    };
    new_messages.push(stop_msg);
    RunAgentResult {
        new_messages,
        steps,
        tokens_in,
        tokens_out,
        truncated: true,
    }
}

async fn execute_tool_call(call: &ToolCall, allowed: &[String]) -> ChatMessage {
    let name = call.function.name.clone();
    if !allowed.iter().any(|a| a == &name) {
        return ChatMessage {
            role: Role::Tool,
            content: json!({ "error": format!("工具 {} 不在允许列表中", name) }).to_string(),
            tool_calls: None,
            tool_call_id: Some(call.id.clone()),
            name: Some(name),
        };
    }
    let args: Value = if call.function.arguments.is_empty() {
        Value::Object(Default::default())
    } else {
        match serde_json::from_str::<Value>(&call.function.arguments) {
            Ok(v) => v,
            Err(e) => {
                return ChatMessage {
                    role: Role::Tool,
                    content: json!({ "error": format!("参数 JSON 解析失败: {}", e) }).to_string(),
                    tool_calls: None,
                    tool_call_id: Some(call.id.clone()),
                    name: Some(name),
                };
            }
        }
    };

    match tools::dispatch(&name, args).await {
        Ok(v) => ChatMessage {
            role: Role::Tool,
            content: truncate_for_context(&stringify(&v), 12_000),
            tool_calls: None,
            tool_call_id: Some(call.id.clone()),
            name: Some(name),
        },
        Err(e) => ChatMessage {
            role: Role::Tool,
            content: json!({ "error": e }).to_string(),
            tool_calls: None,
            tool_call_id: Some(call.id.clone()),
            name: Some(name),
        },
    }
}

fn stringify(v: &Value) -> String {
    if let Some(s) = v.as_str() {
        return s.to_string();
    }
    serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string())
}

fn truncate_for_context(s: &str, max: usize) -> String {
    let count = s.chars().count();
    if count <= max {
        return s.to_string();
    }
    let truncated: String = s.chars().take(max).collect();
    format!("{}\n…（结果过长，已截断到 {} 字符）", truncated, max)
}

// ============================================================================
// 工具 schema 表（内联 JSON Schema）
// ============================================================================
//
// TS 用 zod → JSON Schema 自动生成。Rust 这边手写一份，更可控。
// 字段必须和 tools::dispatch 里每个工具的实际入参对齐。

fn build_tool_definitions(allowed: &[String]) -> Vec<ToolDefinition> {
    let mut out: Vec<ToolDefinition> = Vec::new();
    for name in allowed {
        if let Some((desc, params)) = tool_schema(name) {
            out.push(ToolDefinition {
                kind: "function".to_string(),
                function: ToolDefinitionFunction {
                    name: name.clone(),
                    description: desc,
                    parameters: params,
                },
            });
        }
    }
    out
}

fn tool_schema(name: &str) -> Option<(String, Value)> {
    match name {
        "get_tasks" => Some((
            "获取当前用户在禅道指派给自己的全部任务列表（去掉已关闭/取消的）".into(),
            json!({ "type": "object", "properties": {}, "additionalProperties": false }),
        )),
        "get_today_tasks" => Some((
            "获取今天截止的任务列表".into(),
            json!({ "type": "object", "properties": {}, "additionalProperties": false }),
        )),
        "get_task_detail" => Some((
            "获取单个任务的详细信息".into(),
            json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "任务 ID" }
                },
                "required": ["id"],
                "additionalProperties": false
            }),
        )),
        "get_task_commits" => Some((
            "关联禅道任务与本地 git 提交。按业务线关键词与任务名做软关联，并识别 commit message 中的 #任务号 做精确关联。".into(),
            json!({
                "type": "object",
                "properties": {
                    "range": { "type": "string", "enum": ["today","yesterday","thisWeek","lastWeek","last7days","last30days","thisMonth","all"] },
                    "since": { "type": "string" },
                    "until": { "type": "string" },
                    "rootDir": { "type": ["string", "array"], "items": { "type": "string" } },
                    "includeBody": { "type": "boolean" },
                    "taskIds": { "type": "array", "items": { "type": ["string", "number"] } }
                },
                "additionalProperties": false
            }),
        )),
        "analyze_risk" => Some((
            "分析所有任务的风险（延期、高优先级、依赖）。useLlm=true 时用 LLM 把计数提示升级为具体建议。".into(),
            json!({
                "type": "object",
                "properties": {
                    "useLlm": { "type": "boolean" }
                },
                "additionalProperties": false
            }),
        )),
        "get_daily_review" => Some((
            "生成今日工作复盘：基于本地 commit 与禅道任务关联，输出推进任务、业务线分布、需要更新状态的任务，并附带纯文本日报草稿。".into(),
            json!({
                "type": "object",
                "properties": {
                    "range": { "type": "string", "enum": ["today","yesterday","thisWeek","last7days"] },
                    "since": { "type": "string" },
                    "until": { "type": "string" },
                    "date": { "type": "string" },
                    "hoursPerWorkDay": { "type": "number" },
                    "useLlm": { "type": "boolean" }
                },
                "additionalProperties": false
            }),
        )),
        "get_efforts" => Some((
            "查询帆软报表中的工时明细。可传 range（today/yesterday/thisWeek/thisMonth/thisYear），也可传 begin/end 精确日期；返回每条工时记录及合计。".into(),
            json!({
                "type": "object",
                "properties": {
                    "range": { "type": "string", "enum": ["today","yesterday","thisWeek","thisMonth","thisYear"], "description": "常用日期范围。用户说今天/昨天/本周/本月/今年时优先传这个字段。" },
                    "begin": { "type": "string", "description": "开始日期，格式 YYYY-MM-DD" },
                    "end": { "type": "string", "description": "结束日期，格式 YYYY-MM-DD" },
                    "realName": { "type": "string", "description": "中文姓名，用于过滤本人数据。不传则使用配置中的默认值" }
                },
                "additionalProperties": false
            }),
        )),
        "prepare-log-task-effort" => Some((
            "准备给禅道任务登记工时，但不直接写入。需要任务 ID、工时数和工作内容描述；返回待用户确认的写入建议。".into(),
            json!({
                "type": "object",
                "properties": {
                    "taskId": { "type": "string", "description": "禅道任务 ID" },
                    "hours": { "type": "number", "description": "工时数，必须为正数" },
                    "work": { "type": "string", "description": "工作内容描述" },
                    "date": { "type": "string", "description": "日期，格式 YYYY-MM-DD，不传则默认今天" }
                },
                "required": ["taskId", "hours", "work"],
                "additionalProperties": false
            }),
        )),
        "log-task-effort" => Some((
            "给禅道任务登记工时。需要任务 ID、工时数和工作内容描述。可选指定日期（默认今天）。".into(),
            json!({
                "type": "object",
                "properties": {
                    "taskId": { "type": "string", "description": "禅道任务 ID" },
                    "hours": { "type": "number", "description": "工时数，必须为正数" },
                    "work": { "type": "string", "description": "工作内容描述" },
                    "date": { "type": "string", "description": "日期，格式 YYYY-MM-DD，不传则默认今天" }
                },
                "required": ["taskId", "hours", "work"],
                "additionalProperties": false
            }),
        )),
        _ => None,
    }
}
