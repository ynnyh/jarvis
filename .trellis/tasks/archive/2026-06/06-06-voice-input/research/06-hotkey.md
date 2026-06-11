# Research: 全局热键（触发方式：按住说话 vs 切换）

- **Query**: 本仓库是否已用 tauri-plugin-global-shortcut；push-to-talk vs toggle 哪种适合，怎么落地
- **Scope**: 内部（核实未集成）+ external（插件机制）
- **Date**: 2026-06-07

## 结论先行

- 本仓库**尚未**集成任何全局热键能力（已全仓搜索确认，无 `global-shortcut` / `GlobalShortcut` 任何引用）。需新增 **`tauri-plugin-global-shortcut`**（已核实最新 `2.3.2`）。
- 触发方式 **v1 推荐「toggle（按一下开始录，再按一下停录+转写注入）」作为主路径**，因为 push-to-talk（按住说话、松手停）在全局快捷键插件里**对「按下/松开」两个边沿的可靠捕获更麻烦**，且长按全局键容易和系统/其他软件冲突。toggle 实现简单、心智清晰。
- 同时保留**点桌面小人触发**（约束里要求两个入口都可），点小人走前端已有的点击逻辑，更简单。
- 新增 Tauri 插件的接线，照搬本仓库已有的 **`tauri-plugin-autostart`** 集成方式（最接近的先例）。

## Findings

### 现状：零全局热键基础

- 全仓 grep `global.?shortcut` / `GlobalShortcut` / `register.*shortcut` → **No files found**。
- `Cargo.toml` 插件清单（已读）：`notification` / `updater` / `process` / `autostart` / `log`，**没有 global-shortcut**。
- 前端 `package.json`：有 `@tauri-apps/plugin-autostart` 等，**没有 global-shortcut 的 JS 包**。

→ 全局热键是**纯增量**，需要：后端加 `tauri-plugin-global-shortcut`，capabilities 加权限，（可选）前端加 `@tauri-apps/plugin-global-shortcut`。

### 新增 Tauri 插件的接线模板（照搬 autostart）

本仓库 `lib.rs:36-39` 注册 autostart 插件：
```rust
.plugin(tauri_plugin_autostart::init(
    tauri_plugin_autostart::MacosLauncher::LaunchAgent,
    None,
))
```
全局热键照此在 `tauri::Builder` 链上加：
```rust
.plugin(tauri_plugin_global_shortcut::Builder::new()
    .with_handler(|app, shortcut, event| {
        // 在这里判断 shortcut 命中 + event.state（Pressed/Released）
        // 触发录音开始/停止
    })
    .build())
```

### 权限（capabilities）必须加

本仓库权限集中在 `src-tauri/capabilities/default.json`（已读，含 `core:*` / `notification:*` / `updater:*` / `process:*` / `autostart:default`）。
→ 需要新增 `global-shortcut:default`（或具体 allow-register / allow-unregister）到该文件的 `permissions` 数组。否则前端/后端调用会被 Tauri 权限系统拒。

### push-to-talk vs toggle 详解

| | toggle（推荐 v1） | push-to-talk |
|---|---|---|
| 交互 | 按一下开录，再按一下停录 | 按住录，松手停 |
| 边沿捕获 | 只需 Pressed 边沿，简单 | 需 Pressed+Released 两边沿都可靠，插件支持但更易出问题 |
| 系统冲突 | 短按，冲突概率低 | 长按全局键，与其他软件/系统手势冲突概率高 |
| 误触恢复 | 再按一下即停，可控 | 松手即停，若漏捕松手会一直录 |
| 体验 | 略显「两段式」 | 更自然（像对讲机） |

- `tauri-plugin-global-shortcut` 的 handler 能拿到 `ShortcutState`（Pressed/Released），所以**两种都能做**。
- **v1 先做 toggle**，把 push-to-talk 列为后续体验增强（要做时在 handler 里区分 Pressed/Released 即可）。

### 热键值与可配置性

- 默认热键建议选一个不易冲突的组合（如 `Ctrl+Shift+空格` 之类），具体值待定，建议**做成设置项**（沿用 config.json + 设置面板模式，见 `05-model-download-and-progress.md` 的 deployEnabled 接线）。
- 热键注册时机：app setup 阶段注册；若做成可配置，改键时要先 unregister 旧的再 register 新的。
- **热键只在 `voiceInputEnabled=true` 且模型就绪后才注册**，符合「默认关闭、开启才生效」约束。

### 与点小人触发的关系

- 点小人：前端 `App.vue` 小人元素已有 `@mousedown="onMouseDown"`（`:659`）和点击展开菜单逻辑。语音触发可在小人交互里加一个入口（如菜单项或长按），调后端 `voice_input_start`。
- 两个入口（热键 + 点小人）最终都汇聚到同一个后端「开始录音」命令，逻辑复用。

## Caveats / Not Found

- `tauri-plugin-global-shortcut` 2.x 的 handler 闭包确切签名需以拉到的版本文档为准（2.x 期间 API 有微调）。
- 默认热键的具体键位、是否可配置，是**待用户拍板的决策点**。
- macOS 上注册全局热键可能需要「辅助功能/输入监控」权限（与注入的 enigo 类似），这点要在 macOS 端验证（Windows 一般无此限制）。
- 录音中再次触发热键的行为（停录 vs 忽略）需在状态机里定义清楚。
