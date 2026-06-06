# LLM Wire 协议（chat / responses / anthropic）

> Jarvis LLM 客户端 `src-tauri/src/llm.rs` 支持三种 wire 协议的派发契约，
> 重点是新增协议时**必须同步放行的 5 处 `wire_api` 收口点**（漏一处即静默退化为 chat）。
> 来源：给 llm.rs 加 Anthropic Messages 协议那轮（修 cc-switch claude 导入「没密钥」+ responses 流式死锁）。

---

## 1. Scope / Trigger

- **触发**：infra/跨层集成 —— 新增对外 LLM wire 协议（HTTP 合约 + 凭证 env 接入 + 前后端 wireApi 透传）。属强制 code-spec 深度。
- **模块**：`src-tauri/src/llm.rs`（协议实现）、`settings.rs`/`commands/llm.rs`（凭证归一）、`tools/cc_switch_import.rs`（导入映射）、前端 `desktop/src/stores/config.ts` + `components/settings/ModelEditor.vue`（类型/下拉/回填）。

---

## 2. Signatures

```rust
// 主入口按 cred.wire_api 三路派发（llm.rs:164-175）
pub async fn chat(req: ChatRequest) -> Result<ChatResponse, String>;
pub async fn chat_with_credentials(req: ChatRequest, cred: LlmCredentials) -> Result<ChatResponse, String>;
//   wire_api=="responses"  -> chat_via_responses
//   wire_api=="anthropic"  -> chat_via_anthropic     ← 本次新增
//   其它(含"chat")          -> chat_via_chat_completions

// 流式入口：仅 chat 协议真流式；responses/anthropic 退化为非流式（llm.rs:421-431）
pub async fn streaming_chat<F: Fn(String)+Send>(req:&ChatRequest, cred:&LlmCredentials, on_delta:F)
    -> Result<ChatResponse, String>;

// Anthropic 实现（llm.rs:907+）
fn build_anthropic_url(raw_base:&str) -> String;                       // 末尾 /v1 则 +/messages，否则 +/v1/messages
async fn chat_via_anthropic(req:&ChatRequest, cred:&LlmCredentials) -> Result<ChatResponse,String>;
fn messages_to_anthropic(messages:&[ChatMessage]) -> Result<(String /*system*/, Vec<Value> /*messages*/), String>;
fn tools_to_anthropic_format(tools:&[ToolDefinition]) -> Value;        // {name,description,input_schema}
fn tool_choice_to_anthropic(tc:Option<&ToolChoice>) -> Value;          // {type:"auto"} | {type:"tool",name}
fn parse_anthropic_output(data:&Value, fallback_model:&str) -> Result<ChatResponse,String>;
```

对外的 `ChatMessage`/`ChatRequest`/`ChatResponse` 永远是 **OpenAI Chat Completions 风格**；各 wire 协议的差异只在本文件内适配，上层（agent loop / ask-llm）无感知。

---

## 3. Contracts

### 3.1 ⚠️ `wire_api` 五处收口点（新增协议红线）

`wire_api` 值在多处被「归一 match」，**默认 `_ => "chat"`**。新增一个协议字符串，必须在**全部**下列点放行，否则该协议在运行时被静默打回 chat、新实现成死代码：

| # | 位置 | 作用 | 形态 |
|---|------|------|------|
| 1 | `settings.rs` `get_llm_credentials` (≈177) | **每次 LLM 调用必经**；从 config 解出运行时 `wire_api` | `match {... Some("anthropic")=>..., _=>"chat"}` |
| 2 | `commands/llm.rs` 测试连接/ask cred 构造 (≈378) | 设置页「测试连接」按钮 | 同上 match |
| 3 | `llm.rs` `chat_with_credentials` 派发 (≈169) | 选实现函数 | `if/else if/else` |
| 4 | `llm.rs` `streaming_chat` 退化判断 (≈425) | 决定是否退化非流式 | `wire_api=="responses" \|\| =="anthropic"` |
| 5 | `tools/cc_switch_import.rs` `resolve_endpoint` (≈323) | 导入时按 app_type 定 wire_api | claude=>"anthropic" |

前端另有 2 处（非 match、但限定联合类型）：`config.ts`（`wireApi?: 'chat'\|'responses'\|'anthropic'` + 加载时保留）、`ModelEditor.vue`（form 类型、cc 导入回填、协议 `<select>` 的 `<option>`）。

