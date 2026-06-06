# Jenkins 发版接入 Jarvis（对话式发版）

## Goal

把隔壁已写好的 Jenkins MCP server（`D:\coding\my-mcp-servers\jenkins-mcp`）接入 Jarvis 的 chat agent，
实现「对机器人说一句『给 XX 项目测试环境发个版』→ Jarvis 回显细节确认 → 用户确认后才真正触发 Jenkins 构建」。
核心价值：随时随地（含 IM）**安全**发版——安全是第一约束，发版是高危写操作，绝不能被 agent 自动触发。

## What I already know（前置调研已完成）

### Jenkins MCP server（待接入方）
- 路径 `D:\coding\my-mcp-servers\jenkins-mcp`，TypeScript + `@modelcontextprotocol/sdk`，**stdio transport**，已 npm 装好（需 `npm run build` 出 `dist/index.js`）。
- 8 个工具：只读 6（`test_connection` / `list_environments` / `list_jobs` / `get_job_info` / `get_build_status` / `get_build_log`）+ 写 2（`trigger_build` 发版、`cancel_build` 取消）。
- **无任何确认机制**：`trigger_build` 一调即 POST 触发 Jenkins，无 dry-run / 二次确认。→ 确认 100% 由 Jarvis 侧负责。
- **别名机制**：env `JENKINS_ALIAS_<别名>=[环境名:]真实job`（支持中文）。README 示例即「发版人资管理端」→ 自动映射 job + 账号。→ Jarvis 侧几乎不用做项目俗名→job 映射。
- **环境默认值坑**：`environment` 不传时默认走第一个配置的环境（`envs[0]`），有误发风险。→ 确认必须显式回显环境，LLM 必须明确 environment。
- 配置全走环境变量：`JENKINS_ENV_<NAME>_URL/USERNAME/TOKEN`（多环境多账号）+ `JENKINS_ALIAS_*`。

### Jarvis（接入方）现有架构
- agent loop：`src-tauri/src/chat_agent.rs`（`run_agent` / `run_agent_streaming`，maxIter=8）。
- 工具系统：`src-tauri/src/tools/mod.rs` 的 `dispatch(name, input)` 统一入口 + 手写 JSON Schema（`tool_schema`）；白名单 `DEFAULT_AGENT_TOOLS` + 红线 `AGENT_FORBIDDEN_TOOLS`（写工时 agent 不能直接调）。
- 多渠道：`src-tauri/src/channels/`（telegram / qqbot / router）复用同一 agent。
- **现成确认基建**（关键复用点）：`channels/router/pending_actions.rs`——`PendingAction{id,channel,chat_id,kind,payload,summary,created_at}` 存 `~/.jarvis/channel-pending/`，`maybe_handle_confirmation` 拦「确认/取消」按 `kind` dispatch。目前只接 `log-task-effort` 一种 kind。桌面侧另有 `prepare-log-task-effort` + 确认卡片 UI。
- 安全理念（产品定位）：写操作按风险分级、永不自动、`~/.jarvis/write-back.log` 可追溯、Settings 总开关熔断；本地优先、密码走 OS keychain。

## Confirmed Decisions（2026-06-06 用户已拍板）

1. **发版渠道 = 双端都开**：桌面 + IM（telegram/qq）。发版本就为机器人/远程用。
   → IM 侧必须做身份校验（只认本人 chat_id，防他人冒充触发发版）。
2. **生产环境 = 二次加强确认**：prod 比 test 多一步确认（具体形态待定，见 Open Questions）。
3. **Jenkins token = 走 OS keychain**：同 Jarvis 现有密钥/密码处理，不落明文配置；spawn MCP 子进程时再注入 env。
4. **接入架构 = 完整通用 MCP 平台**：内置可配置的多 MCP server 管理器（`~/.jarvis/mcp-servers.json` + 设置页 UI），任意 MCP server 零代码接入；Jenkins 是第一个挂上去的 server。呼应"Jarvis 做成可扩展助手平台"的长远定位。关键新增设计点：**动态工具安全分类**（运行时发现的工具无法再靠硬编码白名单，需通用机制判定哪些需确认/禁止 agent 直调）。

## Milestones（里程碑，2026-06-06 定）

