# 架构拆分:voice.rs / llm.rs / App.vue 三大文件模块化

> 阶段一(内功期)任务 3/3,也是最大最危险的任务。
> 前置已就绪:可观测性(tracing 日志)+ 测试网(worklog/settings/核心模块测试 + CI)。
> 目标:把三个"巨型文件"拆成模块,降低单文件复杂度,为后续团队期/开源期的可维护性打基础。

---

## 1. 背景

三个文件已到"再加功能就失控"的临界点:
- **voice.rs(1487 行)**:本地 SenseVoice 引擎 + 云端豆包 ASR(Volc 协议) + 录音控制 + 热键,四块逻辑挤一个文件。
- **llm.rs(1363 行)**:chat / responses / anthropic 三协议实现 + 公共入口,三套协议的转换函数交错。
- **App.vue(904 行)**:主窗口承载一切 —— 14 个 composable 的初始化编排 + 10+ 子窗口组件挂载。

拆分是**纯结构调整,不改行为**。每个子模块独立可测、可读。拆分后单文件 < 500 行为目标。

## 2. 拆分方案

### 2.1 voice.rs → voice/ 模块(4 文件)

当前 voice.rs 内部三大块 + 录音控制,边界清晰:

| 新文件 | 内容 | 来源 |
|---|---|---|
| `voice/mod.rs` | 模块声明 + 公共 API(voice_start/stop/transcribe)+ Recording struct + 热键相关 | voice.rs 的 pub 函数 + Recording |
| `voice/local_engine.rs` | 本地 SenseVoice:sherpa 二进制下载 + 启动 + 本地识别 | MODEL_URL/SHERPA_BIN_URLS 等常量 + 下载逻辑 |
| `voice/cloud_engine.rs` | 云端豆包 ASR:Volc 协议常量(30+ VOLC_*)+ VolcServerFrame + 云端识别流 | 所有 VOLC_* 常量 + Volc 相关函数 |
| `voice/capture.rs` | 录音采集:cpal 流管理 + 采样率转换 + 音量检测 | 采集相关 helper |

预期:mod.rs ~300行 / local_engine ~400行 / cloud_engine ~500行 / capture ~300行。

### 2.2 llm.rs → llm/ 模块(4 文件)

三协议边界由函数名前缀天然划分(`*_anthropic*` / `*_responses*` / `*_chat*`):

| 新文件 | 内容 |
|---|---|
| `llm/mod.rs` | 公共入口:chat / streaming_chat / chat_with_credentials + build_endpoint_url + should_retry_llm_error + 公共类型(Messages/Tools/ToolChoice 等) |
| `llm/chat.rs` | chat_via_chat_completions + tool_choice_to_chat + OpenAI chat-completions 协议适配 |
| `llm/responses.rs` | chat_via_responses + messages_to_responses_input + tool_choice_to_responses + tools_to_responses_format + responses 协议适配 |
| `llm/anthropic.rs` | chat_via_anthropic + messages_to_anthropic + parse_anthropic_output + tool_choice_to_anthropic + tools_to_anthropic_format + build_anthropic_url + Anthropic 协议适配 |

预期:每个协议文件 ~300-400行,mod.rs ~300行。

### 2.3 App.vue → 抽 useWindowOrchestration composable

App.vue 的 904 行里,大部分是窗口编排(14 个 composable 的初始化时序 + 子窗口挂载/卸载)。
拆分方向:把编排逻辑抽到 `composables/useWindowOrchestration.ts`,App.vue 只保留布局模板 + 调用编排。

| 改动 | 内容 |
|---|---|
| `composables/useWindowOrchestration.ts`(新增) | 集中管理:所有 composable 的初始化时序、子窗口(open/close)调度、生命周期(onMounted/onUnmounted) |
| `App.vue`(精简) | 只保留模板(avatar + 子窗口插槽)+ 调用 `useWindowOrchestration()`,目标 < 400 行 |

## 3. 范围

### In scope
1. voice.rs 拆成 voice/ 模块(4 文件)。
2. llm.rs 拆成 llm/ 模块(4 文件)。
3. App.vue 抽 useWindowOrchestration composable。
4. 拆分后所有 mod 声明、use 语句、Tauri 命令注册正确。
5. 拆分后 cargo check + clippy(尽量 -D warnings) + check:text 通过。
6. 拆分后前端 vue-tsc 类型检查通过。

### Out of scope
- **不改任何业务逻辑** —— 拆分是纯 move,函数体一行不动。
- 不重命名公开 API(避免破坏前端 invoke 调用)。
- 不补新测试(现有测试要继续通过,但不新增)。
- 不拆其他文件(fine_report/commands.rs 600+ 行等,后续再说)。

## 4. 执行策略(分三批,每批独立验证)

拆分风险高,分三批做,每批做完编译验证 + 提交,出问题好回滚:

**Batch 1:llm.rs**(风险最低,边界最清晰,函数名前缀天然分组)
- 建 llm/ 目录,移 anthropic 相关 → llm/anthropic.rs,responses 相关 → llm/responses.rs,chat 相关 → llm/chat.rs,其余 → llm/mod.rs。
- 调整 use 语句(模块内 super/crate 引用)。
- lib.rs 的 `mod llm` 不用改(目录形式自动识别)。
- 验证:cargo check + cargo test --lib --no-run。

**Batch 2:voice.rs**(风险中等,常量和 struct 跨块引用)
- 建 voice/ 目录,按 2.1 拆分。
- 注意 VOLC_* 常量只在 cloud_engine 用,SHERPA_* 只在 local_engine 用,Recording 在 mod.rs(公共)。
- 验证:cargo check + cargo test --lib --no-run。

**Batch 3:App.vue**(风险最高,前端无类型检查网,逻辑改动微妙)
- 新建 useWindowOrchestration.ts,把 onMounted 里的初始化时序搬过去。
- App.vue 精简为模板 + 调用。
- 验证:vue-tsc + npm run build。

## 5. 验收标准

| # | 条件 | 验证方式 |
|---|------|---------|
| 1 | voice.rs 拆成 voice/{mod,local_engine,cloud_engine,capture}.rs,单文件 < 600 行 | wc -l |
| 2 | llm.rs 拆成 llm/{mod,chat,responses,anthropic}.rs,单文件 < 500 行 | wc -l |
| 3 | App.vue < 500 行,编排逻辑在 useWindowOrchestration.ts | wc -l |
| 4 | 业务行为不变:现有测试全部通过 | cargo test(CI)|
| 5 | cargo check + clippy 通过(尽量 -D warnings) | 本地 + CI |
| 6 | check:text + vue-tsc 通过 | 本地 |
| 7 | 无业务逻辑改动(纯 move) | code review diff |

## 6. 风险

| 风险 | 应对 |
|------|------|
| 拆分破坏模块可见性(pub/private) | 拆分后逐个验证编译;保持原可见性,必要时加 pub(crate) |
| 前端 App.vue 拆分引入运行时回归 | vue-tsc + 手动启动 app 冒烟;编排逻辑纯搬运不改顺序 |
| cargo test 本地跑不了,拆分回归测不出 | 以 CI 为准;push 到分支看 CI 结果 |
| 跨文件常量/struct 引用断裂 | 先移函数,后移常量;常量跟着唯一使用者走,公共的留 mod.rs |
| 三批工作量巨大 | 分三批独立 commit,每批可单独回滚;允许跨多个 session |

## 7. 不做功能

严格遵循"阶段一不新增功能"。拆分是结构调整,不改任何用户可见行为。
