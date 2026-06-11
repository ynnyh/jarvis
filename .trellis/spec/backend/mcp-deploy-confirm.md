# 对话式发版确认流（prepare / confirm 拆分）

> 高危 MCP 写操作（触发 Jenkins 构建）的「agent 提案 → 用户确认 → 真执行」闭环。
> 来源：jenkins-deploy PR3，`src-tauri/src/tools/deploy.rs`。
> 前置：工具注入/门禁见 [mcp-agent-integration.md](./mcp-agent-integration.md)；两层错误/spawn 见 [mcp-client.md](./mcp-client.md)。
> 本篇是 PR2 门禁的下游闭环：PR2 把 `Confirm` 工具「拦下不执行」，PR3 给出**正确的执行路径**。

---

## ⚠️ 重做更新（2026-06-11，v2，commit 662f31c）

下方 §2–§3.1 描述的是 PR3 的**预设参数模型**（`projects→environments→{job,jenkinsEnvironment,params}`），已被重做取代。当前实现契约**以本节为准**；§3.4 确认执行路径（卡片 → tool_execute → confirm-deploy）与 §7 安全红线**不变**。

### 配置模型（`~/.jarvis/deploy-presets.json`）

```json
{ "jenkinsUrl": "http://...", "credentials": [
    { "name": "acct-3f9a", "username": "example-cloud", "token": "keychain:jenkins-acct-3f9a-token",
      "projects": [ { "job": "example-quality-web", "alias": "质量系统" } ] } ] }
```

- `name` = **账号内部 id（自动生成、对用户隐藏）**，作 keychain account(`jenkins-<id>-token`) 与 `JENKINS_ENV_<id大写>_*` 前缀来源；用户只填 用户名 / token / 项目（job+别名）。
- `username` **必需**（Jenkins 鉴权是 `username:token`，重构曾误删致 jenkins-mcp `parseEnvironments` 0 环境而启动崩）。`jenkinsUrl` **全局**一次配置（不再 per-credential）。三级关系：URL → 一组账号(token) → 每账号若干项目(job+别名)。
- 设置页后端 `commands/deploy_config.rs`：`deploy_config_get/save` 读写本文件 + mcp-servers.json + keychain。`save` 顺带给 jenkins server 写**安全默认 toolPolicy**（空时）：`{"trigger_build":"confirm","cancel_build":"confirm","*":"auto"}`——否则空策略被分类器默认 confirm，把只读 `get_job_info` 也拦死、发版读不到参数。项目别名保存校验非空。

### prepare-deploy 两阶段（`tools/deploy.rs`）

- **不带 `parameters`** → `build_deploy_lookup`：按 alias 匹配 job，返回 `{ needsParameters:true, job, credentialName, jenkinsUrl, project, environment, branch }`，引导 agent 调 `mcp__jenkins__get_job_info` 拉构建参数。
- **带 `parameters`**（object）→ `build_deploy_card`：返回确认卡片 `{ pendingWrite:true, kind:"mcp-deploy", summary, payload:{ server:"jenkins", tool:"trigger_build", args:{ jobName, branch?, parameters } } }`，前端渲染带按钮卡片。
- schema 新增可选 `parameters`(object)。系统提示要点：get_job_info 只读**免授权**；用户**点卡片按钮**才算确认，打字说「确认」不算数。

### deploy_test_connection 改为直连 HTTP（不经 MCP spawn）

`deploy_test_connection(name, url, username, token?)`：直接 `GET {url}/api/json` 做 Basic 认证验证**当前表单填写**的凭据——**不必先保存、不依赖 spawn jenkins-mcp**（契合 填→测→存 习惯；旧实现要先保存再测、测的是已存旧值，改 token 不存就测不通）。token 留空回退取 keychain 已存的。200→`{ok:true, detail(含 x-jenkins 版本)}`；401→「认证失败」；403→「无权限」。

### 构建状态轮询（PR4）

`trigger_build` 触发后通常只返回 **queueId（≠ buildNumber）**。轮询 `get_build_status` **绝不能拿 queueId 当 buildNumber**（jenkins `/job/X/{queueId}` 404 → 前端「构建状态查询出错」）；**不传 buildNumber** 让其取 `lastBuild`（触发后最新构建即本次），拿到后锁定真实构建号跟踪到终态（SUCCESS/FAILURE/ABORTED/超时）。

