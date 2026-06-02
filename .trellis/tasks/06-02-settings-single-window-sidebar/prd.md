# 设置页改造：单窗口 + 侧边栏

## Goal

将设置页从"每页一个独立 Tauri 窗口"改为"单个窗口 + 左侧导航栏"，同时合并粒度太小的页面，从 10 页缩减到 6 页。

## 当前问题

- 每个设置页一个独立窗口，切换频繁且割裂感强
- 粒度不均衡：聊天渠道（Telegram+QQ 超长）vs 开机自启（一个开关）
- "提醒"类拆得太碎：作息规则、主动提醒、定时提醒分散四处

## 目标结构

从 10 页 → 6 页，单窗口 + 可折叠侧边栏：

```
┌──────────────┬────────────────────────────────────┐
│ ▸ 接入 (4)    │                                    │
│    禅道       │        当前选中页面的内容            │
│    工时统计   │                                    │
│    AI 模型    │                                    │
│    聊天渠道   │                                    │
│ ──────────── │                                    │
│ ▸ 工作流 (1)  │                                    │
│    代码与日报 │                                    │
│ ──────────── │                                    │
│ ▸ 提醒 (2)    │                                    │
│    日常提醒   │ ← 合并 作息+主动提醒+定时提醒+今日覆盖 │
│    工时提醒   │ ← 保留独立                          │
│ ──────────── │                                    │
│ ▸ 个性化 (1)  │                                    │
│    外观与行为 │ ← 合并 自启+名称+模式+宠物+点击+今天  │
│ ──────────── │                                    │
│ ▸ 关于 (1)    │                                    │
│    更新日志   │                                    │
└──────────────┴────────────────────────────────────┘
```

## Requirements

### 窗口改造
- 废弃 SettingsWindow（菜单覆盖层）+ SettingsDetailApp（独立窗口）的两窗口模式
- 改为单一 `settings` 窗口，左侧固定侧边栏（~180px），右侧内容区
- 侧边栏分组折叠，当前选中页高亮
- 点击菜单项即时切换右侧内容，无需关窗/重开

### 页面合并
- 日常提醒：合并 WorkDaysSection + WorkPeriodsSection + QuietRulesSection + RitualsSection + WorkdayNudgesSection + TodayOverrideSection + RemindersSection
- 外观与行为：合并 AutoStartSection + AssistantNameSection + WorkStyleSection + PetSection + LeftClickActionSection
- 接入类保持独立（禅道、工时统计、AI 模型、聊天渠道各自一页）
- 工作流保持独立（代码与日报）
- 工时提醒保持独立（逻辑较复杂）
- 关于保持独立

### 侧边栏
- 可折叠分组，已折叠时显示分组名+向右箭头
- 当前页高亮
- 菜单项显示配置状态摘要（已配置/未启用/数量）

### 兼容性
- 现有 `settings_open` 命令参数 `page` 保持有效，打开到指定页
- 设置窗口改为 resizable（已有），尺寸略增以容纳侧边栏

## Acceptance Criteria

- 打开设置只弹一个窗口，左侧有导航
- 所有 6 个页面内容可正常显示和操作
- 合并后的"日常提醒""外观与行为"页面内容完整（不丢字段）
- settings_open('zentao') 等现有调用可直接定位到对应页面
- 关闭设置窗口后重新打开，记住上次浏览的页面（或默认回到首页）

## Out of Scope

- 页面内部逻辑重构（只改页面组织方式，不改各 Section 的内部实现）
- 移动端适配
- 侧边栏宽度可拖动调整（固定宽度即可）

## Technical Approach

### 改造路径

1. **SettingsDetailApp.vue** 改造为有侧边栏布局
2. **SettingsWindow.vue** 废弃，不再使用
3. **settings-menu.ts** 更新页面定义：新增"日常提醒""外观与行为"页面，多组件合并渲染
4. 去掉 `settings_open` 中的 `window.location.reload()` 调用，改为 SPA 内切换路由
5. 窗口已在 tauri.conf.json 注册为 settings，尺寸从 620x540 扩到 820x600

### 关键文件
- `desktop/src/SettingsDetailApp.vue` — 大改：加侧边栏布局
- `desktop/src/settings-menu.ts` — 更新页面定义
- `desktop/src/components/SettingsWindow.vue` — 废弃
- `desktop/src/commands/window.rs` — 删掉 reload hack
- `src-tauri/tauri.conf.json` — 调整 settings 窗口尺寸
- `desktop/src/stores/app.ts` — 可能不再需要 showSettingsWindow 状态
