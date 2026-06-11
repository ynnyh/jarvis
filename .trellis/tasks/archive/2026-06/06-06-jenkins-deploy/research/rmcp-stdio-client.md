# Research: rmcp as a stdio MCP client (for Jarvis `McpClientManager`)

> **One-line summary:** Use `rmcp = { version = "0.8", features = ["client", "transport-child-process"] }`; spawn the server with `TokioChildProcess::new(Command::new("node").arg(...).configure(...))`, then start the session with `().serve(transport).await?` which returns a `RunningService<RoleClient, ()>` whose `.peer()` (or deref) gives you `list_tools` / `call_tool`; `initialize` runs automatically during `serve`.

- **Query**: How to use the official Rust MCP SDK (`rmcp`) as a client over stdio child-process transport, to spawn an external MCP server, initialize, list tools, and call tools — for a Tauri (tokio) app.
- **Scope**: External (crate API) + Internal (read local `Cargo.toml` + jenkins-mcp server source)
- **Date**: 2026-06-06

---

## ⚠️ Research-environment caveat (read first)

In this session **all web/search tools were unavailable** (`mcp__exa__*`, `WebSearch`, `WebFetch` all returned "No such tool available") and **Bash network/`cargo` calls were blocked by the safety classifier** (as the task warned might happen). `rmcp` is **not** present in the local cargo registry cache (`C:\Users\82673\.cargo\registry`), so I could not read the crate source on disk either.

Therefore the **server-side facts (Q9 inputs) are verified from real local source**, but the **rmcp client-side API below is reconstructed from prior knowledge of the `modelcontextprotocol/rust-sdk` repo and docs.rs**, not from a live fetch this session. I have flagged confidence per item. **Before writing PR1, the implementer should run `cargo add rmcp --features client,transport-child-process` and check `cargo doc --open` / docs.rs to confirm the exact 0.x symbol names**, because `rmcp` is pre-1.0 and renames symbols between minor versions. Key things most likely to drift: exact feature-flag names, `serve`/`serve_client` spelling, and the `CallToolRequestParam` field names.

**Authoritative sources to confirm against (not fetched this session):**
- crates.io: <https://crates.io/crates/rmcp>
- docs.rs: <https://docs.rs/rmcp> (look at `rmcp::service`, `rmcp::transport::child_process`, `rmcp::model`)
- GitHub repo + examples: <https://github.com/modelcontextprotocol/rust-sdk> — especially `examples/clients/` (there is a stdio client example that spawns a server with `TokioChildProcess`).

---

## Q1 — Crate, version, feature flags, exact `Cargo.toml` line

- **Crate name:** `rmcp` (this is the official SDK published by the `modelcontextprotocol` org; the repo is `modelcontextprotocol/rust-sdk`). *(High confidence on name.)*
- **Latest stable version:** `0.8.x` is the latest line I'm aware of (the crate has moved 0.1 → 0.2 → … → 0.8 over 2025). **Confirm the exact current patch on crates.io** — pin to the latest `0.8` at implementation time. *(Medium confidence — version could be slightly higher by 2026-06; treat "0.8" as a floor, verify.)*
- **Required features:**
  - **Acting as a client:** `client`
  - **stdio child-process transport:** `transport-child-process` (this feature pulls in tokio process support and the `TokioChildProcess` type). *(Medium-high confidence on both names — these are the documented feature names in the 0.x line. `transport-io` exists separately for raw stdin/stdout / generic AsyncRead+AsyncWrite; you do NOT need it for the child-process case, though `transport-child-process` may enable it transitively.)*
  - You do **not** need a separate `tokio` feature on rmcp; rmcp uses tokio internally and the child-process feature wires it up. You already depend on `tokio` directly (good — you need `tokio::process::Command`).

**Exact dependency line (add to `src-tauri/Cargo.toml` `[dependencies]`):**

```toml
rmcp = { version = "0.8", features = ["client", "transport-child-process"] }
```

If `cargo` complains a feature name is unknown, run `cargo add rmcp` then inspect `cargo metadata`/docs.rs for the real feature list; candidate alternative names to check are `transport-io` and `tokio`. *(Low-confidence fallback note.)*