---

## 1. Scope / Trigger

- **触发**：高危写操作的确认闭环——跨「agent 提案工具 / 前端确认卡片 / `tool_execute` 真执行 / MCP `call_tool`」多个边界 + 新配置文件 + keychain 注入，属强制 code-spec 深度。
- **安全第一**：发版绝不能被 agent 自动触发（PRD 铁律）。agent 只能产出**提案**，真正触发必须由用户在卡片上点确认。

---

## 2. Signatures（`tools/deploy.rs`）

```rust
// agent 可调：生成待确认建议，唯一副作用是读磁盘配置
pub(crate) async fn prepare_deploy(input: Value) -> Result<Value, String>;
// 纯函数（无 IO）：校验 + 组 payload + 组 summary。单测直接打它，测的就是上线代码。
fn build_deploy_proposal(config: &DeployConfig, project: &str,
                         environment: &str, branch: Option<&str>) -> Result<Value, String>;

// NOT agent 可调（在 AGENT_FORBIDDEN_TOOLS）：真正调 MCP trigger_build
pub(crate) async fn confirm_deploy(input: Value) -> Result<Value, String>;

// 配置模型（serde camelCase）
struct DeployConfig { server: String /*默认 "jenkins"*/, projects: BTreeMap<String, ProjectPreset> }
struct ProjectPreset { environments: BTreeMap<String, EnvironmentPreset> }   // 键 = test/prod
struct EnvironmentPreset { job: String, jenkins_environment: String, params: BTreeMap<String,String> }
fn load_deploy_config() -> Result<DeployConfig, String>;                     // ~/.jarvis/deploy-presets.json
fn parse_build_identifiers(text: &str) -> (Option<Value>, Option<Value>);    // 尽力捞 queueId/buildNumber
fn append_deploy_audit(ok, server, tool, args, result, error);              // → ~/.jarvis/write-back.log
```

注册与门禁归属：

| 位置 | 内容 |
|---|---|
| `tools/mod.rs::dispatch` | `"prepare-deploy" => deploy::prepare_deploy`、`"confirm-deploy" => deploy::confirm_deploy` |
| `chat_agent.rs::DEFAULT_AGENT_TOOLS` | 含 `"prepare-deploy"`（提案可调）；**不含** `confirm-deploy` |
| `chat_agent.rs::AGENT_FORBIDDEN_TOOLS` | `["log-task-effort", "confirm-deploy"]`（红线：agent 永不直调真执行） |
| `chat_agent.rs::tool_schema` | `prepare-deploy` 的 schema：`project` + `environment` 必填，`branch` 可选 |
| `channels/router/pending_actions.rs` | `"mcp-deploy" => tools::dispatch("confirm-deploy", payload)`（IM/M2 消费侧预埋） |

---

## 3. Contracts

### 3.1 配置文件 `~/.jarvis/deploy-presets.json`

```json
{ "server": "jenkins",
  "projects": {
    "人资管理端": {
      "environments": {
        "test": { "job": "example-access-web-test", "jenkinsEnvironment": "test",
                  "params": { "branch": "dev", "node_version": "nodejs-18.14.2",
                              "server_ip": "192.0.2.23", "CLEAN_DEPLOY": "false" } },
        "prod": { "job": "example-access-web-prod", "jenkinsEnvironment": "prod",
                  "params": { "branch": "prod", "server_ip": "192.0.2.162" } } } } } }
```

- **凭据不在这里**：account/baseUrl/token 走 `mcp-servers.json` 的 env（keychain 注入，见 mcp-client.md §3.2）。本文件只管「发哪个项目的哪个环境、带什么构建参数」。
- `jenkinsEnvironment` = 传给 `trigger_build` 的 `environment`（对应 `JENKINS_ENV_<NAME>_*`）；`job` 可填真实 job 或 jenkins-mcp 别名。

### 3.2 `prepare_deploy` 入/出

- **入**：`{ project: String, environment: String /*必填*/, branch?: String }`。
- **出**（前端确认卡片据此渲染）：

