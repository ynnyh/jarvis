# CC Switch 全量 Provider 导入

## 需求

设置页 LLM 模型列表中，新增"从 CC Switch 导入"入口，扫描 `~/.cc-switch/cc-switch.db` 全量 providers，按 `app_type` 分组展示（Claude / Codex），用户勾选后一键导入为 Jarvis llmProfiles。

## 数据源

- **路径**：`~/.cc-switch/cc-switch.db`（SQLite）
- **表**：`providers`
- **字段**：
  - `id` TEXT — provider UUID
  - `name` TEXT — 显示名称
  - `app_type` TEXT — `"claude"` 或 `"codex"`
  - `settings_config` TEXT — JSON 字符串，含：
    - `auth.OPENAI_API_KEY` → apiKey
    - `config` (TOML 文本) → `model`、`model_provider`、`[model_providers.xxx].base_url`、`wire_api`

## 字段映射

### Codex 类型（直接兼容）
- `name` → profile name
- `base_url` → baseUrl
- `model` → model
- `auth.OPENAI_API_KEY` → apiKey（存密钥链）
- `wire_api` → wireApi（默认 "chat"）

### Claude 类型（需适配）
- `name` → profile name（加 "（Claude）" 后缀区分）
- 从 TOML `model_providers` 段取 `base_url`
- `model` → model
- `auth.OPENAI_API_KEY` 或 `auth.ANTHROPIC_API_KEY` → apiKey
- `wireApi` 固定 `"chat"`（Claude API 用 Chat Completions 兼容协议）

## UI 设计

在 LlmSection.vue 的模型列表上方，加一个"从 CC Switch 导入"按钮：
- 点击后调用后端命令，返回全量 providers 列表
- 弹出一个选择面板，分两组：Claude 列表 / Codex 列表
- 每个 provider 显示名称 + model + baseUrl，带 checkbox
- 底部"导入选中"按钮，批量创建 llmProfiles
- 已导入过的（按 baseUrl+model 去重）标记为"已导入"并禁用

## 改动范围

### 后端
- `src-tauri/src/tools/cc_switch_import.rs`：新增 `list_cc_switch_providers()` 函数，扫描全量
- 新增 Tauri command 或扩展现有 tool，返回 `Vec<CcSwitchProvider>`
- 批量导入 command：接收选中的 provider IDs，逐个创建 profile

### 前端
- `LlmSection.vue`：加导入按钮 + 选择面板
- 新增 `CcSwitchImportPanel.vue` 组件（或内联在 LlmSection 中）
- `config.ts`：可能需要加类型定义
