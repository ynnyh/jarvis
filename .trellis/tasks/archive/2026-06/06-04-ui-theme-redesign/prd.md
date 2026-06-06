# 前端风格主题系统与控件交互重设计

## Goal

把 Jarvis 桌面端的视觉风格做成可切换的"风格主题"系统（如科幻风/俏皮风），
作为与小人皮肤（pet）并列、互相独立的视觉维度；同时重设计当前粗糙的原生控件（如成本页的
checkbox），建立一套交互良好、可主题化的共享控件。核心诉求是**交互体验**，
不是简单换皮。

## What I already know（代码现状）

- **丑控件位置**：`desktop/src/CostApp.vue` — 含离职 checkbox（L346-349）、
  含加班 checkbox（L376-379），均为原生 `<input type="checkbox">` + `.overtime-check`。
- **小人皮肤系统**：`desktop/src/petManifest.ts` 已有 10 个 pet，分 3 类
  （mecha 机甲 / pet 宠物 / character 人物），通过 `config.petId` 选择（默认 robo）。
  选择 UI 在 `desktop/src/components/settings/PetSection.vue`。
- **已有"主题"概念但只管文案**：`config.menuTheme` → `menu-themes.ts`，是右键菜单
  **措辞人格**（正经/牛马/极简/中二），**不涉及视觉样式**。视觉风格是全新维度。
- **无设计 token 系统**：`style.css` 用 Tailwind v4 + 硬编码 rgba（`.glass` =
  `rgba(15,23,42,0.92)`），无 CSS 变量。做主题需引入设计 token（CSS 变量）。
- **多窗口**：9 个 HTML 入口（cost/chat/settings/writeHours/manualHours/
  todayPlan/batchWrite/index/mock），各自独立 Vue 挂载 → 主题 token 必须放共享
  CSS 并能被所有窗口读取（config store 各窗口都加载）。
- **配置持久化**：`stores/config.ts`，字段变化 250ms 防抖写盘，跨窗口通过
  `config-changed` 事件同步。新增 `styleTheme` 字段即可走同一套机制。

## Decisions（2026-06-04 与用户确认）

1. **耦合模型 = 纯视觉**（2026-06-06 修正，原"风格套装=皮肤+视觉联动"已废弃）：选"风格"只切视觉（配色/形态/背景/动效），**不动皮肤**；皮肤是完全独立的维度，由用户在 PetSection 单独选。原因：强制联动改皮肤会打扰用户既有选择，违背"交互体验优先"。
2. **风格深度 = 全沉浸**：配色 + 形态（圆角/阴影/字体）+ 背景纹理 + 微动效。
3. **风格组数 = 4+ 组**（候选见下方"风格集提案"，待用户敲定）。
4. **控件范围 = 全 app 统一替换**：建可主题化共享控件集，覆盖所有窗口的
   checkbox/按钮/输入/下拉等。

> 规模评估：全沉浸 × 4+ 套 × 9 窗口 + 全控件替换 = 大工程。必须里程碑递进，
> 不能一次性大爆炸（不可评审、回归风险高）。详见"Implementation Plan"。

## Confirmed（2026-06-04）

- **实施策略 = 分阶段 + 先看旗舰 demo**：M1 交付可点的旗舰主题+新控件给用户拍板，再铺开。
- **首发旗舰 & app 默认 = 科幻风**（当前深色 UI 的自然演进，迁移风险最低）。
- **4 套风格**（视觉维度；下表"推荐皮肤"仅为搭配建议，**不会自动应用**，皮肤由用户单独选）：
  | 风格 | 推荐皮肤 | 配色/形态基调 |
  |------|---------|--------------|
  | 科幻风（默认/旗舰） | 小机器人 robo | 冷青蓝霓虹×近黑、硬直角、细发光描边、等宽数字、网格背景 |
  | 俏皮风 | 月球钓鱼猫 cat-moon | 奶油/薄荷/薰衣草柔彩、大圆角、软阴影、圆润字体、弹跳动效 |
  | 治愈风 | 冥想树懒 sloth | 低饱和大地绿/米色、中圆角、柔和低对比、呼吸慢动效、纸纹理 |
  | 极简风 | 皮肤无关（默认 robo） | 灰阶+单强调色、发丝边框、零多余阴影/动效、系统字体 |

## Implementation Plan（里程碑）

- **M1 地基 + 旗舰 demo**（本次实现重点）：设计 token 体系 + `style-themes.ts` 注册表 +
  主题切换/持久化/跨窗口同步 + 共享控件集（先 ToggleSwitch）+ 科幻风一套，落地
  **成本页 + 设置页（含风格选择器）**。→ 交付给用户拍板。
- **M2 主题铺开**：补齐 俏皮 / 治愈 / 极简 三套（配色+形态+背景+动效资源）。
- **M3 全窗口铺开**：共享控件 + token 应用到剩余 7 个窗口，逐窗回归。
- **M4 沉浸打磨**：背景纹理 / 微动效细化。

> M2–M4 依赖 M1 拍板结果，届时各自开新任务，不在本任务一次性做完。

## M1 Scope（首个交付契约）

**新增配置**：`JarvisConfig.styleTheme: string`（默认 `'sci-fi'`），`load()` 里做缺省合并。
**仅写 styleTheme，不联动 petId**（纯视觉，见 Decisions 1 的 2026-06-06 修正）。沿用 250ms 防抖落盘 + `config-changed` 跨窗口同步。