```json
{ "pendingWrite": true, "kind": "mcp-deploy",
  "payload": { "server": "jenkins", "tool": "trigger_build",
               "args": { "jobName": "...", "environment": "test", "parameters": { "branch": "dev", "...": "..." } } },
  "summary": "项目: ...\n环境: test\n分支: dev\nJob: ...\n参数: ...",
  "message": "已准备发版建议，请用户确认后再执行。" }
```

### 3.3 `trigger_build` 入参形态（已对 jenkins-mcp `src/index.ts` 核实）

`{ jobName, environment?, parameters?(嵌套对象), branch? }`，**构建参数嵌套在 `parameters` 子对象**，job 字段名是 `jobName`（非 `job`）。

> ⚠️ **分支必须放进 `parameters.branch`（小写），绝不走顶层 `branch`**：jenkins-mcp 把顶层 `branch` 映射成**大写 `BRANCH`** 构建参（`triggerBuild` handler `buildParams["BRANCH"]=branch`），而这些 job 要的是小写 `branch`。传错位置 → 发错分支且无报错。

### 3.4 确认执行路径（M1 桌面）

```
agent 调 prepare-deploy（无副作用）
  → 前端识别 tool 消息 name=="prepare-deploy" && kind=="mcp-deploy" → 渲染确认卡片（回显 summary）
  → 用户点「确认」→ invoke('tool_execute', { name:"confirm-deploy", input: payload })
  → tools::dispatch("confirm-deploy", payload)（无门禁）→ manager().call_tool("jenkins","trigger_build",args)
```

- `tool_execute → tools::dispatch` **不过 agent 门禁**（门禁在 `execute_tool_call`，仅 agent loop 路径）。这正是「用户点确认」能真执行、而 agent 不能的机制——与 `prepare-log-task-effort`/`log-task-effort` 完全同构。
- `confirm_deploy` 入参 = `prepare_deploy` 产出的 `payload`（`{server, tool, args}`）。

---

## 4. Validation & Error Matrix

| 条件 | 行为 |
|------|------|
| `prepare`：`project` 空 | `Err("必须指定发版项目")` |
| `prepare`：`environment` 空/缺 | `Err("必须显式指定环境（test/prod），不能省略")`——**绝不默认第一个环境**（堵死 jenkins-mcp `envs[0]` 误发） |
| `prepare`：未知 project | `Err("未配置项目 X")` |
| `prepare`：project 无该 env | `Err("项目 X 未配置 <env> 环境")` |
| 配置文件不存在 | `Err("未配置发版项目，请先在 ~/.jarvis/deploy-presets.json 配置")`——**与 mcp-servers.json 宽容缺省不同**（无预设发版无意义，显式引导去配） |
| 配置 JSON 坏 | `Err("解析 deploy-presets.json 失败: ...")` |
| `confirm`：`tool != "trigger_build"` | `Err("confirm-deploy 只允许 trigger_build")`——纵深防御，杜绝「调任意 MCP 工具」后门 |
| `confirm`：`args` 非 JSON 对象 | `Err("confirm-deploy 的 args 必须是 JSON 对象")` |
| `confirm`：传输/协议错 | `call_tool` 返回 `Err` → 审计(false) + 传播（见 mcp-client.md §3.3） |
| `confirm`：工具失败 | `Ok(is_error==Some(true))` → 取 `first_text` → 审计(false) + `Err` |
| `confirm`：成功 | 审计(true) + `Ok({ ok:true, queueId, buildNumber, raw })`（`queueId/buildNumber` 尽力解析，`raw` 必带） |

---

## 5. Good / Base / Bad 用例

- **Good**：`prepare-deploy{project:"人资管理端",environment:"test"}` → 卡片回显 test/dev → 用户确认 → `confirm-deploy` → 返回 `queueId`。
- **Base**：未建 `deploy-presets.json` → `prepare-deploy` 报错引导用户去配，不 panic、不空发。
- **Bad**：agent 在 loop 内直调 `confirm-deploy`（被 `AGENT_FORBIDDEN_TOOLS` 拦）或 `mcp__jenkins__trigger_build`（被 PR2 `Confirm` 门禁拦）→ **都不执行**。

---

## 6. Tests Required（`deploy::tests` + `chat_agent::tests`）

