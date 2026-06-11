# Research: 模型下载 + 进度 + 确认态（默认关闭/按需下载）

- **Query**: reqwest 流式下载模型存哪（~/.jarvis 还是 appdata）；大文件进度上报前端；确认态 UX；默认关闭→开启接线
- **Scope**: 内部（强复用现有约定）+ external（reqwest stream）
- **Date**: 2026-06-07

## 结论先行

- 模型存到 **`~/.jarvis/models/`**（沿用本仓库既定的 `~/.jarvis` 目录约定，**不要**用 OS appdata，全仓库都用前者）。
- 下载用 **`reqwest` 流式**（本仓库 `Cargo.toml` 已开 `stream` feature，且 `llm.rs` 已有 `bytes_stream()` 用法可照抄）。
- 进度上报前端用 **`app.emit("voice:model-download", {...})`**（照抄 `chat:stream` / `config-changed` 的事件模式）。
- 「默认关闭 + 首次启用要下模型 + 弹框确认」的接线，**完全照搬本仓库 `deployEnabled` 这个 feature-toggle 模式**（下文给出确切代码位置）。

## Findings

### 模型存储位置：必须用 `~/.jarvis/`

本仓库统一把用户级数据放 `~/.jarvis/`（`USERPROFILE` 或 `HOME` + `.jarvis`），已核实多处：

| 用途 | 路径 | 代码位置 |
|---|---|---|
| 主配置 | `~/.jarvis/config.json` | `src-tauri/src/settings.rs:66` `jarvis_dir()` / `:78` `config_path()` |
| 会话 | `~/.jarvis/conversations/` | `src-tauri/src/conversations.rs:13-21` |
| 发版预设 | `~/.jarvis/deploy-presets.json` | `src-tauri/src/tools/deploy.rs:63` |
| MCP servers | `~/.jarvis/mcp-servers.json` | `lib.rs` 注释 / `commands/deploy_config.rs` |
| 写回审计 | `~/.jarvis/write-back.log` | `worklog.rs:304` 等 |
| 成本时薪 | `~/.jarvis/cost-rates.json` | `cost_rates.rs:18` |

**规范的 jarvis_dir 实现**（`settings.rs:66-71`，可直接复用 `crate::settings::jarvis_dir()`）：
```rust
pub fn jarvis_dir() -> PathBuf {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_default();
    PathBuf::from(home).join(".jarvis")
}
```
→ 模型目录用 `crate::settings::jarvis_dir().join("models")`，模型文件如 `~/.jarvis/models/ggml-base-q5_1.bin`。

### reqwest 流式下载（现成范式照抄）

- `Cargo.toml` 已有：`reqwest = { ..., features = ["json", "rustls-tls", "cookies", "socks", "stream"] }` —— **`stream` 已开，无需改依赖**。
- `futures-util = "0.3"` 已在依赖里（流式要用 `StreamExt`）。
- 现成 `bytes_stream()` 用法在 `src-tauri/src/llm.rs:481-495`：
  ```rust
  use futures_util::StreamExt;
  let mut stream = resp.bytes_stream();
  while let Some(chunk_result) = stream.next().await {
      let chunk = chunk_result.map_err(...)?;
      // ... 累加 chunk 写文件 + 累计已下字节
  }
  ```
- 下载实现要点：
  - 从响应头 `Content-Length` 拿总大小，边写文件边累加已下字节 → 算百分比。
  - **写临时文件 + 下载完原子改名**（避免中断留下半截文件被当成完整模型）。本仓库已有 `util::write_atomic`（`settings.rs:74` 注释提到），可参考其原子写思路。
  - 下完**校验**（whisper.cpp 的 HF 模型有已知大小，最好再比对 sha256；HF 仓库可拿到校验值）。
  - 失败要保留可重试（清掉半截 temp）。

### 进度上报前端（事件模式照抄）