> `commands/llm.rs` 的**保存** profile 路径是原样 `insert(w)`、不归一，无需改。识别「收口点」靠 grep `wire_api`/`wireApi` 找带 `_ => "chat"` 或 `=== 'responses' ? : 'chat'` 的归一逻辑。

### 3.2 Anthropic Messages 请求/响应

- **Endpoint**：`POST {base}/v1/messages`。base 来自凭证 base_url（cc-switch claude 存在 `ANTHROPIC_BASE_URL`）。**不要复用 `build_endpoint_url`**——它对带 path 前缀的 host 只追加 `/messages`、漏 `/v1`。
- **Auth header**：`Authorization: Bearer <token>`（`.bearer_auth`），**非** `x-api-key`。因 cc-switch claude provider 存 `ANTHROPIC_AUTH_TOKEN`（Claude Code 用 AUTH_TOKEN 时正走 Bearer）。`anthropic-version: 2023-06-01` 必带。
- **请求体**：`{model, max_tokens(必填,unwrap_or 1024), temperature, system(顶层独立,空则省略), messages, tools?, tool_choice?}`。
- **messages 转换**（`ChatMessage[]` → Anthropic）：
  - `Role::System` → 抽进顶层 `system` 字符串（多条 `\n\n` 拼），不进 messages。
  - `Role::User`/`Assistant`（纯文本）→ `{role, content:<字符串>}`。
  - `Role::Assistant` 带 tool_calls → `{role:"assistant", content:[ {type:"text",text}?, {type:"tool_use",id,name,input} ... ]}`；`input` 须由 `ToolCall.arguments`（JSON 字符串）`from_str` 转回**对象**（失败兜底 `{}`）。
  - `Role::Tool` → **user** 角色的 `{type:"tool_result",tool_use_id,content}`；**相邻多条 `Role::Tool` 折叠进同一条 user 消息的 content 数组**（见 §7）。
- **tools**：`{name, description, input_schema:<JSON Schema>}`（字段名 `input_schema`，不是 `parameters`）。
- **响应解析**：`content[]` 取 `type=="text"` 拼 text、`type=="tool_use"` → `ToolCall{id, name, arguments: input.to_string()}`（对象转回字符串）；`stop_reason`：`tool_use→"tool_calls"`、`max_tokens→"length"`、其它→`"stop"`；usage 字段名 `input_tokens`/`output_tokens`。

### 3.3 cc-switch claude provider 导入映射

`~/.cc-switch/cc-switch.db` 的 `providers` 表，`app_type=="claude"` 与 `"codex"` 存储位置完全不同：

| 字段 | codex (OpenAI) | claude (Anthropic) |
|------|----------------|--------------------|
| key | `settings_config.auth.OPENAI_API_KEY` | `settings_config.env.ANTHROPIC_AUTH_TOKEN` |
| baseUrl | codex TOML `[model_providers.x].base_url` | `settings_config.env.ANTHROPIC_BASE_URL` |
| model | codex TOML `model` | `settings_config.env.ANTHROPIC_MODEL`（默认 `claude-sonnet-4-20250514`） |
| wireApi | TOML `wire_api`（responses/chat） | 固定 `"anthropic"` |

`extract_api_key_from_config` 取 key 顺序：`auth.OPENAI_API_KEY → env.ANTHROPIC_AUTH_TOKEN → env.ANTHROPIC_API_KEY → auth.ANTHROPIC_API_KEY`。`resolve_endpoint(app_type, config)` 统一给 list/import 两处复用，按 app_type 分支返回 `(base_url, model, wire_api)`。

---

## 4. Validation & Error Matrix

| 条件 | 行为 |
|------|------|
| `wire_api` 为未知字符串 | 归一 match 落 `_` → 当 `"chat"` 处理（**这就是漏放行新协议的后果**） |
| Anthropic HTTP 非 2xx | `Err("LLM HTTP {code}: {body前400字}")` |
| Anthropic 响应既无 text 也无 tool_use | `Err("Anthropic 响应既无文本也无 tool_calls: ...")` |
| `Role::Tool` 消息缺 `tool_call_id` | `Err("tool 消息缺少 tool_call_id，Anthropic 无法定位 tool_result")` |
| `streaming_chat` 遇 responses/anthropic | 退化非流式：转调 `chat_with_credentials`、`on_delta(全文)` 一次、返回 `ChatResponse`（对 agent loop 透明） |
| claude provider 无 `ANTHROPIC_AUTH_TOKEN` | `extract_api_key_from_config` 返 None → 导入后 profile 空 key（DoD：曾经的 bug，现已覆盖各路径） |
| temperature > 1.0（anthropic） | **当前无 caller 触发**（实际值 ≤0.4）；若显式传 >1.0，Anthropic 返 400。未加 clamp（避免静默掩盖配置错误 + 协议间行为分叉），列为已知 latent 风险 |

