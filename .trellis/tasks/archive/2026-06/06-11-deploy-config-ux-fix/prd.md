# 对话式发版配置体验重做与连接修复

## Goal

让用户能在设置页**顺畅地**配好 Jenkins 连接并发版，不再被「凭据名」「用户名到底填啥」「弹 node 黑框」「报 Transport closed 卡死」这些问题劝退。
当前 jenkins-deploy 的 M1 后端逻辑基本能跑，但**配置交互极差 + 有几个确诊的 bug**，真实用户（本人）按现状根本配不通。本任务专注「配置 + 连接」这一段的体验重做与 bug 修复，不碰发版触发/轮询主流程。

## What I already know（本次排查已确诊，2026-06-11）

### 已修（本会话内，未提交）
- **username 被重构误删**：jenkins-mcp 的 `parseEnvironments()`（dist/index.js:392）要求 `url && username && token` 三者非空才注册环境，否则 0 环境 → 启动即抛「未配置 Jenkins 连接」退出 → Jarvis 侧「MCP initialize 握手失败: connection closed」。重构把 username 从凭据模型删了（写死 `USERNAME=""`）。已加回 username 全链路（`deploy_config.rs` 模型/写两文件/读视图 + `DeploySection.vue` 输入框 + 保存校验）。**此修复需纳入本任务一并提交。**

### 确诊待修的 bug
1. **保存时弹 node 控制台黑框**（Windows）：`mcp_client.rs::spawn_running`（~448）用 `Command::new("node")` spawn 子进程，未加 Windows `CREATE_NO_WINDOW (0x08000000)` 创建标志 → 每次 spawn/重启都弹一个控制台窗口。`deploy_config_save` 每次保存都 shutdown+respawn，所以保存就弹窗。
2. **「Transport closed」僵尸条目不自愈**：manager 的 `servers: Arc<Mutex<HashMap<String, RunningService>>>` 只凭「map 里有 key」判定 connected（`connected_ids` 不探活）。子进程一旦死掉（rmcp 1.7 `RunningService` drop/cancel → `ChildWithCleanup` kill 子进程），map 里留下死条目；`deploy_test_connection` 见「已连接」跳过重 spawn → `call_tool` 朝死进程发 → rmcp 返回 `Transport closed`（mcp_client.rs:363）。且 `spawn_server`（264）发现 key 还在就早返回，不替换死条目 → 卡死直到整段重启 app。本次靠「从终端干净重启」绕过，但根因未修。
3. **凭据模型过度设计**：当前模型是 `jenkinsUrl + credentials[{name, username, token, projects}]`，多凭据 + 「凭据名（credential name）」概念。用户只有一个 Jenkins、一个账号，却要先懂「凭据名是啥」（它只是 Jarvis 内部 env key 前缀 `JENKINS_ENV_<NAME大写>_*` + keychain account `jenkins-<name>-token` 的来源），交互负担大、用户明确反馈「搞不清这是啥」。
4. **用户名无引导**：Jenkins 认证用 User ID（`username:apiToken` 基本认证），用户分不清 User ID / 登录名 / 显示名，反复 401。jenkins-mcp 有 `/whoAmI/api/json` 可返回认证主体 `name`，可用于「填完 token 自动探测/校验用户名」。
5. **token 保存反馈弱**：保存逻辑「token 留空=不改已有密钥」，用户看不出 keychain 里到底存了啥 token，怀疑「我 token 是对的为啥还失败」时无从自查。

### 关键事实（排查得出）
- jenkins-mcp 本身**完全健康**：独立跑能正常处理 test_connection、空闲 35s 不退、只在 stdin EOF 才退；网络可达（Jenkins HTTP 200）。所有问题在 Jarvis 接入侧。
- rmcp 固定 `1.7.0`（Cargo.toml:46，自 PR1 起；本次未提交改动**未动** rmcp，也未动 `mcp_client.rs`）。
- 涉及文件：`src-tauri/src/mcp_client.rs`（spawn/transport/manager）、`src-tauri/src/commands/deploy_config.rs`（配置读写/test）、`src-tauri/src/tools/deploy.rs`（presets 读 + prepare/confirm + 轮询）、`desktop/src/components/settings/DeploySection.vue`（设置页 UI）。

## Decision (ADR-lite)

**Context**：（1）「凭据名」内部概念让用户困惑；（2）这是面向最终用户的产品功能，须引导式 UI（不能贴原始 JSON）；（3）三级关系：一个 URL → 一组 token（账号）→ 每个 token 对应一个或多个项目，每个项目要有别名。