本仓库 Rust→前端事件是 `app.emit(...)`，已核实多处：
- `commands/chat.rs:150` `app.emit("chat:stream", &event)`（流式逐 chunk 推）
- `commands/config.rs:50` `app.emit("config-changed", ())`
- `commands/tasks.rs:106` `app.emit("new-tasks-detected", ...)`

前端监听用 `@tauri-apps/api/event` 的 `listen(...)`，已核实于 `stores/config.ts:368-371`：
```ts
listen('reminders-changed', () => { refreshReminders() })
listen('config-changed', () => { load() })
```
→ 下载进度：Rust 端 `app.emit("voice:model-download", { downloaded, total, percent, status })`，前端 `listen('voice:model-download', ...)` 更新进度条。

### 默认关闭 + 启用确认：照搬 deployEnabled 模式

这是用户明确点名的参照。`deployEnabled` 的完整接线（已逐文件核实）：

| 层 | 做法 | 代码位置 |
|---|---|---|
| 配置类型 | `JarvisConfig` 加 `deployEnabled: boolean` 字段 | `desktop/src/stores/config.ts:125` |
| 默认值 | `defaultConfig()` 里 `deployEnabled: false` | `config.ts:209` |
| load 兜底 | `deployEnabled: remote.deployEnabled ?? defaults.deployEnabled` | `config.ts:295` |
| 后端默认 | `default_config()` JSON 里 `"deployEnabled": false` | `src-tauri/src/commands/mod.rs:192` |
| 设置 UI | 一个独立 section + toggle + hint | `desktop/src/components/settings/DeployEnableSection.vue`（全文 18 行，可整段照抄改名） |
| 条件渲染 | 别处用 `config.deployEnabled` 决定是否显示发版项 | `App.vue` 里 `costFeatureEnabled` 同类用法 `:57-61` |

`DeployEnableSection.vue` 模板（直接套用）：
```vue
<input type="checkbox" v-model="store.config.deployEnabled" />
<span>启用对话式发版（Jenkins）</span>
<p class="settings-section-hint">发版属高危操作，默认关闭。开启后...</p>
```

→ **语音输入照此加 `voiceInputEnabled: boolean`（默认 false）**，做一个 `VoiceInputSection.vue`。

### 「首次开启 → 弹框确认是否下模型」的 UX 接线思路

1. 用户在设置里把 `voiceInputEnabled` 打开（toggle）。
2. 前端检测：开启时如果 `~/.jarvis/models/<默认模型>` 不存在 → 弹确认框（「需下载约 57MB 模型，是否继续？」）。
   - 模型存在性检查需要一个后端命令，如 `voice_model_status() -> { exists, path, size }`。
3. 用户点「下载」→ 调 `voice_model_download` 命令，后端流式下，emit 进度，前端进度条。
4. 用户点「否」→ **把 toggle 关回去**（`voiceInputEnabled = false`），不下载。这正是约束 3 的要求。
5. 模型就绪后功能才真正可用（热键/点小人才生效）。

- 弹框可复用本仓库现有弹窗/Toast 组件（`components/ToastContainer.vue`、`PopupMenu.vue`，或独立窗口模式如各 `*_open` 命令）。
- 配置自动保存：`stores/config.ts` 有 250ms 防抖 `watch(config, save)`（`:361-365`），toggle 改动会自动落盘，无需手动 save。

## Caveats / Not Found

- HF 模型的 sha256 校验值需要在实现时从 whisper.cpp 仓库/HF 取（本研究未抓具体哈希）。
- 国内访问 huggingface.co 不稳，下载源策略（官方直链 / hf-mirror.com 镜像 / 允许用户填自定义 URL）是**待用户拍板的决策点**——建议至少提供镜像兜底或自定义 URL 输入。
- `util::write_atomic` 的确切签名未读（只见引用），实现时去 `src-tauri/src/util.rs` 确认。
- 是否要支持「下载中可取消」属体验增强，v1 可不做或简单做。