- **M1 桌面先行（本任务交付重点）**：通用 MCP client 核心（spawn stdio server + initialize + list_tools + call_tool + 工具注入 agent + `mcp__<server>__<tool>` 命名空间路由）+ 挂 Jenkins + 动态工具安全分类 + 桌面对话发版完整闭环（环境显式回显 + 确认 + trigger_build + 构建状态查询）。→ 交付拍板。
- **M2 渠道铺开**：IM（telegram/qq）发版 + 身份校验（chat_id 白名单）+ 生产环境二次加强确认。
- **M3 平台完善**：设置页 MCP server 管理 UI（增删/启停/查看已发现工具）+ 多 server + 构建结果自动轮询推送。

> M2–M3 依赖 M1 拍板，届时各开新任务，不在本任务一次性做完。

## 动态工具安全分类（M1 核心设计，2026-06-06 定）

每个 MCP 工具标一个风险级别，决定 agent 能否直调：
- **confirm（默认）**：agent 不直接执行，生成 pending action，用户确认才真正调。
- **auto**：agent 可在 loop 内直接调（适合纯只读工具）。
- **blocked**：agent 完全不能调（危险工具熔断开关，可选）。

级别从三处来源定，**就高不就低**（冲突取最严）：
1. 用户在 `mcp-servers.json` 的 `toolPolicy` 显式标注（最高优先）。如 Jenkins：`{"trigger_build":"confirm","cancel_build":"confirm","*":"auto"}`。
2. 采信 MCP annotations（server 提供时）：`readOnlyHint`→auto 倾向、`destructiveHint`→confirm。仅作参考。
3. 兜底：无标注无 annotations → **一律 confirm**，绝不默认 auto。

理由：贴合 Jarvis「写操作永不自动 + 用户确认」铁律；不依赖 server 是否给 annotations（jenkins-mcp 老 SDK 没有）；用户可主动把只读工具降 auto。不采用「LLM 预判风险」做门禁（不可靠，仅可作辅助提示）。

## 确认卡片与参数预设（2026-06-06 定）

确认卡片回显字段：**项目（别名）+ 环境（test/prod，必须显式）+ 分支 + 关键构建参数**。
Jenkins 构建参数（用户截图确认的字段，按项目/环境预设）：
- `branch`（测试 dev / 生产 prod）、`node_version`（如 nodejs-18.14.2）、`server_ip`（测试 192.0.2.23 / 生产 192.0.2.162）、`frontend_dir`（如 example-access-web）、`nginx_dir`（如 /data/nginx/html）、`CLEAN_DEPLOY`（是否清理）。
参数来源策略：**一次配好**——在 Jarvis 配置里给「每个项目 × 每个环境」预设上述参数，发版时直接带，不每次问；仅新增项目时改配置。（不走每次 `get_job_info` 动态问参，因这些值极少变动。）
prod 二次加强确认形态：归 M2，本任务（M1）不实现。

## 构建结果反馈（2026-06-06 定）

发版触发后**自动轮询** `get_build_status`：
- 轮询间隔**可配置**（如 30s / 1m）。
- 成功 → 提示成功；失败 → 拉失败日志尾巴（`get_build_log`）+ 提示失败。
- 默认**超时**（如 15 分钟）后停止轮询，提示超时。
- M1 走桌面通知；IM 推送归 M2。

## 配置形态（2026-06-06 定）

M1 先用**手写 JSON 配置**（`~/.jarvis/`，与 `mcp-servers.json` 配合）；设置页 UI 归 M3。
数据模型 = **账号(token) ↔ 项目 一对多**：
- 账号列表：`{ 名称, baseUrl, username, token(keychain) }`
- 项目列表：`{ 别名, 环境表 }`；环境键 = `test`/`prod`，或**只有 prod**
- 每个环境：`{ 引用哪个账号, job 名, 参数预设 }`
- `toolPolicy` 在 server 级别配置（如 Jenkins：`{"trigger_build":"confirm","cancel_build":"confirm","*":"auto"}`）。

## M1 实现计划（小 PR 拆分，2026-06-06 定）

- **PR1 — MCP client 骨架**：rmcp stdio client 接入 spike + `McpClientManager`（读 `~/.jarvis/mcp-servers.json` → spawn stdio 子进程 → initialize → list_tools → call_tool）。验收：能 spawn jenkins-mcp 并列出 8 个工具；暂不接 agent。
- **PR2 — 工具注入 + 路由 + 动态安全分类**：发现的工具注入 `build_tool_definitions`（`mcp__<server>__<tool>` 前缀），`execute_tool_call` 按前缀路由到 `McpClientManager`；三级安全分类（toolPolicy > annotations > 默认 confirm），`trigger_build`/`cancel_build` 判 confirm 并入红线。验收：agent 能直调只读工具；agent 调 `trigger_build` 被拦截不执行。
- **PR3 — 确认闭环 + 参数预设 + 环境回显**：复用 `pending_actions`，新增 `kind:"mcp-deploy"`；桌面确认卡片显示项目/环境/分支/参数预设；确认后才真正调 `trigger_build`；token 走 keychain，spawn 时注入 env。验收：未确认不发版、取消不发版、确认后返回 queueId/构建号；环境显式回显杜绝 envs[0] 误发。
- **PR4 — 构建结果轮询反馈**：触发后按可配间隔轮询 `get_build_status`，成功提示 / 失败拉日志尾巴 + 提示 / 超时停止；桌面通知。验收：三种终态都有反馈。

