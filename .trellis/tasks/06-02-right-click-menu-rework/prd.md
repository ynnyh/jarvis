# 右键菜单精简 + 主题风格系统

## Goal

将 avatar 右键菜单从 10 项精简为 7 项，并内置多套风格主题让用户可切换。

## What I already know

- 菜单定义在 `desktop/src/App.vue` ~540-563 行，10 个 button
- 点击事件对应 menuShowAlerts / menuShowRisk / menuShowReview / menuOpenTodayPlan / menuOpenManualHours / menuOpenChat / menuShowSettings / menuToggleDock / menuCheckUpdate / menuQuit
- "写工时"现在走 `menuOpenManualHours` 打开 ManualHoursApp 独立窗口
- "今日复盘"(ReviewWindow) 已经内置了"一键写工时"按钮（openBatchWrite）
- 配置加载在 `desktop/src/stores/config.ts`
- 设置面板组件在 `desktop/src/components/settings/`

## Decisions

菜单精简（已确认）：
- **去掉**：⚠️ 风险分析、📥 贴边 → 从菜单移除，功能保留在其他入口
- **合并**：✍️ 写工时 → 并入"今日复盘"
- **保留**：🔔 任务提醒、📝 今日计划、💬 聊天、⚙️ 设置、✨ 检查更新、🚪 退出

主题风格（已确认）：
- config 新增 `menuTheme: string` 字段，默认 `'default'`
- 在设置「外观」页加一个下拉选择器，4-5 套内置主题
- 菜单从主题配置读文字和 emoji，不复写死
- 主题定义在独立的 `menu-themes.ts` 文件里

## Menu Themes

### default（正经）
| 键 | 名称 |
|---|---|
| tasks | 🔔 任务提醒 |
| review | 📋 今日复盘 |
| plan | 📝 今日计划 |
| chat | 💬 聊天（大窗） |
| settings | ⚙️ 设置 |
| update | ✨ 检查更新 |
| quit | 🚪 退出 |

### niuma（牛马风）
| 键 | 名称 |
|---|---|
| tasks | 🔔 催债清单 |
| review | 📋 记工分 |
| plan | 📝 画饼 |
| chat | 💬 摸鱼唠嗑 |
| settings | ⚙️ 设置 |
| update | ✨ 检查更新 |
| quit | 🚪 跑路 |

### minimal（极简）
| 键 | 名称 |
|---|---|
| tasks | 🔔 待办 |
| review | 📋 工时 |
| plan | 📝 计划 |
| chat | 💬 聊天 |
| settings | ⚙️ 设置 |
| update | ✨ 更新 |
| quit | 🚪 退出 |

### chuunibyou（中二病）
| 键 | 名称 |
|---|---|
| tasks | 🔔 任务副本 |
| review | 📋 今日结算 |
| plan | 📝 作战部署 |
| chat | 💬 通讯台 |
| settings | ⚙️ 设置 |
| update | ✨ 版本检视 |
| quit | 🚪 登出 |

## Requirements

- [ ] 菜单从 10 项精简为 7 项（去掉风险分析、贴边）
- [ ] 写工时入口合并到今日复盘内
- [ ] 新建 `menu-themes.ts` 内置主题数据
- [ ] config store 新增 `menuTheme` 字段
- [ ] 设置「外观」页加主题下拉选择器
- [ ] App.vue 菜单从主题动态渲染
- [ ] 菜单用 divider 分组：「日常」和「系统」

## Acceptance Criteria

- [ ] 右键菜单显示 7 项，有"日常"/"系统"分组分隔
- [ ] 默认主题名字跟原来一样
- [ ] 切主题后菜单文字和 emoji 跟着变
- [ ] 切主题后刷新/重启不丢失（已持久化）

## Out of Scope

- 自定义输入名字（只用内置主题）
- 菜单项排序自定义
- 风险分析功能本身不移除，只去掉菜单入口

## Technical Notes

- `desktop/src/App.vue` 第 541-563 行：菜单 template → 改为 v-for 动态渲染
- `desktop/src/App.vue` 第 400-530 行：menu 相关 handler 函数
- `desktop/src/stores/config.ts`：新增 `menuTheme` 状态
- `desktop/src/components/settings/`：外观页加主题选择器
- 菜单分组：加一个视觉分隔线，日常组在上，系统组在下
