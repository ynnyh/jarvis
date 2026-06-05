<script setup lang="ts">
import { ref, computed, watch, onMounted, onUnmounted, nextTick } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'

import { useAppStore } from './stores/app'
import { useConfigStore } from './stores/config'
import { useTaskAlerts } from './composables/useTaskAlerts'
import { useTaskCommits } from './composables/useTaskCommits'
import { useDailyReview } from './composables/useDailyReview'
import { useEveningReminder } from './composables/useEveningReminder'
import { useTodayPlanPrompt } from './composables/useTodayPlanPrompt'
import { ignoreTodayEffortClosing, useEffortClosingCheck } from './composables/useEffortClosingCheck'
import { useWorkdayNudges } from './composables/useWorkdayNudges'
import { useTimeGreetings } from './composables/useTimeGreetings'
import { useCursorPassthrough } from './composables/useCursorPassthrough'
import { useUpdater } from './composables/useUpdater'
import { useScheduledReminders } from './composables/useScheduledReminders'
import { useTheme } from './composables/useTheme'
import { useAvatarDock, type AvatarAnchor } from './composables/useAvatarDock'
import { useAvatarDrag } from './composables/useAvatarDrag'
import TaskWindow from './components/TaskWindow.vue'
import SettingsWindow from './components/SettingsWindow.vue'
import RiskWindow from './components/RiskWindow.vue'
import ReviewWindow from './components/ReviewWindow.vue'
import UpdateWindow from './components/UpdateWindow.vue'
import BindTaskWindow from './components/BindTaskWindow.vue'
import WelcomeWizard from './components/WelcomeWizard.vue'
import PetAvatar from './components/PetAvatar.vue'
import ErrorBoundary from './components/ErrorBoundary.vue'
import { MENU_KEYS, getMenuTheme } from './menu-themes'

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
  let winX = 0, winY = 0
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
  // 打开大窗对话：avatar 自动隐藏，聊天窗口接管
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
onMounted(() => {
  configStore.load()
  store.refreshTaskBindings()

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
    (event) => {
      const tasks = event.payload || []
      for (const t of tasks) {
        store.enqueueBindTask(t)
      }
      // 队列非空 + 当前没显示绑定窗 → 拉起来（#84 实装窗口后生效）
      if (store.pendingBindTasks.length > 0 && !store.showBindTaskWindow) {
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
</script>

<template>
  <ErrorBoundary>
  <div class="jarvis-container" :data-anchor="avatarAnchor" @contextmenu.prevent="toggleMenu">
    <!-- 菜单打开时铺满窗口的透明遮罩，点击任意位置关闭菜单 -->
    <div v-if="showMenu" class="menu-backdrop pointer-target" @click="showMenu = false" @contextmenu.prevent="showMenu = false" />
    <div v-if="showMenu" class="menu pointer-target">
      <!-- 日常组 -->
      <button class="menu-item" @click="menuShowAlerts">
        <span>{{ dailyMenuItems[0].emoji }}</span><span>{{ dailyMenuItems[0].label }}</span>
        <span v-if="store.overdueCount > 0" class="menu-badge badge-danger">{{ store.overdueCount }}</span>
        <span v-else-if="store.todayCount > 0" class="menu-badge badge-warn">{{ store.todayCount }}</span>
        <span v-else-if="store.soonCount > 0" class="menu-badge badge-soon">{{ store.soonCount }}</span>
      </button>
      <button class="menu-item" @click="menuShowReview">
        <span>{{ dailyMenuItems[1].emoji }}</span><span>{{ dailyMenuItems[1].label }}</span>
      </button>
      <button class="menu-item" @click="menuOpenTodayPlan">
        <span>{{ dailyMenuItems[2].emoji }}</span><span>{{ dailyMenuItems[2].label }}</span>
      </button>
      <button v-if="configStore.config.costFeatureEnabled && costMenuItem" class="menu-item" @click="menuOpenCost">
        <span>{{ costMenuItem.emoji }}</span><span>{{ costMenuItem.label }}</span>
      </button>

      <div class="menu-divider" />

      <!-- 系统组 -->
      <button class="menu-item" @click="menuOpenChat">
        <span>{{ systemMenuItems[0].emoji }}</span><span>{{ systemMenuItems[0].label }}</span>
      </button>
      <button class="menu-item" @click="menuShowSettings">
        <span>{{ systemMenuItems[1].emoji }}</span><span>{{ systemMenuItems[1].label }}</span>
      </button>
      <button class="menu-item" @click="menuCheckUpdate">
        <span>{{ systemMenuItems[2].emoji }}</span><span>{{ systemMenuItems[2].label }}</span>
        <span v-if="updater.available.value" class="menu-badge badge-soon">新</span>
      </button>
      <button class="menu-item menu-item-danger" @click="menuQuit">
        <span>{{ systemMenuItems[3].emoji }}</span><span>{{ systemMenuItems[3].label }}</span>
      </button>
    </div>

    <div class="menu-btn pointer-target" @click="toggleMenu">⋯</div>

    <!--
      avatar-group 只是 flex 排版容器，跨越 alert 气泡到 avatar 的整个矩形（含
      间隙、外边距）。如果在这一层加 pointer-target，整个 200×200 范围都不
      穿透，鼠标在空白处也被吃掉。把标记下沉到真正有像素的子元素上。
      拖拽/点击事件也只挂在 avatar 上 —— 状态条和气泡不应该触发拖窗。

      hover 处理挂在 group 上而非 .avatar：dock 状态下用户从 avatar 移到 status-label
      或 alert-bubble 都属于"仍在 hover 范围内"，挂 group 上 sibling 切换不会触发
      mouseleave（mouseenter/leave 不冒泡但会在新进入的祖先链上触发）。
    -->
    <div class="avatar-group" @mouseenter="onAvatarHover" @mouseleave="onAvatarLeave">
      <!-- 弹出气泡（位于状态条上方，绑定到右边对齐）。dock 收纳态下隐藏——
           气泡如果带到屏幕外用户也看不到；showAlert 会触发 pokeOut 弹出来再显示 -->
      <transition name="bubble">
        <div v-if="hasAlert && (!dockEdge || isPoked)" class="alert-bubble pointer-target">
          <span class="alert-bubble__emoji">{{ alertEmoji }}</span>
          <span class="alert-bubble__body">
            <span class="alert-bubble__text">{{ alertText }}</span>
            <span v-if="alertActions.length" class="alert-bubble__actions">
              <button
                v-for="action in alertActions"
                :key="action.label"
                class="alert-bubble__action"
                @click.stop="runAlertAction(action.action)"
              >
                {{ action.label }}
              </button>
            </span>
          </span>
          <button class="alert-bubble__close" @click.stop="dismissAlert" aria-label="关闭">×</button>
        </div>
      </transition>

      <div v-show="false" class="status-label" />

      <!-- 状态条已删：状态用宠物外圈颜色 + 气泡传达，「待命中」常驻条只是视觉噪音，
           dock 时还会被屏幕边切掉。如果以后想加回来，把这一行删了把原 div 还原即可。 -->

      <div class="avatar pointer-target" :class="{ docked: dockEdge && !isPoked }"
        @mousedown="onMouseDown"
      >
        <PetAvatar
          :pet-id="configStore.config.petId"
          :color="current.color"
          :glow-color="current.glowColor"
          :active="state === 'working'"
          :flashing="stateFlashing"
        />
      </div>
    </div>

    <!-- 任务提醒窗口 -->
    <TaskWindow />
    <!-- 风险分析窗口 -->
    <RiskWindow />
    <!-- 今日复盘窗口 -->
    <ReviewWindow />
    <!-- 设置小屏菜单 -->
    <SettingsWindow />
    <!-- 更新窗口 -->
    <UpdateWindow :updater="updater" />
    <!-- 任务↔项目绑定窗（新任务事件 / 任务卡未绑定图标都会拉起） -->
    <BindTaskWindow />
    <!-- 首启引导：配置不完整时全屏覆盖，写完后消失 -->
    <WelcomeWizard v-if="needsWizard" @done="onWizardDone" />
  </div>
  </ErrorBoundary>
</template>

<style scoped>
.jarvis-container {
  width: 100%;
  height: 100%;
  position: relative;
  -webkit-user-select: none;
  user-select: none;
  overflow: visible;
  background: transparent;
  /* 默认 anchor=rb 的 CSS variable，data-anchor 切换时被同名规则覆盖。
     --avatar-* 控制 .avatar-group 在窗口的 4 个角；--panel-* 控制各面板的
     inset 翻转，让面板始终在小人对侧 → 远离屏幕边界。 */
  --avatar-top: auto;
  --avatar-right: 10px;
  --avatar-bottom: 10px;
  --avatar-left: auto;
  --panel-top: 8px;
  --panel-right: 8px;
  --panel-bottom: 90px;
  --panel-left: 8px;
}
.jarvis-container[data-anchor="rt"] {
  --avatar-top: 10px;
  --avatar-right: 10px;
  --avatar-bottom: auto;
  --avatar-left: auto;
  --panel-top: 90px;
  --panel-bottom: 8px;
}
.jarvis-container[data-anchor="lb"] {
  --avatar-top: auto;
  --avatar-right: auto;
  --avatar-bottom: 10px;
  --avatar-left: 10px;
}
.jarvis-container[data-anchor="lt"] {
  --avatar-top: 10px;
  --avatar-right: auto;
  --avatar-bottom: auto;
  --avatar-left: 10px;
  --panel-top: 90px;
  --panel-bottom: 8px;
}

.avatar-group {
  position: absolute;
  top: var(--avatar-top);
  right: var(--avatar-right);
  bottom: var(--avatar-bottom);
  left: var(--avatar-left);
  display: flex;
  /* anchor 在窗口顶时反转子元素顺序：avatar 放最上、状态条/气泡在下方
     堆叠，避免 avatar 远离 group 锚点导致整体跑到屏幕外。 */
  flex-direction: column;
  align-items: flex-end;   /* 子元素全部贴右边对齐 */
  gap: 6px;
  touch-action: none;
}
.jarvis-container[data-anchor="rt"] .avatar-group,
.jarvis-container[data-anchor="lt"] .avatar-group {
  flex-direction: column-reverse;
}
.jarvis-container[data-anchor="lb"] .avatar-group,
.jarvis-container[data-anchor="lt"] .avatar-group {
  align-items: flex-start; /* 左侧 anchor 时，子元素改贴左对齐 */
}

/* ===== 状态条（始终显示） ===== */
.status-label {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  max-width: 180px;
  padding: 3px 10px;
  font-size: 11px;
  color: var(--text-ghost);
  background: rgba(0, 0, 0, 0.45);
  border-radius: var(--radius-sm);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.status-label.active {
  color: var(--green-text);
  background: var(--green-bg);
}
.status-label__emoji { font-size: 12px; flex-shrink: 0; }
.status-label__text { overflow: hidden; text-overflow: ellipsis; }

/* ===== 提示气泡（向左展开） ===== */
.alert-bubble {
  position: relative;
  display: flex;
  align-items: flex-start;
  gap: 8px;
  min-width: 160px;
  max-width: 320px;
  padding: 8px 28px 8px 12px;     /* 右侧留出关闭按钮空间 */
  background: var(--popup-bg);
  backdrop-filter: blur(20px);
  -webkit-backdrop-filter: blur(20px);
  border: var(--panel-border);
  border-radius: var(--radius-lg);
  box-shadow: var(--panel-shadow);
  color: var(--text);
  font-size: 12px;
  line-height: 1.55;
  /* 关键：换行策略 */
  white-space: normal;
  word-break: normal;
  overflow-wrap: anywhere;        /* 兜底长串可断 */
  cursor: default;
}

.alert-bubble__emoji {
  font-size: 16px;
  line-height: 1.4;
  flex-shrink: 0;
}
.alert-bubble__text {
  flex: 1;
  min-width: 0;                    /* 让 flex 子项允许收缩 */
}
.alert-bubble__body {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.alert-bubble__actions {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}
.alert-bubble__action {
  height: 24px;
  padding: 0 9px;
  color: var(--text-ghost);
  background: var(--surface-item-hover);
  border: 1px solid var(--border);
  border-radius: var(--radius-control);
  font: inherit;
  font-size: 11px;
  cursor: pointer;
}
.alert-bubble__action:hover {
  color: var(--text);
  background: var(--accent-glow);
  border-color: var(--accent-border);
}
.alert-bubble__close {
  position: absolute;
  top: 2px;
  right: 6px;
  width: 18px;
  height: 18px;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 14px;
  line-height: 1;
  color: var(--text-muted);
  background: transparent;
  border: none;
  border-radius: var(--radius-control);
  cursor: pointer;
}
.alert-bubble__close:hover {
  color: var(--text-ghost);
  background: var(--surface-item-hover);
}

/* 气泡下方小尾巴，指向 avatar */
.alert-bubble::after {
  content: '';
  position: absolute;
  right: 36px;                     /* 大致对准 avatar 中心 */
  bottom: -5px;
  width: 10px;
  height: 10px;
  background: var(--popup-bg);
  border-right: var(--panel-border);
  border-bottom: var(--panel-border);
  transform: rotate(45deg);
}

/* 进出动效 */
.bubble-enter-active,
.bubble-leave-active {
  transition: opacity 0.22s ease, transform 0.22s ease;
}
.bubble-enter-from,
.bubble-leave-to {
  opacity: 0;
  transform: translateY(4px);
}

.avatar {
  position: relative;
  width: 72px;
  height: 72px;
  cursor: pointer;
}
/* 内容（包括发光、Lottie 动画、状态点、hover 放大、flashing 脉冲）全在 PetAvatar.vue 里。
   .avatar 只做 72×72 事件钩子，事件挂在它上面（mousedown）。 */

.menu-btn {
  position: fixed; bottom: 86px; right: 16px;
  width: 22px; height: 22px;
  display: flex; align-items: center; justify-content: center;
  font-size: 14px; color: var(--text-faint);
  background: var(--surface); border-radius: var(--radius-control);
  cursor: pointer; line-height: 1;
}
.menu-btn:hover { color: var(--text-ghost); background: var(--surface-item-active); }
/* menu-btn 跟随 anchor 翻转：用 CSS 变量跟 avatar-group 同步 */
.jarvis-container[data-anchor="rt"] .menu-btn { top: 86px; bottom: auto; }
.jarvis-container[data-anchor="lb"] .menu-btn { left: 16px; right: auto; }
.jarvis-container[data-anchor="lt"] .menu-btn { top: 86px; left: 16px; bottom: auto; right: auto; }

.menu-backdrop {
  position: fixed;
  inset: 0;
  z-index: 90;
}
.menu {
  position: fixed; bottom: 16px; right: 90px;
  background: var(--popup-bg); backdrop-filter: none;
  border-radius: var(--radius-md); border: var(--menu-border);
  box-shadow: var(--menu-shadow);
  padding: 4px 0; z-index: 100; min-width: 130px;
  overflow: hidden;
}
/* 菜单跟随 anchor 翻转：始终在 menu-btn 左侧 */
.jarvis-container[data-anchor="rt"] .menu { top: 16px; bottom: auto; }
.jarvis-container[data-anchor="lb"] .menu { left: 90px; right: auto; }
.jarvis-container[data-anchor="lt"] .menu { top: 16px; left: 90px; bottom: auto; right: auto; }
.menu-item {
  width: 100%; padding: 8px 14px;
  display: flex; align-items: center; gap: 8px;
  font-size: 12px; color: var(--text);
  background: transparent; border: none; cursor: pointer;
  text-align: left;
}
.menu-item:hover { background: var(--surface-item-hover); }
.menu-item-danger { color: var(--red-text); }
.menu-item-danger:hover { background: var(--red-bg); color: var(--red-text-light); }
.menu-divider {
  height: 1px;
  margin: 4px 8px;
  background: var(--border-soft);
}
.menu-badge {
  margin-left: auto;
  font-size: 10px;
  padding: 1px 6px;
  border-radius: var(--radius-sm);
  font-family: var(--font-display);
  font-variant-numeric: var(--num-font-variant);
}
.badge-danger { background: color-mix(in srgb, var(--red) 80%, transparent); color: var(--badge-text); }
.badge-warn { background: color-mix(in srgb, var(--yellow) 80%, transparent); color: var(--badge-text); }
.badge-soon { background: color-mix(in srgb, var(--blue) 80%, transparent); color: var(--badge-text); }
</style>