---

## 5. Good / Base / Bad

- **Good**：cc-switch 导入一个 claude provider → wireApi=`anthropic`、key 从 `env.ANTHROPIC_AUTH_TOKEN` 取到 → 设为 active → agent 走 `chat_via_anthropic` → 正常返回 text/tool_calls。
- **Base**：active 是 responses（"公司"）profile → agent 用 `streaming_chat` → 退化非流式 → 不再因「Responses 不支持流式」死锁。
- **Bad**：新加协议但漏改 `settings.rs:177` 收口 → config 里 wireApi 写对了，运行时 `get_llm_credentials` 仍把它归一成 `chat` → 打 OpenAI 格式到 Anthropic 端点 → 困惑性失败。

---

## 6. Tests Required（`llm::tests` / `cc_switch_import` tests）

- `anthropic_tool_result_after_assistant_opens_new_user_not_fold_into_assistant` —— 锁定首条 tool_result 不被误折叠进 assistant 块（agent-loop 关键不变量）。
- `anthropic_tool_result_after_string_user_does_not_corrupt_that_user_turn` —— tool 紧跟字符串 user 回合时不破坏原 user。
- `anthropic_tool_message_without_tool_call_id_is_err` —— 缺 tool_call_id 走 Err。
- `build_anthropic_url_*`、`parse_anthropic_output_*`、tools/tool_choice 格式断言。
- cc_switch：claude AUTH_TOKEN / ANTHROPIC_API_KEY / codex OPENAI 各路径 key 提取；`resolve_endpoint("claude"/"codex")` 的 (base_url,model,wire_api)。
- **不强求**：`streaming_chat` 退化路径需发真 HTTP、无 mock 设施，纯单测不可行；其内部转调的 `chat_with_credentials` 已被覆盖即可，勿写无意义测试。

---

## 7. Wrong vs Correct

### 7.1 新增 wire 协议只改实现、漏改收口点（本次最大坑）

```rust
// Wrong —— 只加了 chat_via_anthropic + 派发分支，没动 settings.rs 的归一 match
wire_api: match profile_s("wireApi").or_else(|| llm_s("wireApi")).as_deref() {
    Some("responses") => "responses".to_string(),
    _ => "chat".to_string(),   // "anthropic" 落这里 → 运行时永远是 chat，新协议成死代码
}
// Correct —— 5 处收口点全部放行（见 §3.1）
    Some("responses") => "responses".to_string(),
    Some("anthropic") => "anthropic".to_string(),
    _ => "chat".to_string(),
```

### 7.2 Anthropic 连续 tool_result 必须折叠进一条 user

```rust
// Wrong —— 每条 Role::Tool 各自成一条 user 消息
// agent 一轮调 2 个工具 → 连续两条 user → Anthropic 400（role 不交替）
out.push(json!({"role":"user","content":[tool_result_1]}));
out.push(json!({"role":"user","content":[tool_result_2]}));
// Correct —— 若上一条已是 user 且 content 是块数组，追加进去；否则才新开 user
if let Some(last)=out.last_mut() {
    if last["role"]=="user" {
        if let Some(arr)=last.get_mut("content").and_then(|v|v.as_array_mut()) { arr.push(block); continue; }
    }
}
out.push(json!({"role":"user","content":[block]}));
```
> 守卫用 `as_array_mut()`：普通 user 回合 content 是**字符串**、取不到数组 → 不会把字符串挤成数组破坏原回合，安全落到「新开 user」。

---

## Design Decision：流式退化而非实现 Anthropic SSE

agent loop 只走 `streaming_chat`。responses 本就无 SSE、anthropic SSE 事件结构（`content_block_delta` 等）与 OpenAI 差异大。选择对二者**退化为非流式**（跑完一次性 `on_delta` 全文），而非各写一套 SSE 解析器——agent 用返回的 `ChatResponse.text/.tool_calls` 做全部逻辑，`on_delta` 仅用于实时显示，退化后只是「整段一次性出现」，对调用方透明，省掉两套易错的流式解析。