**Decision**（2026-06-11 用户确认）：引导式表单，三级关系 **URL → 账号(token) → 项目**：
- **URL 全局、一次配置**（顶部一个字段，所有账号共用；**不**下沉到每个账号——上一版 per-account URL 误判，已撤回）。
- URL 下挂**一组账号**，每个账号 = 用户名（=Jenkins User ID）+ token + 项目列表。
- 每个**项目 = job + 别名（必填）**。
- 账号**内部 id 自动生成、对用户隐藏**（无「凭据名」输入）；id 稳定，作 keychain account / `JENKINS_ENV_<id>_*` 前缀来源。
- 引导式 UI（非贴原始 JSON）；token 走 keychain。

**Consequences**：
- 数据模型 = 现状即可：`{ jenkinsUrl(全局), credentials:[{name(隐藏id), username, token, projects:[{job, alias}]}] }`。**不引入 per-account URL**。
- 唯一新增校验：项目别名保存时非空（前端拦 + 后端兜底）。
- 401 根因复盘：用户此前在 Jarvis 表单存的 token ≠ 其外部标准 MCP 配置里的真 token → 引导式 UI 正确录入即可，非文件同步 bug。环境名可含中文（如 `SYSTEM_物流`），但用隐藏 id 规避 `[A-Za-z0-9-]` 限制。
- **现状进度**：PR1（node 黑框 + 僵尸自愈，`mcp_client.rs`）与 PR2（账号卡片 + 隐藏 id + 用户名说明 + 测试友好提示，`DeploySection.vue`）+ username 修复（`deploy_config.rs`）本会话已实现并 cargo check 通过；仅剩别名必填。

## Open Questions

- （已收敛）凭据模型 → 见上方 Decision。

## Requirements (evolving)

- 保存/重启 jenkins-mcp 时**不弹任何控制台窗口**。
- 连接出问题能**自愈**：子进程死后再次操作能自动重连，不需重启整个 app；错误信息可读、可指导下一步。
- 配置表单**直白**：用户不必理解 Jarvis 内部概念就能配好「连哪个 Jenkins、用哪个账号、发哪些项目」。
- **用户名有明确引导**（说明它是 Jenkins User ID；尽量做到填完 token 自动探测/校验账号）。
- 把本会话已做的 **username 修复**一并纳入并提交。

## Acceptance Criteria (evolving)

- [ ] 设置页保存发版配置，全程无 node 黑框弹出。
- [ ] 子进程死亡后，再点「测试连接」能自动重连并返回真实结果（成功/认证失败），不再出现「Transport closed」死局。
- [ ] 新用户照设置页就能配通连接（含明确的用户名引导），不需要看代码或问人。
- [ ] 配置/连接相关逻辑有单测覆盖（纯函数 + 关键路径）。

## Definition of Done

- 类型检查 / 构建通过（Rust `cargo` + 前端构建）。
- 设置页实测：配置 → 保存（无黑框）→ 测试连接通过。
- 僵尸进程自愈有测试或可复现验证。
- 本会话 username 修复 + 本任务改动一起按中文规范提交。

## Out of Scope (tentative)

- 发版触发 / 构建参数选择（`needsParameters` 前端 UI 缺口）/ 轮询推送等主流程——另议（PRD 与实现已就「预设参数 vs 动态 get_job_info」漂移，单独处理）。
- 多 Jenkins server 管理 UI（M3）。
- IM 渠道 / prod 二次确认（M2）。

## Technical Notes

- node 黑框修复：`#[cfg(windows)] use std::os::windows::process::CommandExt; cmd.creation_flags(0x08000000)` 加在 `spawn_running` 构建 Command 处。
- 自愈修复：`call_tool` 命中传输层错误（区别于工具 `is_error`）时，从 map 剔除该 server 并按 mcp-servers.json 重新 spawn 后重试一次；或 `connected`/`spawn_server` 改为探活感知。
- 用户名自动探测：填 token 后调 jenkins-mcp `test_connection`/或直接 `whoAmI` 取 `name` 回填。
- 关联：`.trellis/tasks/06-06-jenkins-deploy/`（M1 母任务，本任务是其配置/连接段的修复重做）。

## Research References

- （如需 rmcp 子进程窗口隐藏 / 自愈重连的最佳实践，再补 research/*.md）