> M2（IM 双端 + chat_id 白名单 + prod 二次确认）、M3（设置页 MCP 管理 UI + 自动轮询推送）另开任务。

## Open Questions

已全部收敛（见上方「确认卡片与参数预设 / 构建结果反馈 / 配置形态」及「M1 实现计划」各节）。

## Requirements (evolving)

- **通用 MCP client 管理器**：从 `~/.jarvis/mcp-servers.json` 读多个 MCP server 配置，spawn（stdio）+ initialize + list_tools，动态把工具注入 agent 的 `build_tool_definitions`（带 `mcp__<server>__<tool>` 命名空间前缀），`execute_tool_call` 按前缀路由到对应 client。
- **动态工具安全分类**：运行时发现的工具需有通用机制判定哪些「需确认/禁止 agent 直调」（不能再靠硬编码白名单）。
- 设置页可管理 MCP server 列表（增删/启停/查看已发现工具）。
- Jarvis chat agent 能发现并调用 Jenkins MCP 的工具（只读 6 个 + 受控的 trigger_build）。
- `trigger_build` / `cancel_build` 纳入「需确认」红线，agent 绝不直接执行——走 pending action，用户确认才真正调用。
- 确认环节显式回显环境（test/prod），杜绝默认环境误发。
- 双端（桌面 + IM）均可发起发版并完成确认闭环；IM 侧带身份校验。
- 生产环境二次加强确认。
- Jenkins 凭据走 keychain。

## Acceptance Criteria (evolving)

- [ ] 桌面对 Jarvis 说「给 X 测试环境发版」→ 出现含「项目+环境+分支+参数」的确认 → 确认后 Jenkins 真触发、返回 queueId / 构建号。
- [ ] 未确认绝不触发；取消则不发版。
- [ ] IM（telegram/qq）同样走通发版确认闭环，且非本人 chat_id 无法触发。
- [ ] 发往 prod 时有区别于 test 的二次加强确认。
- [ ] Jenkins token 不出现在明文配置文件里（keychain 存储）。
- [ ] 发版后能查询/回报构建状态。

## Definition of Done

- 类型检查 / 构建通过（Rust `cargo` + 前端 `vite build`）。
- 桌面 + 至少一个 IM 渠道实测发版闭环（test 环境）。
- 安全红线有测试覆盖：agent 不能绕过确认直接 trigger_build。
- MCP client 接入方式 + 确认流写入 `.trellis/spec/backend/`。
- 凭据 keychain 存取与现有密钥机制一致。

## Out of Scope (tentative)

- 本任务只实接 Jenkins 一个 MCP server；其它 MCP server 由用户后续自行配置接入（框架支持，但本任务不预置）。
- 不做发版审批工作流 / 多人会签。
- 不做构建产物下载、部署回滚编排（只触发 + 看状态/日志）。
- 不替换 Jarvis 现有 native 工具为 MCP。

## Technical Notes

- 待接入 MCP：`D:\coding\my-mcp-servers\jenkins-mcp`（`src/index.ts` 工具定义、`src/client/jenkins.ts` API、`README.md` 配置说明）。
- 复用点：`src-tauri/src/chat_agent.rs`（build_tool_definitions / execute_tool_call / AGENT_FORBIDDEN_TOOLS）、`src-tauri/src/tools/mod.rs`（dispatch）、`src-tauri/src/channels/router/pending_actions.rs`（PendingAction + maybe_handle_confirmation）。
- Rust MCP client 候选：官方 `rmcp` SDK（stdio transport）。
- 关键安全约束：trigger_build/cancel_build 入 AGENT_FORBIDDEN_TOOLS 同级红线；environment 必须显式；IM chat_id 白名单；prod 二次确认；token keychain。

## Research References

- （待 brainstorm 中按需补充：rmcp stdio client 用法、MCP client 接入 agent loop 的常见模式等）
