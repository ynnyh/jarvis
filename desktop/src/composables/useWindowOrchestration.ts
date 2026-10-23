import { ref, shallowRef, computed, watch, onMounted, onUnmounted, nextTick } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { useAppStore } from '../stores/app'
import { useConfigStore } from '../stores/config'
import { useTaskAlerts } from './useTaskAlerts'
import { useTaskCommits } from './useTaskCommits'
import { useDailyReview } from './useDailyReview'
import { useEveningReminder } from './useEveningReminder'
import { useTodayPlanPrompt } from './useTodayPlanPrompt'
import { ignoreTodayEffortClosing, useEffortClosingCheck } from './useEffortClosingCheck'
import { useWorkdayNudges } from './useWorkdayNudges'
import { usePluginHost } from './usePluginHost'
import { useCursorPassthrough } from './useCursorPassthrough'
import { useUpdater } from './useUpdater'
import { useScheduledReminders } from './useScheduledReminders'
import { useTheme } from './useTheme'
import { useAvatarDock, type AvatarAnchor } from './useAvatarDock'
import { useAvatarDrag } from './useAvatarDrag'
import { usePetRoamer, type PetAvatarApi } from './usePetRoamer'
import { loadCustomPets, loadPackPets } from '../petManifest'
import { MENU_KEYS, getMenuTheme } from '../menu-themes'
import { appEvent } from '../api/plugins'
import type { ChatStreamEvent } from '../api/chat'

