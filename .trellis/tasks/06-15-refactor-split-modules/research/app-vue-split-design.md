# Batch 2 设计稿：App.vue → 抽 useWindowOrchestration

> 本笔记是 Batch 2 执行前的设计分析（2026-06-16 通读 App.vue 全文后写）。
> fresh session 直接据此实现，省去重新分析。**先和用户确认方案 A / B 再动手。**

## 现状（语音下线后）

App.vue ≈ 967 行：script setup ~587 行 + template ~110 行 + scoped style ~264 行。
要点：**这不是 llm 那种机械搬运**。App.vue 是高耦合的「编排中枢」，`showAlert` 是枢纽函数。

### script 各块（按职责，非行号——行号会变）
1. **imports**：vue / tauri api / 2 stores / 14 composables / 9 组件 / menu-themes
2. **store + 无依赖 composable**：useAppStore、useConfigStore、useTheme()、useTaskAlerts→refreshAlerts、useTaskCommits→fetchCommits、useDailyReview→openReview
3. **alert/menu state**：`JarvisState` 类型、`state`、`showMenu`、`menuItems`/`dailyMenuItems`/`systemMenuItems`/`costMenuItem`(computed)、`alertText/alertEmoji/alertActions`、`avatarAnchor`
4. **menu helpers**：`closeAllPanels`、`menuOpenTodayPlan`、`menuOpenManualHours`
5. **dock + drag**：`useAvatarDock({avatarAnchor,closeAllPanels,showMenu})` → 解构 12 个；`handleAvatarLeftClick`；`useAvatarDrag({...})` → `onMouseDown`
6. **state map**：`StateConfig`、`stateMap`、`current`、`hasAlert`、`stateFlashing`+watch+flashTimer
7. **`ensureBubbleVisible`**（异步，纠正气泡出屏）
8. **alert system**：`showAlert`、`runAlertAction`、`dismissAlert`、`ignoreEffortClosingToday`、alertTimer
9. **reminder consumers**（都依赖 showAlert）：useEveningReminder / useTodayPlanPrompt / useEffortClosingCheck / useWorkdayNudges / useTimeGreetings / useCursorPassthrough / useScheduledReminders / useUpdater→`updater`(updater.start())
10. **menu actions**：openUpdateWindow / toggleMenu / menuShowAlerts / menuShowRisk / menuShowReview / menuOpenCost / menuQuit / menuShowSettings / menuOpenChat / menuCheckUpdate
11. **任务提醒联动**：`showTaskAlertBubble`、`watch(()=>store.alertLevel)`
12. **onMounted #1**：`await configStore.load()` → store.refreshTaskBindings → loadCustomPets → petId 校验 → onFocusChanged(unlistenFocus) → greetingTimer → watch(alertsLoaded, once)
13. **onMounted #2**：listen('new-tasks-detected')→unlistenNewTasks、listen('settings-detail-closed')→unlistenSettingsClosed
14. **needsWizard**(computed) + **onWizardDone**
15. **onUnmounted**：清 alertTimer/greetingTimer/unlistenNewTasks/unlistenSettingsClosed/unlistenFocus

## 依赖链（拆分成败关键）

- **showAlert 枢纽**：被 reminder consumers(8)、menu actions、watch(alertLevel)、onMounted(greeting)、onWizardDone 调。→ 任何持有 showAlert 的拆分单元必须在 consumers 之前初始化。
- **showAlert / ensureBubbleVisible 反向依赖 useAvatarDock**：`dockEdge`、`isPoked`、`pokeOut`、`getMonitorBounds`、`undockedWinPos`、`setUndockedWinPos`、`animateWindowToLogical`。→ useAvatarDock 必须在 alert 系统之前。
- **closeAllPanels** 被 menu actions、handleAvatarLeftClick、dock、reminder onTrigger 调；它写 `showMenu` + 6 个 store 窗口标志。
- **useAvatarDock 依赖** closeAllPanels / showMenu / avatarAnchor（循环：dock 要 closeAllPanels，alert 要 dock 的函数）→ 初始化顺序：showMenu/avatarAnchor/closeAllPanels → useAvatarDock → alert 系统 → reminder consumers。
- **useAvatarDrag 依赖** avatarAnchor / dockEdge / breakoutFromDock / maybeAutoDock / handleAvatarLeftClick。