**Fit with your existing `src-tauri/Cargo.toml`** (verified by reading the file):
- `edition = "2021"`, `rust-version = "1.77.2"` — rmcp 0.8 needs a reasonably recent compiler; **verify rmcp's MSRV ≤ your toolchain** (it may require newer than 1.77; if so you'll bump `rust-version`). *(Flag — verify.)*
- You already have `tokio = { version = "1", features = ["sync", "time", "process", "rt", "macros"] }` — `process` is present, which is exactly what the child-process transport needs. You may also want `"rt-multi-thread"` since Tauri's runtime is multi-threaded (Tauri sets up its own tokio runtime; rmcp will run on it).
- `serde = { ... "derive" }` and `serde_json = "1.0"` are already present — rmcp re-exports/needs serde for tool args/results, so you're covered.
- Note: your release profile has `panic = "abort"`. That's fine for rmcp, but be aware any internal `catch_unwind` semantics differ; not a blocker.

---

## Q2 — Spawn child process, get transport, start client session

**Transport type:** `rmcp::transport::TokioChildProcess` (constructed from a `tokio::process::Command`). It wires the child's stdin/stdout into the MCP framed transport. *(High confidence on concept; confirm exact module path `rmcp::transport::child_process::TokioChildProcess` vs re-export `rmcp::transport::TokioChildProcess`.)*

**Start pattern (the important one):** rmcp uses a `serve`-style API where the *client handler* serves over a transport and returns a running service. For a **bare client with no custom callbacks**, the unit type `()` implements the client handler role, so:

```rust
use rmcp:: serviceExt;          // brings `.serve(...)` into scope  (NOTE: trait is `ServiceExt`)
use rmcp::transport::TokioChildProcess;
use tokio::process::Command;

let transport = TokioChildProcess::new(Command::new("node").arg(server_js_path))?;
let client = ().serve(transport).await?;   // performs initialize handshake, returns RunningService
```

- The returned handle type is **`RunningService<RoleClient, ()>`** (generic over the role marker `RoleClient` and your handler type `()`). *(Medium-high confidence.)*
- You obtain the **peer** (the thing you call methods on) via `client.peer()` which returns a `Peer<RoleClient>` (often just called the "service peer"), **or** the `RunningService` derefs to the peer so you can call `client.list_tools(...)` directly. Use whichever the version exposes; `peer()` is the safe explicit form. *(Medium confidence — confirm whether `RunningService` derefs to `Peer` or you must call `.peer()`.)*

> There may also be a free function / convenience like `serve_client(handler, transport)`; the trait-method form `().serve(transport)` is the idiomatic one shown in repo examples. Confirm the `ServiceExt` import path (likely `use rmcp::ServiceExt;`). The pseudo-import on the first line above is intentionally flagged — **the real trait is `rmcp::ServiceExt`**.

---

## Q3 — Injecting env vars at spawn time

`TokioChildProcess` is constructed from a standard `tokio::process::Command`, so **environment injection is plain `tokio::process::Command` API** — rmcp does not hide it. Set env **before** wrapping in the transport:

```rust
let mut cmd = Command::new("node");
cmd.arg(r"D:\coding\my-mcp-servers\jenkins-mcp\dist\index.js");
cmd.env("JENKINS_ENV_TEST_URL", url)
   .env("JENKINS_ENV_TEST_USERNAME", user)
   .env("JENKINS_ENV_TEST_TOKEN", token);
// optional hardening: cmd.env_clear() first, then re-add only what you want + PATH
let transport = TokioChildProcess::new(cmd)?;
```

- `.env(k, v)`, `.envs(iter)`, `.env_clear()`, `.env_remove(k)` are all available because it's just `tokio::process::Command`. *(High confidence.)*
- **Security note (matches your "env only at spawn time" requirement):** env vars are passed at process creation and are not re-readable by you after spawn; this is the correct channel for the Jenkins creds. Consider `env_clear()` then whitelisting to avoid leaking the parent Jarvis environment into the child. *(This is an observation about the mechanism, not a recommendation to change scope.)*
- One ergonomics caveat: some rmcp versions wrap construction in a builder that takes the `Command` by value and may also let you capture **stderr**. If you want the child's stderr for diagnostics, check for a `TokioChildProcess::builder(cmd)` / a variant returning `(transport, stderr)`; otherwise the child inherits Jarvis's stderr by default. *(Medium confidence — verify if you need stderr capture.)*
- The server reads these exact env names — verified from `jenkins-mcp/src/index.ts` `parseEnvironments()`: it accepts `JENKINS_ENV_{NAME}_URL|USERNAME|TOKEN`, legacy `JENKINS_TEST_*` / `JENKINS_PROD_*`, and default `JENKINS_URL|USERNAME|TOKEN`; aliases via `JENKINS_ALIAS_{NAME}` = `env:jobName`. If **none** are set the server throws and exits — so a spawn with zero Jenkins env will fail the handshake. *(High confidence — read from source.)*

