# Research: 本仓库可复用的现成能力（小人状态 / keychain / 设置范式）

- **Query**: 桌面小人怎么表达状态（录音/转写可视化）；现有全局快捷键/剪贴板用法；settings 开关范式；keychain
- **Scope**: 内部（去读代码核实）
- **Date**: 2026-06-07

## 结论先行

语音输入功能能大量复用本仓库现成机制，**几乎不用从零造轮子**：
- 小人状态可视化：现成 `JarvisState` 状态机 + `PetAvatar` 发光环，**加两个新状态（listening / transcribing）即可**。
- 设置开关：照搬 `deployEnabled` / `costFeatureEnabled` 范式（见 `05-model-download-and-progress.md` 已详列）。
- 后端命令 / 窗口 / 事件：照搬 `commands/` + `app.emit` 模式。
- keychain：本地 STT **不需要任何密钥**（这是它相对云端的优势），keychain 用不上；仅当未来加云端兜底才需要，本仓库 `secret_get/set`（`settings.rs:17-63`）现成可用。

## Findings

### 1. 桌面小人状态可视化（现成状态机，直接扩展）

**真正在用的是 `PetAvatar.vue`（Lottie 宠物 + 发光环），不是 `Avatar.vue`**（后者是旧 mock，`App.mock.vue` 才引用它）。

App.vue 的状态机（已读 `:43-236`）：
- `type JarvisState = 'idle' | 'thinking' | 'working' | 'warning' | 'happy' | 'morning' | 'coffee' | 'late'`（`:43`）
- `const state = ref<JarvisState>('idle')`（`:45`）
- `stateMap: Record<JarvisState, StateConfig>`（`:170`）——每个状态定义 `text/emotion/color/glowColor/animation/description`。例：
  - `thinking`：色 `#3b82f6`（蓝），文案「正在分析」🧠
  - `working`：色 `#10b981`（绿），文案「正在处理」⚙️
- `const current = computed(() => stateMap[state.value])`（`:236`）→ 喂给 `PetAvatar` 的 `:color` / `:glow-color`。
- `<PetAvatar :pet-id :color="current.color" :glow-color="current.glowColor" :active="state==='working'" :flashing="stateFlashing" />`（`:661-667`）。
- 状态切换有 `stateFlashing` 脉冲一次（`:240-245`），给视觉信号。
- `showAlert(text, emoji, state, duration, actions)`（`:301`）会设 `state.value = s` 并起气泡，到点回 `idle`（`:319`）。

**复用方案**：给 `JarvisState` 加 `'listening'`（录音中，建议红/橙色脉冲）和 `'transcribing'`（转写中，建议蓝色，复用 thinking 视觉），在 `stateMap` 加两条配置。录音开始 `state.value='listening'`，转写时切 `'transcribing'`，注入完回 `'idle'`。**PetAvatar 不用改**，它只消费 color/glowColor。

`PetAvatar.vue`（已读）：用 `lottie-web` 渲染 72×72 透明容器 + 发光环 + 状态点，`props` 收 `petId/color/glowColor/active/flashing`，换 petId 重载动画。纯展示，状态判断在 App.vue。

### 2. 设置开关范式（deployEnabled / costFeatureEnabled）

完整接线已在 `05-model-download-and-progress.md` 详列。核心：
- `JarvisConfig` 加 boolean 字段（默认 false），`config.ts` 的 `defaultConfig()` + load 兜底 + 后端 `default_config()` 三处同步。
- 做一个 `XxxSection.vue`（照抄 `DeployEnableSection.vue` 18 行）。
- 条件渲染参考 `App.vue:57-61` 的 `costFeatureEnabled`：
  ```ts
  if (!configStore.config.costFeatureEnabled) {
    return items.filter(i => i.key !== 'cost')
  }
  ```

### 3. 后端命令 / 窗口 / 事件模式

- **命令**：`#[tauri::command] pub async fn xxx(app: AppHandle, ...)`，在 `lib.rs` 的 `invoke_handler![]`（`:246-328`）注册。语音功能加 `voice_input_start/stop`、`voice_model_status/download` 等。
- **后端→前端事件**：`app.emit("事件名", payload)`（`chat.rs:150`、`config.rs:50` 等）；前端 `listen(...)`（`config.ts:368`）。
- **子进程（若走 sidecar plan B）**：`silent_command`（`commands/mod.rs:44-52`，Windows `CREATE_NO_WINDOW` 防黑窗）；`mcp_client.rs` 有成熟 spawn/管理子进程范例；`rmcp` 已带 `transport-child-process`。

### 4. keychain（本地 STT 用不上，仅备查）

- `settings.rs:17-63`：`secret_get/secret_set/secret_clear/secret_exists`，Service 名 `Jarvis-Secrets`，底层 `keyring` crate（`Cargo.toml` 已依赖，含 windows-native/apple-native）。
- 配置里密钥用占位符 `********`，存取走 keychain：`commands/mod.rs:254-299`（`hydrate_secret_placeholders` / `strip_secrets_for_save`）。
- **本地 STT 无密钥**，这套不需要。只有未来加云端 STT 兜底（用户明确说「只对本地模型成立」，即默认不做云端）时才用得上。

### 5. 现有「全局快捷键 / 剪贴板」用法 → 都没有

- 全局快捷键：**无**（见 `06-hotkey.md`）。
- 剪贴板：全仓无 `arboard` / `clipboard-manager` 使用，需新增（见 `04-text-injection.md`）。

## 复用清单（一句话汇总）

| 能力 | 现成？ | 复用点 |
|---|---|---|
| 小人状态可视化 | ✅ | App.vue `JarvisState` + `stateMap` + `PetAvatar`，加 listening/transcribing 两态 |
| 设置开关（默认关） | ✅ | `deployEnabled` 全套接线，照抄 `DeployEnableSection.vue` |
| 配置存取 + 自动保存 | ✅ | `stores/config.ts`（250ms 防抖 watch→save） |
| 后端命令注册 | ✅ | `lib.rs` invoke_handler + `commands/` |
| Rust→前端事件 | ✅ | `app.emit` / 前端 `listen` |
| ~/.jarvis 目录 | ✅ | `settings::jarvis_dir()` |
| reqwest 流式 | ✅ | `Cargo.toml` 已开 stream，`llm.rs` 有范例 |
| 防黑窗子进程 | ✅ | `silent_command` / `mcp_client` |
| keychain | ✅（用不上） | 本地 STT 无密钥 |
| 全局热键 | ❌ 新增 | tauri-plugin-global-shortcut |
| 录音 cpal | ❌ 新增 | — |
| STT 引擎 whisper-rs | ❌ 新增 | — |
| 文字注入 enigo+arboard | ❌ 新增 | — |

## Caveats / Not Found

- 小人是否要在「录音中」额外显示一个明显的红点/波形动画（超出现有发光环表达力）属体验增强，v1 用发光环变色 + 气泡文案「正在听…」即可，波形等二期再说。
- `App.vue` 较大（700+ 行），语音相关 UI 逻辑建议抽成独立 composable（如 `useVoiceInput.ts`），与现有 `composables/` 风格一致，避免继续堆 App.vue。