## 两个方案（动手前与用户确认）

### 方案 A —— PRD 原方案：单个 useWindowOrchestration
把整个 script（除组件 import）搬进 `composables/useWindowOrchestration.ts`，`export function useWindowOrchestration()` 末尾 `return {…30+ binding}`。App.vue script 仅剩组件 import + `const {…} = useWindowOrchestration()`。
- 优点：照 PRD；改动集中。
- 缺点：本质是「逻辑搬家」非解耦，god composable ~480 行；return 清单巨大易漏。

### 方案 B —— 更内聚（偏离 PRD，但真正降复杂度）
- `useAlertSystem(dockDeps)` → state/alert*/current/hasAlert/stateFlashing/showAlert/runAlertAction/dismissAlert/ensureBubbleVisible
- `useAppMenu({store,configStore,dock,showAlert,openReview,closeAllPanels})` → 全部 menu* + closeAllPanels
- reminder consumers 可留 App.vue 或抽 `useReminderHub(showAlert,...)`
- App.vue 保留：store init、useAvatarDock/useAvatarDrag（与 template 事件紧绑）、onMounted/onUnmounted 时序、组合上面 composable
- 优点：每个 composable 内聚可测；缺点：偏离 PRD「一个」的措辞，需用户点头。

## template 需要的 binding（方案 A 的 return 清单 / 方案 B 的总暴露面，逐个核对勿漏）
`avatarAnchor` `showMenu` `state` `current` `hasAlert` `stateFlashing` `alertText` `alertEmoji` `alertActions` `dailyMenuItems` `systemMenuItems` `costMenuItem` `dockEdge` `isPoked` `updater` `needsWizard` `configStore` `store` `toggleMenu` `menuShowAlerts` `menuShowReview` `menuOpenTodayPlan` `menuOpenCost` `menuOpenChat` `menuShowSettings` `menuCheckUpdate` `menuQuit` `onAvatarHover` `onAvatarLeave` `onMouseDown` `runAlertAction` `dismissAlert` `onWizardDone`

⚠️ 待核对：`menuShowRisk`/`menuShowAlerts` 与 `store.showRiskWindow`——template menu 里没有「风险」按钮（menuShowRisk 似乎是 dead code 或别处触发 showRiskWindow），实现时确认是否保留。

## 风险与验证（前端无 vue-tsc 是最大坑）
- **template binding 漏 return / 响应式丢失 → vite build 不报、运行时才 undefined**。必须逐个核对上面清单。
- composable return `ref`/`computed`，App.vue 解构后 template 自动 unwrap；**不要** toRefs 一个已含 ref 的对象导致双层。
- 相对 import：composable 在 `composables/`，路径 `./xxx` → `../xxx`。
- 初始化顺序见「依赖链」：showMenu/avatarAnchor/closeAllPanels → useAvatarDock → useAlertSystem → reminder consumers → useAvatarDrag。
- 验证三件套：`npx vite build --config vite.config.ts` + `npm run check:text` + **手动跑 app 冒烟**（右键菜单全项、左键面板、拖拽+dock、各类提醒气泡、wizard、更新窗口）。

## 建议执行步骤（渐进，每步 vite build）
1. 抽 `useAlertSystem`（最内聚）→ build。
2. 抽 `useAppMenu` → build。
3. （可选）抽 reminder hub → build。
4. App.vue 精简为 template + 编排骨架 → build + check:text。
5. 跑 app 冒烟，全部交互过一遍。
6. 提交：`refactor(app): App.vue 抽 useWindowOrchestration/...`。

## 验收（拆分任务 PRD §4 Batch 2）
App.vue 显著精简（PRD 目标 <400~500 行；含 264 行 style 时以 script 大幅瘦身为准）、编排逻辑入 composable、vite build + 跑 app 无回归。