- `deploy_config_parses_minimal` / `deploy_config_server_override`——serde 缺省（`server` 默认 jenkins）。
- `prepare_deploy_requires_explicit_environment`——空环境 / 未知 project / 缺 env 三种 `Err`（打**纯函数** `build_deploy_proposal`，非测试副本）。
- `prepare_deploy_happy_path`——断言 `kind=="mcp-deploy"`、`args.jobName`、`args.environment==jenkinsEnvironment`、参数**嵌套在 parameters**、**无顶层 branch**、`summary` 含环境与分支。
- `prepare_deploy_branch_override`——显式 branch 覆盖 `parameters.branch`；prod 环境回显 prod。
- `confirm_deploy_rejects_non_trigger_tool`——`tool!=trigger_build` 立即 `Err`，**不发起 live 调用**。
- `parse_build_identifiers_best_effort`——JSON 带 `queueId` 能解析；非 JSON → 全 None（靠 raw 兜底）。
- `chat_agent::tests::confirm_deploy_in_forbidden_list`——`confirm-deploy ∈ AGENT_FORBIDDEN_TOOLS`、`log-task-effort` 红线不回归、`prepare-deploy ∉` 禁用名单。
- `chat_agent::tests::prepare_deploy_is_agent_callable_with_schema`——`prepare-deploy ∈ DEFAULT_AGENT_TOOLS` 且有 schema、`project`/`environment` 必填。
- 真·发版（live `trigger_build`）须 `#[ignore]`：依赖真 jenkins-mcp + 真凭据，本机无 `mcp-servers.json` 时单测绝不依赖它。

---

## 7. Wrong vs Correct

### 7.1 分支位置

```rust
// Wrong —— 走顶层 branch，被 jenkins-mcp 映射成大写 BRANCH，这些 job 收不到 → 发错分支
args.insert("branch", json!(branch));
// Correct —— 放进 parameters.branch（小写），与 job 期望的构建参名一致
parameters.insert("branch".into(), json!(branch));   // args.parameters = parameters
```

### 7.2 环境绝不默认

```rust
// Wrong —— 省略/默认环境 = 可能误发到 prod（jenkins-mcp 不传则走 envs[0]）
let env = input.environment.unwrap_or_else(|| first_env());
// Correct —— 空即报错，强制显式；environment 显式回显到 summary 供人核对
if environment.is_empty() { return Err("必须显式指定环境（test/prod），不能省略".into()); }
```

### 7.3 真执行工具不能进 agent 可调表

```rust
// Wrong —— 把 confirm-deploy 放进 DEFAULT_AGENT_TOOLS → agent 能自己发版
// Correct —— confirm-deploy ∈ AGENT_FORBIDDEN_TOOLS 且 ∉ DEFAULT_AGENT_TOOLS；
//            只经 tool_execute（用户点卡片）或 channels 确认流触达
```

---

## Design Decision：为何拆成 prepare / confirm 两个工具

**Context**：发版是高危不可逆写操作，必须「用户确认才执行」，且 agent 绝不能绕过确认自己触发。

**决策**：完全复刻既有 `prepare-log-task-effort`（提案、agent 可调、无副作用）/ `log-task-effort`（真写、`AGENT_FORBIDDEN_TOOLS`、仅前端经 `tool_execute` 触达）模式：

- **agent 只摸得到 `prepare-deploy`**：它只读配置、组一张待确认卡片，没有任何副作用，被 agent 误调也无害。
- **`confirm-deploy` 是真执行**：在禁用红线里、不在默认白名单里；唯一入口是用户点卡片后前端的 `tool_execute`（该路径无门禁），或 channels 确认流。
- **门禁天然分层**：agent loop 路径有门禁（`execute_tool_call` 查 `AGENT_FORBIDDEN_TOOLS` + MCP `Confirm` gate），用户确认路径无门禁——同一份 `tools::dispatch` 两条路、两种待遇，安全边界清晰。

**可复用性**：任何未来的高危 MCP 写操作都应照此办——加一个 `prepare-X`（agent 可调、纯提案）+ 一个 `X`（禁用、前端 reinvoke），而不是放宽门禁让 agent 直调。`confirm_deploy` 内 `tool=="trigger_build"` 断言确保它不退化成通用「调任意工具」后门。