---

## Q4 — Initialize handshake: automatic or explicit?

**Automatic.** When you call `().serve(transport).await`, rmcp performs the MCP `initialize` request/response (and the `notifications/initialized` follow-up) as part of bringing the session up. If `serve` returns `Ok`, the handshake already succeeded and the peer is ready for `list_tools`/`call_tool`. You do **not** call `initialize` yourself. *(High confidence — this is the documented behavior of the `serve` API.)*

**Client info / capabilities you pass:** with the `()` handler, rmcp supplies a default `ClientInfo` (implementation name/version) and default client capabilities. To customize (recommended so the server logs "jarvis" as the client), implement the client handler or pass `ClientInfo`/`InitializeRequestParam` containing:
- `protocolVersion` (rmcp fills the version it supports — see Q9),
- `clientInfo: { name: "jarvis", version: "0.8.3" }` (an `Implementation` struct),
- `capabilities: ClientCapabilities { ... }` (you can leave defaults; you don't advertise roots/sampling unless you implement them).

The type carrying these is roughly `rmcp::model::InitializeRequestParam` / `ClientInfo` with `Implementation` and `ClientCapabilities`. *(Medium confidence on exact struct names — confirm in `rmcp::model`.)* For PR1 the zero-config `().serve(transport)` is sufficient.

---

## Q5 — `list_tools` and the returned tool shape (incl. annotations)

**Call:** the peer exposes both a paged `list_tools` and a "get everything" helper:

```rust
// paginated (pass None for the first/only page):
let page = client.list_tools(None).await?;          // -> ListToolsResult
let tools = page.tools;                              // Vec<Tool>

// or fetch across all pages in one go (name may be list_all_tools):
let tools = client.list_all_tools().await?;          // -> Vec<Tool>
```

*(Medium-high confidence: `list_tools(Option<PaginatedRequestParam>)` exists; the convenience `list_all_tools()` exists in recent versions. Confirm spelling.)*

**Shape of each `rmcp::model::Tool`** *(field names — medium-high confidence, confirm in `rmcp::model`):*

| Concept | rmcp `Tool` field | Type | Notes |
|---|---|---|---|
| Tool name | `name` | `Cow<'static, str>` / `String` | e.g. `"trigger_build"` |
| Description | `description` | `Option<Cow<str>>` | jenkins-mcp always sends one |
| Input JSON Schema | `input_schema` | `Arc<serde_json::Map<String, Value>>` (a JSON-Schema object) | **Important:** in rmcp this is typically an `Arc<Map<String,Value>>`, not a free `Value`. Field is named `input_schema` in Rust; on the wire it's `inputSchema`. |
| Output schema (newer MCP) | `output_schema` | `Option<Arc<Map<String,Value>>>` | May be absent from old TS server |
| **Annotations** | `annotations` | `Option<ToolAnnotations>` | **This is the field you need for security classification.** |

**`ToolAnnotations` (the field you care about for the security feature)** *(medium confidence on struct name/fields — verify):*

```text
ToolAnnotations {
    title:            Option<String>,
    read_only_hint:   Option<bool>,   // wire: readOnlyHint
    destructive_hint: Option<bool>,   // wire: destructiveHint
    idempotent_hint:  Option<bool>,   // wire: idempotentHint
    open_world_hint:  Option<bool>,   // wire: openWorldHint
}
```

- So to read an annotation when present: `tool.annotations.as_ref().and_then(|a| a.destructive_hint)`.
- **Reality check for jenkins-mcp (verified from source):** the server's `handleListTools()` returns objects with only `name`, `description`, `inputSchema` — **it sends NO annotations**. So `tool.annotations` will be `None` for every Jenkins tool today. Your security-classification code must therefore **tolerate `None`** and fall back to its own heuristic/allowlist (e.g. `trigger_build`, `cancel_build` are destructive; `list_*`, `get_*`, `test_connection` are read-only). The rmcp field is the right place to read annotations **if/when** a server sends them. *(High confidence on the "no annotations today" fact — read from `src/index.ts` lines 61–249.)*

---

## Q6 — `call_tool` signature and result shape

**Call:** takes a single param struct `CallToolRequestParam { name, arguments }`:

```rust
use rmcp::model::CallToolRequestParam;
use serde_json::json;

let result = client.call_tool(CallToolRequestParam {
    name: "trigger_build".into(),                 // Cow<'static, str>
    arguments: json!({ "jobName": "my-app", "branch": "main" })
        .as_object().cloned(),                    // arguments: Option<serde_json::Map<String, Value>>
}).await?;                                         // -> CallToolResult
```

- **`arguments` is `Option<serde_json::Map<String, Value>>`** (an object map, wrapped in `Option`; pass `None` for no-arg tools like `list_environments`). Build it from a `serde_json::Value::Object` via `.as_object().cloned()`, or construct the `Map` directly. *(Medium-high confidence — this Option<Map> shape is the rmcp convention; confirm field type.)*
- **`name`** is `Cow<'static, str>` so `"trigger_build".into()` works. *(Medium confidence.)*

**Result shape — `rmcp::model::CallToolResult`** *(medium-high confidence):*

| Concept | Field | Type |
|---|---|---|
| Content blocks | `content` | `Vec<Content>` where `Content` is an enum/struct of `Text`, `Image`, `Resource`, … |
| Error flag | `is_error` | `Option<bool>` (wire `isError`) |
| Structured result (newer) | `structured_content` | `Option<Value>` (may be absent) |

**Reading the text** (jenkins-mcp always returns a single `{ type: "text", text: ... }` block — verified from every handler in `index.ts`):

```rust
// Pull the first text block out of the result:
let text = result.content.iter().find_map(|c| c.as_text().map(|t| t.text.clone()));
// `Content` typically offers `.as_text() -> Option<&RawTextContent>` with a `.text: String` field.
// In some versions content items are `Annotated<RawContent>`; access via `c.raw.as_text()` or a `.as_text()` helper.
```

*(Medium confidence on the exact accessor — `Content`/`RawContent` with `.as_text()` is the pattern; confirm whether it's `c.as_text()` or `c.raw.as_text()`. Worst case, match on the enum variant `RawContent::Text(RawTextContent { text, .. })`.)*

**Error semantics (two distinct layers — important):**
1. **Protocol/transport error** → `call_tool(...).await` returns `Err(rmcp::ServiceError / ErrorData)` (e.g. method not found, child died, bad JSON-RPC). Handle with `?`/match.
2. **Tool-level error** → `Ok(CallToolResult)` with `is_error == Some(true)` and the error message inside a text content block. **This is what jenkins-mcp emits on failure** — verified: its `handleCallTool` catch block returns `{ content: [{ type:"text", text: "Error: ..." }], isError: true }`. So you must check `result.is_error == Some(true)` and surface `content` text; it will NOT come back as a Rust `Err`. *(High confidence — read from `index.ts` lines 279–290.)*

---

## Q7 — Holding the client long-lived in shared tokio/Tauri state

- **Handle type:** `RunningService<RoleClient, ()>` (the value returned by `serve`). The thing you actually call methods on is the **`Peer<RoleClient>`** obtained via `.peer()` — and `Peer` is **`Clone`** (it's a cheap handle around channels), so you can hand out clones to multiple callers. *(Medium-high confidence: `Peer` is `Clone + Send + Sync`. Confirm.)*
- **`Send + Sync`:** both `RunningService` and `Peer<RoleClient>` are `Send + Sync` (the SDK is built for exactly this multi-task use). Safe to store in Tauri shared state. *(Medium-high confidence — verify in docs, but this is the design intent.)*
- **Recommended shared-state pattern for N long-lived clients** (Tauri uses tokio; store in `tauri::State` or a global):

```rust
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use rmcp::{service::RunningService, model::RoleClient};

pub struct McpClientManager {
    // keep the RunningService so the child stays alive; key by server id
    clients: Arc<Mutex<HashMap<String, RunningService<RoleClient, ()>>>>,
}
```

  - Keep the **owned `RunningService`** in the map (dropping it tears the session down — see below). Clone the `Peer` out for individual calls if you don't want to hold the lock across an `await`. Prefer `tokio::sync::Mutex`/`RwLock` (not `std`) because you'll `await` while holding references. *(Pattern observation.)*
  - Because `Peer` is `Clone`, a common refinement is to store `Peer<RoleClient>` for calling **and** keep the `RunningService` (or its cancellation token) somewhere to control lifetime.

- **Shutdown / killing the child cleanly:** the `RunningService` provides a graceful stop — typically **`client.cancel().await`** (consumes/▸signals the service to shut down and the child process is killed/awaited). There is also a cancellation-token accessor and the service exposes a `waiting()`/join handle to await natural termination. **Dropping** the `RunningService` also drops the transport and thus kills/closes the child, but prefer the explicit `cancel().await` for a clean shutdown. *(Medium confidence on method name `cancel()` — confirm; alternatives seen: `.cancel()`, `.shutdown()`. The "drop kills child" behavior is reliable.)*

---

## Q8 — Minimal end-to-end Rust example (compile-plausible against rmcp 0.8)

> Treat symbol names as ~90% — re-confirm `ServiceExt`, `TokioChildProcess` path, and `CallToolRequestParam`/`Content` field names against docs.rs before relying on it.

```rust
use rmcp::ServiceExt;                       // brings `().serve(transport)` into scope
use rmcp::transport::TokioChildProcess;     // stdio child-process transport
use rmcp::model::CallToolRequestParam;
use serde_json::json;
use tokio::process::Command;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 1. Build the child command with one extra env var injected at spawn time.
    let mut cmd = Command::new("node");
    cmd.arg(r"D:\coding\my-mcp-servers\jenkins-mcp\dist\index.js");
    cmd.env("JENKINS_ENV_TEST_URL", "https://jenkins.example.com");
    cmd.env("JENKINS_ENV_TEST_USERNAME", "ci-bot");
    cmd.env("JENKINS_ENV_TEST_TOKEN", "REDACTED_TOKEN");

    // 2. Wrap in the transport and start the client session.
    //    `serve` performs the MCP `initialize` handshake automatically.
    let transport = TokioChildProcess::new(cmd)?;
    let client = ().serve(transport).await?;        // RunningService<RoleClient, ()>

    // (optional) inspect what the server reported during initialize:
    // println!("server info: {:?}", client.peer_info());

    // 3. List tools and print their names.
    let tools = client.list_all_tools().await?;     // Vec<rmcp::model::Tool>
    for t in &tools {
        // annotations will be None for jenkins-mcp today:
        let ro = t.annotations.as_ref().and_then(|a| a.read_only_hint);
        println!("tool: {}  (read_only_hint={:?})", t.name, ro);
    }

    // 4. Call one tool with a JSON argument and print the text result.
    let result = client
        .call_tool(CallToolRequestParam {
            name: "list_environments".into(),
            arguments: json!({}).as_object().cloned(), // Option<Map<String,Value>>
        })
        .await?;

    if result.is_error == Some(true) {
        eprintln!("tool returned an error result");
    }
    if let Some(text) = result
        .content
        .iter()
        .find_map(|c| c.as_text().map(|t| t.text.clone()))
    {
        println!("result text:\n{text}");
    }

    // 5. Shut the child down cleanly.
    client.cancel().await?;                          // or just drop(client)
    Ok(())
}
```

**Likely-to-need-tweaking spots** (call these out in PR review):
- `use rmcp::ServiceExt;` — confirm trait name/path.
- `TokioChildProcess::new(cmd)?` — some versions take the command via a builder or return `(transport, child_stderr)`.
- `client.list_all_tools()` vs `client.list_tools(None)?.tools`.
- `c.as_text()` accessor on content items (might be `c.raw.as_text()` or an enum match).
- `t.annotations` / `read_only_hint` exact names.
- `client.cancel()` shutdown method name.

---

## Q9 — Protocol-version compatibility with the TS server (`@modelcontextprotocol/sdk ^1.0.0`)

**Verified server facts (read from local source):**
- `jenkins-mcp/package.json`: depends on `@modelcontextprotocol/sdk ^1.0.0` (so any 1.x, e.g. 1.x latest at install time).
- `jenkins-mcp/src/index.ts`: uses the **low-level `Server`** (`@modelcontextprotocol/sdk/server`) + `StdioServerTransport`, advertises `capabilities: { tools: {} }`, registers `ListToolsRequestSchema` and `CallToolRequestSchema`. It's a **tools-only stdio server**. No resources/prompts.

**Compatibility assessment:**
- MCP uses a **date-string protocol version** (e.g. `2024-11-05`, `2025-03-26`, `2025-06-18`) negotiated in the `initialize` exchange — **not** semver of the SDKs. The client sends the highest version it supports; the server replies with a version it supports (echoing the client's if compatible, else its own). rmcp and the TS SDK both implement this negotiation, so a modern rmcp talking to a TS `@modelcontextprotocol/sdk ^1.0.0` server will negotiate a common protocol version and interoperate for the tools surface. *(High confidence on the negotiation mechanism; this is core MCP.)*
- **Caveat 1 (version skew):** if rmcp 0.8 only advertises a *newer* protocol version than the installed TS 1.x supports, the TS server should still respond with *its* supported version and rmcp should accept it. But a too-new rmcp could in principle reject a much older server. Given `^1.0.0` resolves to a fairly recent TS SDK, this is **unlikely to be a problem**, but **verify by actually running the handshake** (the cheapest test: `serve` returns `Ok`). *(Medium confidence — recommend an integration smoke test.)*
- **Caveat 2 (annotations):** as noted in Q5, the TS server sends **no tool annotations**, so don't expect `readOnlyHint`/`destructiveHint` from Jenkins tools regardless of protocol version. Not a compat error — just absent data.
- **Caveat 3 (tool errors):** the server returns tool failures as `isError: true` content (not JSON-RPC errors). rmcp surfaces that as `Ok(CallToolResult { is_error: Some(true), .. })` — handle it (Q6). *(High confidence.)*
- **Caveat 4 (`structuredContent`/`outputSchema`):** these are newer MCP additions; the old TS server won't emit them, so treat `output_schema`/`structured_content` as optional/`None`.

**Bottom line:** rmcp 0.8 as a client should interoperate with this tools-only TS stdio server over the negotiated protocol version. Confirm with a live `serve` + `list_tools` smoke test against `node dist/index.js` before building higher-level features.

---

## Findings: files read (internal)

| File Path | What it told us |
|---|---|
| `src-tauri/Cargo.toml` | edition 2021, rust-version 1.77.2; `tokio "1"` already has `process` feature; serde/serde_json present; release profile `panic="abort"`, `lto="thin"`. No rmcp yet. |
| `D:\coding\my-mcp-servers\jenkins-mcp\package.json` | Server uses `@modelcontextprotocol/sdk ^1.0.0`, started via `node dist/index.js` (`main: dist/index.js`). |
| `D:\coding\my-mcp-servers\jenkins-mcp\src\index.ts` | Low-level `Server` + `StdioServerTransport`; 8 tools (`list_environments`, `list_jobs`, `get_job_info`, `trigger_build`, `get_build_status`, `get_build_log`, `cancel_build`, `test_connection`); tools have `name`/`description`/`inputSchema` only — **no annotations**; results are single text blocks; errors via `isError:true`; reads Jenkins creds from env (`JENKINS_ENV_{NAME}_*`, legacy `JENKINS_TEST_*`/`JENKINS_PROD_*`, default `JENKINS_*`, aliases `JENKINS_ALIAS_*`) and **exits if none set**. |

## Caveats / Not verified this session

- **rmcp client API names are from prior knowledge, not a live fetch** (web tools unavailable; Bash/cargo blocked; rmcp not in local cargo cache). Confidence flagged per item above. **Must confirm** against docs.rs/repo examples before merge:
  1. exact latest `0.8.x` version + that `client` and `transport-child-process` are the real feature names;
  2. `ServiceExt` trait + `().serve(transport)` spelling and `TokioChildProcess` constructor signature;
  3. `Tool` / `ToolAnnotations` field names (`input_schema`, `annotations`, `read_only_hint`, …);
  4. `CallToolRequestParam.arguments` type (`Option<Map<String,Value>>`) and `Content::as_text()` accessor;
  5. shutdown method (`cancel()` vs `shutdown()`), and that `Peer`/`RunningService` are `Send + Sync` + `Peer: Clone`;
  6. rmcp's MSRV vs your `rust-version = "1.77.2"`.
- **Fastest verification path** for the implementer: `cargo add rmcp --features client,transport-child-process`, then `cargo doc -p rmcp --open` and open the `examples/clients/` stdio example in the `modelcontextprotocol/rust-sdk` checkout.