**设计 token（CSS 变量，主题切换的唯一开关面）**：
- 颜色：`--bg / --surface / --surface-2 / --border / --text / --text-dim / --accent / --accent-2 / --danger / --success`
- 形态：`--radius-sm/md/lg`、`--radius-control`、`--shadow-1/2`、`--font-sans`、`--font-mono`
- 动效：`--motion-fast/base`、`--ease`
- 机制：`:root[data-theme="sci-fi"] { --… }`；`useTheme()` composable 读 `config.styleTheme`
  → 写 `document.documentElement.dataset.theme`，监听 `config-changed` 实时切换。每个窗口入口调用。

**共享控件**：`desktop/src/components/ui/ToggleSwitch.vue`（替换成本页含离职/含加班两个原生
checkbox）——键鼠可操作、focus ring 清晰、hover/checked 状态分明、读 token 主题化。

**落地页面**：成本页（控件栏/统计卡/表格改用 token + ToggleSwitch）、设置页（新增"风格"
选择器 section，镜像 PetSection.vue 的卡片选择 UI）。

**不在 M1**：俏皮/治愈/极简三套主题、其余 7 窗口、背景纹理/复杂动效、其它控件（按钮/输入/下拉）
的组件化（M1 先就地 tokenize，不强行抽组件）。

## Requirements（evolving）

- 引入设计 token（CSS 变量）作为主题基础，所有窗口共享
- 新增可切换的视觉风格，至少含用户点名的「科幻风（机器人）」「俏皮风（小猫）」
- 重设计成本页 checkbox 为交互良好的控件（胶囊/开关/分段等）
- 风格切换即时生效、跨窗口同步、持久化

## Acceptance Criteria（evolving）

- [ ] 在设置里能切换视觉风格，所有窗口实时生效
- [ ] 科幻风与俏皮风在配色/形态上**明显可区分**
- [ ] 成本页不再有原生 checkbox，新控件键鼠/hover/focus 状态清晰
- [ ] 切换风格后重启应用仍保持（持久化）

## Definition of Done

- 类型检查 / 构建通过（vite build）
- 至少在 cost + settings 两个窗口实测风格切换与新控件交互
- 设计 token 约定写入 `.trellis/spec/frontend/`（新建样式/主题规范）

## Out of Scope（tentative，待确认）

- 不做主题的在线下载/用户自定义配色（先内置几套）
- 不在本任务内替换全部窗口的所有控件（先建体系 + 重点页落地）

## Technical Notes

- Tailwind v4 原生支持 `@theme` + CSS 变量，可用 `:root[data-theme="sci-fi"]`
  覆盖 token，切换只改 `<html data-theme>`。
- 可镜像 `menu-themes.ts` 的"按 id 注册表"模式，新建 `style-themes.ts`。
- pet 与 style 是两个**完全独立**的维度（2026-06-06 决策：纯视觉，互不联动）。

## M1 实现结果（2026-06-04）

**已落地文件**：
- `desktop/src/style.css` — 设计 token + 四套主题（sci-fi/playful/zen/minimal）的 `[data-theme]` 覆盖
- `desktop/src/style-themes.ts` — STYLE_THEMES 注册表 + getStyleTheme
- `desktop/src/stores/config.ts` — 新增 `styleTheme` 字段（默认 sci-fi）+ 校验合并
- `desktop/src/composables/useTheme.ts` — 应用 + watch 实时切换
- `desktop/src/components/ui/ToggleSwitch.vue` — 无障碍胶囊开关
- `desktop/src/components/settings/StyleThemeSection.vue` + `settings-menu.ts` — 卡片式风格选择器（外观页）
- `desktop/src/CostApp.vue` — 接入 useTheme + 两个 checkbox → ToggleSwitch + 全量 tokenize
- `desktop/src/SettingsDetailApp.vue` — 接入 useTheme

**验证**：
- `vite build` 通过（8 窗口全量构建，无错误；仅 lottie/插件耗时等既有警告）。
- 跨窗口契约已核对：`config_save` → `config.rs:50 emit("config-changed")` → 各窗 store load() → styleTheme 变 → useTheme 重打 data-theme。成本页 + 设置页已接 useTheme。
- **未做**：交互式 GUI 实测（需用户跑 `npm run desktop:dev` 实际点选/观感拍板——这正是「先看 demo」环节）。

**M1 范围内已含 4 套 token**（switcher 才有的可切；sci-fi 精修，playful/zen/minimal 为第一版，
M2 再打磨到全沉浸：背景纹理/动效/精修配色）。settings 外壳本体未 tokenize（保持深色），M3 再铺。

## M1 收尾（2026-06-06，已拍板交付）

- 主题从 4 套扩到 **6 套**：新增 `matrix`（矩阵风）+ `cyber`（未来风）两套 canvas 主题（`MatrixRain.vue` / `CyberParticles.vue`），见 `84dca25`。
- 修复极简主题下悬浮面板透视：`UpdateWindow.vue` 等改用 `--popup-bg`；spec `theming.md` 补「透明主窗面板背景规则」+ canvas 主题组件契约（`3cd7ee0`）。
- 用户 GUI 实测确认 6 套主题正常、极简主题下面板可见 → **M1 拍板通过，交付完成**。