export function useWindowOrchestration() {
  const store = useAppStore()
  const configStore = useConfigStore()
  useTheme()
  const { refresh: refreshAlerts } = useTaskAlerts()
  const { fetchCommits } = useTaskCommits({ autoLoad: true })
  const { openReview } = useDailyReview()

  // ===== Alert state (forward-declared before composable consumers that call showAlert) =====

  type JarvisState = 'idle' | 'thinking' | 'working' | 'warning' | 'happy' | 'morning' | 'coffee' | 'late'

  const state = ref<JarvisState>('idle')
  const showMenu = ref(false)

  /** 当前主题的菜单项，按 MENU_KEYS 顺序排列 */
  const menuItems = computed(() => {
    const theme = getMenuTheme(configStore.config.menuTheme)
    return MENU_KEYS.map(key => theme.items.find(i => i.key === key)!)
  })

  /** 日常类菜单项（前 3 个）和系统类菜单项（后 4 个） */
  const dailyMenuItems = computed(() => {
    const items = menuItems.value
    if (!configStore.config.costFeatureEnabled) {
      return items.filter(i => i.key !== 'cost')
    }
    return items
  })
  const systemMenuItems = computed(() => {
    // 系统组始终从 chat 开始（根据 MENU_KEYS 顺序，chat 是日常组之后第一个系统项）
    const items = menuItems.value
    const chatIdx = items.findIndex(i => i.key === 'chat')
    if (chatIdx === -1) return items.slice(3)
    return items.slice(chatIdx)
  })
  const costMenuItem = computed(() => menuItems.value.find(i => i.key === 'cost'))

  const alertText = ref('')
  const alertEmoji = ref('')
  const alertActions = ref<Array<{ label: string; action: () => void | Promise<void> }>>([])
  // 当前气泡是否为持久气泡（duration=0，带操作按钮、需用户响应）。
  // 摸头台词会临时顶掉气泡，结束后要凭这个判断是否恢复原气泡。
  const alertPersistent = ref(false)

  // 面板/气泡的开向锚点，根据拖拽后小人在屏幕上的位置自动选。
  // rb=右下(默认) / rt=右上 / lb=左下 / lt=左上。
  // 二期重构后小人恒居 900×900 窗口正中心，anchor 不再移动小人/窗口，只由
  // data-anchor 驱动 CSS：面板 inset、气泡堆叠方向、菜单开向整体翻转，
  // 始终朝"小人指向屏幕中心的那一侧"展开，避免面板飞出屏幕。
  const avatarAnchor = ref<AvatarAnchor>('rb')

  // PetAvatar 组件 ref：App.vue 模板绑定（ref="petAvatarEl"）。点播菜单（E1）与
  // 漫游引擎（E2）共用同一个入口 —— playClip/stopClip/setFacing 都走它，避免
  // 两处各自持有组件 ref 导致「一个在播、另一个不知道」的片段状态分裂。
  // shallowRef：PetAvatarApi.currentClip 本身是 Ref，deep ref 的 UnwrapRef 会把
  // 它解包成 string|null，与组件 expose 的真实类型对不上（类型层面假报错）。
  const petAvatarEl = shallowRef<PetAvatarApi | null>(null)

  // ===== Menu helpers (forward-declared for composable consumers) =====

  /** 关闭所有面板和菜单。打开任意 panel/menu 前调用，确保左右键互斥 */
  function closeAllPanels() {
    showMenu.value = false
    store.showTaskWindow = false
    store.showRiskWindow = false
    store.showReviewWindow = false
    store.showUpdateWindow = false
    store.showBindTaskWindow = false
    configStore.showSettingsWindow = false
    // 迷你对话面板一并收起：与大窗/其它面板互斥（chat:stream 双消费者防重）
    store.isPetChatOpen = false
  }

  function menuOpenTodayPlan() {
    closeAllPanels()
    showMenu.value = false
    invoke('today_plan_open').catch(e => console.error('today_plan_open 失败:', e))
  }

  function menuOpenManualHours() {
    closeAllPanels()
    showMenu.value = false
    invoke('manual_hours_open').catch(e => console.error('manual_hours_open 失败:', e))
  }

  // ===== Dock + Drag composables =====

  const {
    dockEdge,
    isPoked,
    undockedWinPos,
    setUndockedWinPos,
    animateWindowToLogical,
    getMonitorBounds,
    maybeAutoDock,
    pokeOut,
    onAvatarHover,
    onAvatarLeave,
    exitDock,
    breakoutFromDock,
  } = useAvatarDock({ avatarAnchor, closeAllPanels, showMenu })

  function handleAvatarLeftClick() {
    // dock 状态点击 → 退出 dock 再开面板。否则窗口大部分在屏幕外，面板也跟着藏，
    // 用户点完看不到反馈。exitDock 把窗口缓动回 undock 位置，面板随窗口一起入场。
    if (dockEdge.value) {
      exitDock()
    }
    const action = configStore.config.leftClickAction
    if (action === 'review') {
      if (store.showReviewWindow) {
        store.showReviewWindow = false
      } else {
        closeAllPanels()
        openReview('today')
      }
      return
    }
    // 默认（含未识别值）走任务列表
    if (store.showTaskWindow) {
      store.showTaskWindow = false
    } else {
      closeAllPanels()
      store.showTaskWindow = true
    }
  }

  // ===== 摸头（按住小人 ≥700ms 不动）=====
  // 纯本地反馈，不依赖 LLM：心心粒子 + 状态短暂 happy + 随机台词气泡。

  /** 摸头台词语料池：{assistantName}/{userTitle} 为模板变量 */
  const PAT_LINES = [
    '嘿嘿，被{userTitle}摸头了～',
    '{assistantName}最喜欢被摸头了！',
    '再摸的话…{assistantName}就要蹭你了哦',
    '呼噜呼噜…充电中，请继续',
    '{userTitle}的手暖暖的',
    '被摸头的{assistantName}战斗力 +10%',
    '唔…头发要被揉乱啦',
    '{userTitle}今天也辛苦了，摸摸',
    '咕噜咕噜…这是开心引擎的声音',
    '嘿嘿，被抓到害羞的一面了',
    '摸头协议已执行：心情值充满',
    '{assistantName}会一直陪着{userTitle}的',
    '再摸下去，{assistantName}要融化掉了～',
  ]

  function pickPatLine(): string {
    const line = PAT_LINES[Math.floor(Math.random() * PAT_LINES.length)]
    return line
      .replaceAll('{assistantName}', configStore.config.assistantName)
      .replaceAll('{userTitle}', configStore.config.userTitle)
  }

  // ===== 心心粒子 =====
  interface PetHeart {
    id: number
    emoji: string
    x: number       // 头顶随机水平偏移（px）
    size: number    // 字号（px）
    duration: number // 漂浮时长（ms）
    delay: number    // 出场延迟（ms）
  }
  const HEART_EMOJIS = ['♥', '💖']
  const hearts = ref<PetHeart[]>([])
  let heartSeq = 0
  // 批量清理用：spawn 的自删定时器，unmount 时统一撤掉
  const heartTimers: number[] = []

  function spawnHearts() {
    // 头顶水平散布随小人渲染盒宽按比例取（±25% 容器宽）：72px 时代 ±18px 逐值
    // 不变；任何尺寸下心心都从实际头顶宽度范围内飘起，不会挤在一条竖线上。
    const spread = configStore.config.petSizePx * 0.25
    const count = 3 + Math.floor(Math.random() * 3) // 3–5 个
    for (let i = 0; i < count; i++) {
      const delay = Math.round(Math.random() * 260)
      const duration = 1100 + Math.round(Math.random() * 300) // 单颗 ~1.1–1.4s，整体 ~1.4s
      const heart: PetHeart = {
        id: ++heartSeq,
        emoji: HEART_EMOJIS[Math.floor(Math.random() * HEART_EMOJIS.length)],
        x: Math.round(Math.random() * spread * 2 - spread),
        size: 11 + Math.round(Math.random() * 5),
        duration,
        delay,
      }
      hearts.value.push(heart)
      const timer = window.setTimeout(() => {
        hearts.value = hearts.value.filter(h => h.id !== heart.id)
      }, delay + duration + 60)
      heartTimers.push(timer)
    }
  }

  /** 持久气泡被摸头台词顶掉时的存根，台词结束后原样恢复（含状态色） */
  let patDisplaced: {
    text: string
    emoji: string
    actions: Array<{ label: string; action: () => void | Promise<void> }>
    s: JarvisState
  } | null = null
  let patHappyTimer: number | null = null

  // ===== agent 会话联动状态（chat:stream，定义在下方 applyChatStreamEvent）=====
  // 一轮 agent 会话是否进行中。ref：漫游引擎（E2）按它门控「不发起走路」，
  // 需要跨 composable 读取；语义与原布尔变量一致，只是可观察。
  const agentRunActive = ref(false)
  // 本轮最近的活动态（thinking/working）：摸头台词恢复气泡时若 agent 还在跑，
  // 状态色要归还给它，而不是恢复气泡的旧状态（A3：agent 事件覆盖状态色）
  let agentRunState: JarvisState = 'thinking'
  let agentHappyTimer: number | null = null

  function handleAvatarHold() {
    spawnHearts()
    // 摸头台词气泡顶掉当前气泡；若那是持久气泡（逾期提醒等必响应项），
    // 存下来等台词结束后原样恢复，不让一条台词把它永久吞掉。
    // 工具窗/迷你对话面板打开期间台词气泡会被 showAlert 抑制、气泡内容根本
    // 不会换——此时不存在「顶掉」，不做存根：否则 1.5s 后会把没被顶掉的气泡
    // 原样重弹一遍，若期间用户手动 × 掉还会复活僵尸气泡。
    if (!patDisplaced && !store.isAnyToolWindowOpen && !store.isPetChatOpen && alertPersistent.value && alertText.value) {
      patDisplaced = {
        text: alertText.value,
        emoji: alertEmoji.value,
        actions: alertActions.value,
        s: state.value,
      }
    }
    showAlert(pickPatLine(), '💗', 'happy', 8000)
    // 状态短暂切 happy：1.5s 后归位 idle（有持久气泡在场则随恢复一起还原状态）。
    // 归位时避开 agent 会话：agent 跑动期间状态色归 agent（thinking/working），
    // 恢复的持久气泡只还原文案与按钮、状态色不回写（A3：agent 事件覆盖状态色）。
    state.value = 'happy'
    if (patHappyTimer) clearTimeout(patHappyTimer)
    patHappyTimer = window.setTimeout(() => {
      patHappyTimer = null
      if (patDisplaced) {
        const d = patDisplaced
        patDisplaced = null
        showAlert(d.text, d.emoji, d.s, 0, d.actions)
        if (agentRunActive.value) state.value = agentRunState
      } else if (!agentRunActive.value) {
        state.value = 'idle'
      }
    }, 1500)
  }

  const {
    onMouseDown,
    isDragging,
  } = useAvatarDrag({
    avatarAnchor,
    dockEdge,
    onDragStart: breakoutFromDock,
    onDragEnd: maybeAutoDock,
    onClick: handleAvatarLeftClick,
    onHold: handleAvatarHold,
  })

  // ===== State map + UI helpers =====

  interface StateConfig {
    text: string
    emotion: string
    color: string
    animation: string
    description: string
  }

  const stateMap: Record<JarvisState, StateConfig> = {
    idle: {
      text: '待命中',
      emotion: '😌',
      color: '#00d4ff',
      animation: 'breathe',
      description: '随时为你服务',
    },
    thinking: {
      text: '正在分析',
      emotion: '🧠',
      color: '#3b82f6',
      animation: 'think',
      description: '让我想想...',
    },
    working: {
      text: '正在处理',
      emotion: '⚙️',
      color: '#10b981',
      animation: 'work',
      description: '全力以赴中',
    },
    warning: {
      text: '发现风险',
      emotion: '⚠️',
      color: '#f59e0b',
      animation: 'alert',
      description: '需要注意！',
    },
    happy: {
      text: '今天不错',
      emotion: '😊',
      color: '#ec4899',
      animation: 'happy',
      description: '任务完成很棒',
    },
    morning: {
      text: '早安',
      emotion: '🌅',
      color: '#fbbf24',
      animation: 'breathe',
      description: '新的一天开始了',
    },
    coffee: {
      text: '咖啡时间',
      emotion: '☕',
      color: '#a16207',
      animation: 'breathe',
      description: '喝一杯放松下',
    },
    late: {
      text: '该休息了',
      emotion: '🌙',
      color: '#a78bfa',
      animation: 'breathe',
      description: '别熬太晚了',
    },
  }
  const current = computed(() => stateMap[state.value])
  const hasAlert = computed(() => alertText.value !== '')

  // ===== E2 漫游行为引擎 =====
  // 依赖齐全后挂接（isDragging/hasAlert/dockEdge 都在上方）。引擎自带总闸
  // （config loaded + petRoaming + pack 宠物），非 pack 宠物 / 关开关时零行为。
  const { isMoving: petMoving, notifyInteraction: notifyPetInteraction } = usePetRoamer({
    petAvatar: petAvatarEl,
    isDragging,
    hasAlert,
    showMenu,
    dockEdge,
    isAgentActive: () => agentRunActive.value,
    // 大窗打开会藏小人（走动不可见）、设置窗打开用户正在设置界面操作（2026-10-22
    // 起小人不再被设置窗藏起，走动会干扰滑杆调尺寸的预览）、迷你对话面板贴着
    // 小人 —— 都不发起走路
    isPanelOpen: () => store.isAnyToolWindowOpen || store.isPetChatOpen,
  })

  // 状态切换时短暂高亮，给用户一个明显的视觉反馈（CSS 用 .flashing class 触发脉冲）
  const stateFlashing = ref(false)
  let flashTimer: number | null = null
  watch(state, () => {
    stateFlashing.value = true
    if (flashTimer) clearTimeout(flashTimer)
    flashTimer = window.setTimeout(() => { stateFlashing.value = false }, 800)
  })

  let alertTimer: number | null = null
  let greetingTimer: ReturnType<typeof setTimeout> | null = null

  /**
   * 气泡渲染后纠正窗口位置：测气泡 + 状态条的实际屏幕坐标，超出屏幕就把
   * 整窗口缓动滑回屏幕内。修三种截断：
   *  - dock pokeOut 后 undockedWinPos 本身就贴边，气泡仍在屏外
   *  - 非 dock 但小人在屏幕角落，data-anchor 翻完气泡还是出界
   *  - 气泡内容长换行高度变高，原本能放下的 stack 顶到屏幕外
   * 注意 dockEdge 状态下不矫正：dock 主动把窗口推出去，矫正会跟它打架。
   */
  async function ensureBubbleVisible() {
    if (dockEdge.value && !isPoked.value) return
    const bubble = document.querySelector<HTMLElement>('.alert-bubble')
    const label = document.querySelector<HTMLElement>('.status-label')
    if (!bubble && !label) return
    const targets = [bubble, label].filter(Boolean) as HTMLElement[]
    const win = getCurrentWindow()
    let winX: number, winY: number
    try {
      const pos = await win.outerPosition()
      const scale = await win.scaleFactor()
      winX = pos.x / scale
      winY = pos.y / scale
    } catch { return }
    const mon = await getMonitorBounds()
    const margin = 8
    let minLeft = Infinity, minTop = Infinity, maxRight = -Infinity, maxBottom = -Infinity
    for (const el of targets) {
      const r = el.getBoundingClientRect()
      if (r.width === 0) continue
      minLeft = Math.min(minLeft, winX + r.left)
      minTop = Math.min(minTop, winY + r.top)
      maxRight = Math.max(maxRight, winX + r.right)
      maxBottom = Math.max(maxBottom, winY + r.bottom)
    }
    if (!isFinite(minLeft)) return
    let dx = 0, dy = 0
    if (minLeft < mon.x + margin) dx = mon.x + margin - minLeft
    else if (maxRight > mon.x + mon.w - margin) dx = mon.x + mon.w - margin - maxRight
    if (minTop < mon.y + margin) dy = mon.y + margin - minTop
    else if (maxBottom > mon.y + mon.h - margin) dy = mon.y + mon.h - margin - maxBottom
    if (dx === 0 && dy === 0) return
    // 重要：矫正完得同步更新 undockedWinPos，否则下次 retract/exitDock 又跑回原位
    const newX = Math.round(winX + dx)
    const newY = Math.round(winY + dy)
    const curUndocked = undockedWinPos()
    if (curUndocked) setUndockedWinPos({ ...curUndocked, x: newX, y: newY })
    await animateWindowToLogical(newX, newY, 220)
  }

  // ===== Alert system =====

  function showAlert(
    text: string,
    emoji: string,
    s: JarvisState,
    duration = 5000,
    actions: Array<{ label: string; action: () => void | Promise<void> }> = [],
  ) {
    // 任意独立工具窗打开期间抑制临时气泡（duration>0）：大窗开着时小人被后端
    // 隐藏，弹了也看不见；settings 开着时小人虽仍可见（2026-10-22 起不藏），
    // 但用户正在设置界面操作，气泡徒增遮挡与打扰；chat 是例外——小人已改为与
    // 大窗共存，但对话内容就在大窗里，再叠一条气泡只会互相遮挡（agent 联动只
    // 切状态色即可）。迷你对话面板同理（B1-4）：面板就贴在小人身边，气泡会盖
    // 住对话流。持久气泡（duration=0，带操作按钮）是用户必须响应的，仍照常
    // 弹出——关窗后气泡仍可见，不会造成信息丢失。
    if ((store.isAnyToolWindowOpen || store.isPetChatOpen) && duration > 0) return
    state.value = s
    alertText.value = text
    alertEmoji.value = emoji
    alertActions.value = actions
    alertPersistent.value = duration === 0
    if (alertTimer) clearTimeout(alertTimer)
    if (duration > 0) {
      alertTimer = window.setTimeout(() => {
        alertText.value = ''
        alertEmoji.value = ''
        alertActions.value = []
        // 气泡消失时状态也归还 idle，避免小人卡在 thinking/warning 直到下次主动改 state
        state.value = 'idle'
      }, duration)
    }
    // 异步纠正：dock 中先 pokeOut（弹出来）→ 等 DOM 渲染 → 再算气泡是否出界
    ;(async () => {
      if (dockEdge.value) await pokeOut()
      await nextTick()
      await ensureBubbleVisible()
    })()
  }

  async function runAlertAction(action: () => void | Promise<void>) {
    await action()
    alertText.value = ''
    alertEmoji.value = ''
    alertActions.value = []
    state.value = 'idle'
  }

  function dismissAlert() {
    alertText.value = ''
    alertEmoji.value = ''
    alertActions.value = []
    state.value = 'idle'
  }

  function ignoreEffortClosingToday() {
    ignoreTodayEffortClosing()
  }

  // ===== B2 主动搭话：提醒节点让宠物「说人话」=====
  // 固定文案零延迟兜底（showAlert 先行），再异步调 pet_chatter 拿 LLM 一句话；
  // ≤5s 返回且气泡还停在本触发点的固定文案上才覆盖重弹（重置计时器，接受）。
  // LLM 未配置/失败/超时后端都返回 None → 静默放弃，固定文案已经在场。
  // 已知 kind：morning_plan / evening_summary / greeting_morning / greeting_coffee /
  // greeting_late / overdue；"custom" = 插件 llm_line 自带生成指令（context.instruction，
  // 方案 C′）。kind 收宽成 string：插件清单里是自由字段，运行期透传给后端定语气。

  function petChatter(
    kind: string,
    fallback: {
      text: string
      emoji: string
      state: JarvisState
      duration: number
      actions?: Array<{ label: string; action: () => void | Promise<void> }>
    },
    context: Record<string, unknown> = {},
  ) {
    showAlert(fallback.text, fallback.emoji, fallback.state, fallback.duration, fallback.actions)
    // 开关关闭 = 完全走固定文案，与现状一致，零网络请求
    if (configStore.config.petProactiveChat === false) return
    // 人设由前端附带（触发点手上就是最新配置，后端不用读配置快照）
    const fullContext = {
      ...context,
      assistantName: configStore.config.assistantName,
      userTitle: configStore.config.userTitle,
    }
    invoke<string | null>('pet_chatter', { kind, context: fullContext })
      .then((line) => {
        if (!line) return
        // stale 检查：气泡已被别的提醒/摸头台词顶掉 → 不再替换，防止张冠李戴
        if (alertText.value !== fallback.text || alertEmoji.value !== fallback.emoji) return
        showAlert(line, fallback.emoji, fallback.state, fallback.duration, fallback.actions)
      })
      .catch(e => console.error('pet_chatter 失败:', e))
  }

  // ===== Composable consumers (need showAlert defined above) =====

  useEveningReminder({
    onTrigger: () => {
      // 不传 duration=0 — 那会让气泡永久挂着、状态卡在 thinking 直到用户手动 ×。
      // 复盘窗口已经自动打开了，气泡只是个提示动作，15s 足够注意到。
      petChatter('evening_summary', {
        text: `${configStore.config.userTitle}，今天的复盘看一下？`,
        emoji: '📋', state: 'thinking', duration: 15000,
      })
      store.showReviewWindow = true
    },
  })
  useTodayPlanPrompt({
    onTrigger: () => {
      petChatter('morning_plan', {
        text: `${configStore.config.userTitle}，先定个今日计划？`,
        emoji: '📝', state: 'thinking', duration: 0,
        actions: [
          { label: '定今日计划', action: menuOpenTodayPlan },
          { label: '待会儿', action: () => {} },
        ],
      }, {
        // 尽力附带任务面数据（轮询未就绪时为 0，后端缺字段就只报时间）
        overdue: store.overdueCount,
        todayDue: store.todayCount,
      })
    },
  })
  useEffortClosingCheck({
    onReminder: (text, emoji) => {
      showAlert(text, emoji, 'warning', 0, [
        { label: '去写工时', action: menuOpenManualHours },
        { label: '今天忽略', action: ignoreEffortClosingToday },
      ])
    },
    onError: (text, emoji) => {
      showAlert(text, emoji, 'warning', 12000)
    },
  })
  useWorkdayNudges({
    onTrigger: (text, emoji) => {
      // 上班时段的小提示走 happy 表情、12s 自动消失，不打断工作
      showAlert(text, emoji, 'happy', 12000)
    },
  })
  // 插件引擎（方案 C′）：接管声明式插件的触发与执行。
  // 时段问候已迁为内置插件 builtin-greetings（原 useTimeGreetings 已删），llm_line
  // 走上面的 petChatter 链路，LLM 问候升级能力不回退。useWorkdayNudges 保持硬编码：
  // 其触发时间由用户作息配置动态推导（午休/下班锚点、间隔轮转、phase/静音门控），
  // 静态 plugin.json 表达不了，按「有条件逻辑不迁移」口径暂不迁移。
  usePluginHost({
    showAlert,
    petChatter,
    menu: {
      openTodayPlan: menuOpenTodayPlan,
      openManualHours: menuOpenManualHours,
      openReview: menuShowReview,
      openTasks: menuShowAlerts,
      dismiss: dismissAlert,
    },
  })
  // passthrough：让小人窗口的空白区域穿透到桌面，详见 composable 内部说明
  useCursorPassthrough()
  useScheduledReminders({
    onFire: (message) => {
      showAlert(message, '⏰', 'happy', 15000)
    },
  })

  const updater = useUpdater({
    onAvailable: (version) => {
      // 新版本到位：直接弹更新窗口让用户看到本次更新内容（含 CHANGELOG 节选），
      // 同时挂常驻气泡作为残留提示——窗口被关掉之后用户还能看到提醒。
      showAlert(`新版本 v${version} 可用`, '✨', 'happy', 0)
      store.showUpdateWindow = true
    },
  })
  updater.start()

  // ===== Menu actions =====

  function openUpdateWindow() {
    closeAllPanels()
    store.showUpdateWindow = true
  }

  function toggleMenu(e: Event) {
    e.stopPropagation()
    // 准备打开 menu 时，先关掉所有面板
    if (!showMenu.value) {
      closeAllPanels()
      // dock 状态下 menu 默认渲染在窗口右下，屏幕外用户看不到 → pokeOut 让窗口
      // 滑回屏幕内，菜单跟着出来
      if (dockEdge.value) pokeOut()
    }
    showMenu.value = !showMenu.value
  }

  async function menuShowAlerts() {
    closeAllPanels()
    store.showTaskWindow = true
  }

  function menuShowRisk() {
    closeAllPanels()
    store.showRiskWindow = true
  }

  function menuShowReview() {
    closeAllPanels()
    openReview('today')
  }

  function menuOpenCost() {
    closeAllPanels()
    showMenu.value = false
    invoke('cost_open').catch(e => console.error('cost_open 失败:', e))
  }

  async function menuQuit() {
    showMenu.value = false
    await invoke('quit_app')
  }

  function menuShowSettings() {
    closeAllPanels()
    configStore.showSettingsWindow = true
  }

  function menuOpenChat() {
    // 打开大窗对话：小人保持可见（不隐藏），聊天窗与小人共存。
    // closeAllPanels 已收起迷你对话面板（互斥：chat:stream 的消息流渲染
    // 前台只留大窗一个消费者），大窗打开期间迷你面板不再弹回。
    closeAllPanels()
    showMenu.value = false
    invoke('chat_open').catch(e => console.error('chat_open 失败:', e))
  }

  async function menuOpenPetChat() {
    // 打开贴身迷你对话（avatar 窗内 DOM 面板，不经后端窗口体系）。
    // 与聊天大窗互斥：先 chat_close 再开面板——保证 chat:stream
    // 前台只有一个消费者渲染消息流（avatar 自己的状态色联动不受影响）。
    // 不判 store.isChatOpen 直接关：该标记靠 chat-window-state 事件异步回填，
    // 「点开大窗后立刻点对话」时本地标记可能还没翻 true，按标记判断会漏关；
    // chat_close 幂等（窗不在就跳过 hide，事件照发 false），直发无害。
    closeAllPanels()
    showMenu.value = false
    try {
      await invoke('chat_close')
    } catch (e) {
      console.error('chat_close 失败:', e)
    }
    store.isPetChatOpen = true
  }

  function closePetChat() {
    store.isPetChatOpen = false
  }

  function menuCheckUpdate() {
    showMenu.value = false
    openUpdateWindow()
  }

  // --- 任务提醒联动 ---
  function showTaskAlertBubble() {
    // 直接读 store.X：Pinia setup store 中 computed 被自动 unwrap，destructure
    // 出来的是值而非 ref，再 .value 会得到 undefined，所有判断永远走 else 分支。
    const u = configStore.config.userTitle
    if (store.overdueCount > 0) {
      const maxDays = Math.max(...store.overdueTasks.map(t => -t.daysUntilDue))
      // 逾期最久的任务标题给 LLM 当话头（daysUntilDue 最小 = 逾期最久）
      const worst = store.overdueTasks.reduce((a, b) => (a.daysUntilDue <= b.daysUntilDue ? a : b))
      // 事件血管（方案 C′）：插件可订阅 task_overdue（payload 供 {payload.*} 插值）
      appEvent('task_overdue', { count: store.overdueCount, worstTitle: worst.title })
      petChatter('overdue', {
        text: `${u}，你有 ${store.overdueCount} 个任务已逾期，最久 ${maxDays} 天`,
        emoji: '🔥', state: 'warning', duration: 0,
      }, {
        overdue: store.overdueCount,
        overdueTitle: worst.title,
        overdueDays: maxDays,
      })
    } else if (store.todayCount > 0) {
      showAlert(`${u}，今天有 ${store.todayCount} 个任务到期`, '⏰', 'warning', 10000)
    } else if (store.stackedDays.length > 0) {
      const s = store.stackedDays[0]
      showAlert(`${u}，${s.date} 有 ${s.count} 个任务堆在一天，建议提前处理`, '⚠️', 'warning', 10000)
    } else if (store.soonCount > 0) {
      showAlert(`${u}，3 天内有 ${store.soonCount} 个任务到期`, '⏳', 'thinking', 8000)
    } else if (store.alertsLoaded) {
      showAlert(`${u}，7 天内无紧急任务 ✓`, '✅', 'happy', 5000)
    }
  }

  // 监听 alertLevel 变化，自动更新小人状态
  watch(() => store.alertLevel, (level) => {
    if (level === 'danger' || level === 'warning') {
      showTaskAlertBubble()
    }
    // safe 状态不主动打扰
  })

  let unlistenFocus: UnlistenFn | null = null
  onMounted(async () => {
    // await：确保 config.petId 在下方校验前已从磁盘加载完成，否则校验读到默认
    // 值（robo），会把合法的自定义 petId 误判为无效而回退。
    await configStore.load()
    store.refreshTaskBindings()
    await Promise.all([loadCustomPets(), loadPackPets()])

    // 加载完自定义宠物后，验证当前 petId 是否有效，无效则回退到默认
    const { getPetById } = await import('../petManifest')
    const currentPet = getPetById(configStore.config.petId)
    if (currentPet.id !== configStore.config.petId) {
      configStore.config.petId = 'robo'
    }

    // 窗口失焦时关闭大部分面板和菜单：用户点击桌面或其他应用时自动收起。
    // 但不关绑定窗（showBindTaskWindow），因为「浏览选择其它目录」会弹出
    // 原生目录选择器导致窗口短暂失焦，此时关窗会让用户无法确认绑定。
    getCurrentWindow().onFocusChanged(({ payload: focused }) => {
      if (!focused) {
        showMenu.value = false
        store.showTaskWindow = false
        store.showRiskWindow = false
        store.showReviewWindow = false
        store.showUpdateWindow = false
        configStore.showSettingsWindow = false
      }
    }).then(un => { unlistenFocus = un })

    greetingTimer = setTimeout(() => showAlert(`${configStore.config.userTitle}，${configStore.config.assistantName} 来啦`, '🤖', 'idle', 3000), 500)
    // 等数据加载后显示提醒
    watch(() => store.alertsLoaded, (loaded) => {
      if (loaded) showTaskAlertBubble()
    }, { once: true })
  })

  // 监听后端发现的新任务，推入绑定队列。
  // fetch_task_alerts 每次轮询都会做 snapshot diff，新出现的任务通过事件发上来；
  // 首次启动 snapshot 不存在时返回空 diff，老用户升级时不会被存量任务轰炸。
  let unlistenNewTasks: UnlistenFn | null = null
  let unlistenSettingsClosed: UnlistenFn | null = null
  let unlistenChatState: UnlistenFn | null = null
  let unlistenSettingsState: UnlistenFn | null = null
  let unlistenBigWindowState: UnlistenFn | null = null
  let unlistenChatStream: UnlistenFn | null = null

  // ===== Agent 实况联动（chat:stream → 小人状态色）=====
  // 后端 chat.rs 用 app.emit 全窗口广播 StreamEvent（camelCase），avatar 的
  // webview 即使被隐藏也照常收到事件（同 chat-window-state 先例）。这里只切
  // 状态色、不碰气泡系统：isChatOpen 期间临时气泡已被 isAnyToolWindowOpen
  // 抑制（对话内容在大窗里有），持久气泡的状态会被 agent 事件覆盖、文案不动。
  // StreamEvent 的 TS interface（ChatStreamEvent）统一放 api/chat.ts（B1-5），
  // 与聊天大窗/迷你面板共用一份。

  function applyChatStreamEvent(ev: ChatStreamEvent) {
    switch (ev.type) {
      case 'delta':
        // 仅空闲→活动跳变时切 thinking：首轮首个 delta 触发 flash，
        // 同轮后续 delta 同值赋值不重复触发
        if (!agentRunActive.value) {
          agentRunActive.value = true
          // 上一轮 done 的 happy 归位定时器若还挂着（连续两轮对话挨得近），
          // 先撤掉：否则 3s 到点会把本轮刚切的 thinking 打回 idle
          if (agentHappyTimer) { clearTimeout(agentHappyTimer); agentHappyTimer = null }
          agentRunState = 'thinking'
          state.value = 'thinking'
        }
        break
      case 'assistant':
        // assistant 收口且带工具调用 = 工具即将执行 → working
        if (ev.hasToolCalls) {
          agentRunState = 'working'
          state.value = 'working'
        }
        break
      case 'tool':
        // tool 事件在工具执行完之后发，仍处于工作段
        agentRunState = 'working'
        state.value = 'working'
        break
      case 'done': {
        agentRunActive.value = false
        if (agentHappyTimer) { clearTimeout(agentHappyTimer); agentHappyTimer = null }
        if (ev.truncated) {
          // 被截断不算圆满收场，直接归位
          state.value = 'idle'
        } else {
          state.value = 'happy'
          // done 归位 idle：持久气泡下次 show 时自会重设状态。
          // 摸头恢复流程进行中（patDisplaced 未消化）则不抢——恢复逻辑会按
          // 气泡原状态（或 agent 活动态）归位，见 handleAvatarHold。
          agentHappyTimer = window.setTimeout(() => {
            agentHappyTimer = null
            if (!patDisplaced) state.value = 'idle'
          }, 3000)
        }
        break
      }
    }
  }

  onMounted(async () => {
    unlistenNewTasks = await listen<Array<{ id: string; title: string; priority: string; deadline: string }>>(
      'new-tasks-detected',
      async (event) => {
        const tasks = event.payload || []
        for (const t of tasks) {
          store.enqueueBindTask(t)
        }
        // 队列非空 + 当前没显示绑定窗 → 拉起来（#84 实装窗口后生效）
        if (store.pendingBindTasks.length > 0 && !store.showBindTaskWindow) {
          // 宠物处于 dock 收纳状态时先恢复位置，否则弹窗按钮在屏外不可见
          if (dockEdge.value) exitDock()
          store.showBindTaskWindow = true
        }
      }
    )
    unlistenSettingsClosed = await listen('settings-detail-closed', () => {
      closeAllPanels()
      configStore.showSettingsWindow = true
    })
    // 后端独立窗口开关同步：chat/settings/大窗 open/close 时 emit 布尔状态，
    // 前端写入 store 供气泡抑制、状态点同步等联动使用。avatar 窗口隐藏期间
    // 这些事件不会丢失（事件在 hide 前后都 emit）。
    unlistenChatState = await listen<boolean>('chat-window-state', (e) => {
      store.isChatOpen = e.payload
    })
    unlistenSettingsState = await listen<boolean>('settings-window-state', (e) => {
      store.isSettingsOpen = e.payload
    })
    unlistenBigWindowState = await listen<boolean>('big-window-state', (e) => {
      store.isBigWindowOpen = e.payload
    })
    // agent 实况联动：小人状态色跟随聊天 agent 生命周期（详见上方说明）
    unlistenChatStream = await listen<ChatStreamEvent>('chat:stream', (e) => {
      applyChatStreamEvent(e.payload)
    })
  })

  // 首启引导：配置不完整（无禅道地址 OR 没添加代码文件夹）时展示
  const needsWizard = computed(() => {
    if (!configStore.loaded) return false
    const z = configStore.config.zentao
    const r = configStore.config.repoRoots
    return !z.baseUrl.trim() || !z.account.trim() || !r || r.length === 0
  })

  function onWizardDone() {
    // wizard 完成后立刻拉一次任务/提醒 —— 周期轮询要等 5/15 分钟，太慢。
    // wizard 内部已经等过 daemon restart，这里调时新 daemon 已经拿到新凭证。
    showAlert('配置完成，正在加载…', '✅', 'happy', 3000)
    refreshAlerts()
    fetchCommits()
  }

  onUnmounted(() => {
    if (alertTimer) clearTimeout(alertTimer)
    if (greetingTimer) clearTimeout(greetingTimer)
    if (patHappyTimer) clearTimeout(patHappyTimer)
    if (agentHappyTimer) clearTimeout(agentHappyTimer)
    for (const t of heartTimers) clearTimeout(t)
    unlistenNewTasks?.()
    unlistenSettingsClosed?.()
    unlistenChatState?.()
    unlistenSettingsState?.()
    unlistenBigWindowState?.()
    unlistenChatStream?.()
    unlistenFocus?.()
  })

  return {
    avatarAnchor, showMenu, state, current, hasAlert, stateFlashing,
    alertText, alertEmoji, alertActions,
    dailyMenuItems, systemMenuItems, costMenuItem,
    dockEdge, isPoked, updater, needsWizard,
    configStore, store, hearts, isDragging,
    // 漫游引擎（E2）：petAvatarEl 由 App.vue 模板绑定；isMoving 给 squish/fidget
    // 门控（模式照 isDragging）；notifyInteraction 点击小人时唤醒睡觉
    petAvatarEl, petMoving, notifyPetInteraction,
    toggleMenu, menuShowAlerts, menuShowRisk, menuShowReview, menuOpenTodayPlan,
    menuOpenCost, menuOpenChat, menuOpenPetChat, closePetChat,
    menuShowSettings, menuCheckUpdate, menuQuit,
    onAvatarHover, onAvatarLeave, onMouseDown,
    runAlertAction, dismissAlert, onWizardDone,
  }
}
