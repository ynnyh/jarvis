import { ref, computed, watch, onMounted, onUnmounted, nextTick } from 'vue'
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
import { useTimeGreetings } from './useTimeGreetings'
import { useCursorPassthrough } from './useCursorPassthrough'
import { useUpdater } from './useUpdater'
import { useScheduledReminders } from './useScheduledReminders'
import { useTheme } from './useTheme'
import { useAvatarDock, type AvatarAnchor } from './useAvatarDock'
import { useAvatarDrag } from './useAvatarDrag'
import { loadCustomPets } from '../petManifest'
import { MENU_KEYS, getMenuTheme } from '../menu-themes'

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

  // 小人在窗口的锚定角，根据拖拽后小人在屏幕上的位置自动选。
  // rb=右下(默认) / rt=右上 / lb=左下 / lt=左上。
  // 由 data-anchor 驱动 CSS：面板 inset 翻转、avatar-group 角位翻转，
  // 始终让面板向"小人朝向屏幕中心的那一侧"展开，避免面板飞出屏幕。
  const avatarAnchor = ref<AvatarAnchor>('rb')

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
    menuToggleDock,
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

  const {
    onMouseDown,
  } = useAvatarDrag({
    avatarAnchor,
    dockEdge,
    onDragStart: breakoutFromDock,
    onDragEnd: maybeAutoDock,
    onClick: handleAvatarLeftClick,
  })

  // ===== State map + UI helpers =====

  interface StateConfig {
    text: string
    emotion: string
    color: string
    glowColor: string
    animation: string
    description: string
  }

  const stateMap: Record<JarvisState, StateConfig> = {
    idle: {
      text: '待命中',
      emotion: '😌',
      color: '#00d4ff',
      glowColor: 'rgba(0, 212, 255, 0.3)',
      animation: 'breathe',
      description: '随时为你服务',
    },
    thinking: {
      text: '正在分析',
      emotion: '🧠',
      color: '#3b82f6',
      glowColor: 'rgba(59, 130, 246, 0.3)',
      animation: 'think',
      description: '让我想想...',
    },
    working: {
      text: '正在处理',
      emotion: '⚙️',
      color: '#10b981',
      glowColor: 'rgba(16, 185, 129, 0.3)',
      animation: 'work',
      description: '全力以赴中',
    },
    warning: {
      text: '发现风险',
      emotion: '⚠️',
      color: '#f59e0b',
      glowColor: 'rgba(245, 158, 11, 0.4)',
      animation: 'alert',
      description: '需要注意！',
    },
    happy: {
      text: '今天不错',
      emotion: '😊',
      color: '#ec4899',
      glowColor: 'rgba(236, 72, 153, 0.3)',
      animation: 'happy',
      description: '任务完成很棒',
    },
    morning: {
      text: '早安',
      emotion: '🌅',
      color: '#fbbf24',
      glowColor: 'rgba(251, 191, 36, 0.4)',
      animation: 'breathe',
      description: '新的一天开始了',
    },
    coffee: {
      text: '咖啡时间',
      emotion: '☕',
      color: '#a16207',
      glowColor: 'rgba(161, 98, 7, 0.35)',
      animation: 'breathe',
      description: '喝一杯放松下',
    },
    late: {
      text: '该休息了',
      emotion: '🌙',
      color: '#a78bfa',
      glowColor: 'rgba(167, 139, 250, 0.35)',
      animation: 'breathe',
      description: '别熬太晚了',
    },
  }
  const current = computed(() => stateMap[state.value])
  const hasAlert = computed(() => alertText.value !== '')

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
    state.value = s
    alertText.value = text
    alertEmoji.value = emoji
    alertActions.value = actions
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

  // ===== Composable consumers (need showAlert defined above) =====

  useEveningReminder({
    onTrigger: () => {
      // 不传 duration=0 — 那会让气泡永久挂着、状态卡在 thinking 直到用户手动 ×。
      // 复盘窗口已经自动打开了，气泡只是个提示动作，15s 足够注意到。
      showAlert(`${configStore.config.userTitle}，今天的复盘看一下？`, '📋', 'thinking', 15000)
      store.showReviewWindow = true
    },
  })
  useTodayPlanPrompt({
    onTrigger: () => {
      showAlert(`${configStore.config.userTitle}，先定个今日计划？`, '📝', 'thinking', 0, [
        { label: '定今日计划', action: menuOpenTodayPlan },
        { label: '待会儿', action: () => {} },
      ])
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
  useTimeGreetings({
    onTrigger: (text, emoji, s) => {
      // 早晨/咖啡/夜晚问候：15s，状态切到对应色调
      showAlert(text, emoji, s, 15000)
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
    // 打开大窗对话：小人保持可见（不隐藏），聊天窗与小人共存
    closeAllPanels()
    showMenu.value = false
    invoke('chat_open').catch(e => console.error('chat_open 失败:', e))
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
      showAlert(
        `${u}，你有 ${store.overdueCount} 个任务已逾期，最久 ${maxDays} 天`,
        '🔥', 'warning', 0,
      )
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
    await loadCustomPets()

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
    unlistenNewTasks?.()
    unlistenSettingsClosed?.()
    unlistenFocus?.()
  })

  return {
    avatarAnchor, showMenu, state, current, hasAlert, stateFlashing,
    alertText, alertEmoji, alertActions,
    dailyMenuItems, systemMenuItems, costMenuItem,
    dockEdge, isPoked, updater, needsWizard,
    configStore, store,
    toggleMenu, menuShowAlerts, menuShowRisk, menuShowReview, menuOpenTodayPlan,
    menuOpenCost, menuOpenChat, menuShowSettings, menuCheckUpdate, menuQuit,
    onAvatarHover, onAvatarLeave, onMouseDown,
    runAlertAction, dismissAlert, onWizardDone,
  }
}
