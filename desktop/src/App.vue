<script setup lang="ts">
import { ref, computed, watch, onMounted, onUnmounted, nextTick } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { getCurrentWindow, LogicalPosition } from '@tauri-apps/api/window'

/** 获取当前窗口所在屏幕的逻辑像素全局边界（支持多屏幕） */
async function getMonitorBounds(): Promise<{ x: number; y: number; w: number; h: number }> {
  const win = getCurrentWindow()
  const mon = await win.currentMonitor()
  if (!mon) return { x: 0, y: 0, w: window.screen.width, h: window.screen.height }
  return {
    x: mon.position.x / mon.scaleFactor,
    y: mon.position.y / mon.scaleFactor,
    w: mon.size.width / mon.scaleFactor,
    h: mon.size.height / mon.scaleFactor,
  }
}
import { useAppStore } from './stores/app'
import { useConfigStore } from './stores/config'
import { useTaskAlerts } from './composables/useTaskAlerts'
import { useTaskCommits } from './composables/useTaskCommits'
import { useDailyReview } from './composables/useDailyReview'
import { useEveningReminder } from './composables/useEveningReminder'
import { ignoreTodayEffortClosing, useEffortClosingCheck } from './composables/useEffortClosingCheck'
import { useWorkdayNudges } from './composables/useWorkdayNudges'
import { useTimeGreetings } from './composables/useTimeGreetings'
import { useCursorPassthrough } from './composables/useCursorPassthrough'
import { useUpdater } from './composables/useUpdater'
import TaskWindow from './components/TaskWindow.vue'
import SettingsWindow from './components/SettingsWindow.vue'
import RiskWindow from './components/RiskWindow.vue'
import ReviewWindow from './components/ReviewWindow.vue'
import UpdateWindow from './components/UpdateWindow.vue'
import BindTaskWindow from './components/BindTaskWindow.vue'
import WelcomeWizard from './components/WelcomeWizard.vue'
import PetAvatar from './components/PetAvatar.vue'

const store = useAppStore()
const configStore = useConfigStore()
const { refresh: refreshAlerts } = useTaskAlerts()
const { fetchCommits } = useTaskCommits({ autoLoad: true })
const { openReview } = useDailyReview()
useEveningReminder({
  onTrigger: () => {
    // 不传 duration=0 — 那会让气泡永久挂着、状态卡在 thinking 直到用户手动 ×。
    // 复盘窗口已经自动打开了，气泡只是个提示动作，15s 足够注意到。
    showAlert(`${configStore.config.userTitle}，今天的复盘看一下？`, '📋', 'thinking', 15000)
    store.showReviewWindow = true
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

const updater = useUpdater({
  onAvailable: (version) => {
    // 新版本到位：直接弹更新窗口让用户看到本次更新内容（含 CHANGELOG 节选），
    // 同时挂常驻气泡作为残留提示——窗口被关掉之后用户还能看到提醒。
    showAlert(`新版本 v${version} 可用`, '✨', 'happy', 0)
    store.showUpdateWindow = true
  },
})
updater.start()

function openUpdateWindow() {
  closeAllPanels()
  store.showUpdateWindow = true
}

type JarvisState = 'idle' | 'thinking' | 'working' | 'warning' | 'happy' | 'morning' | 'coffee' | 'late'

const state = ref<JarvisState>('idle')
const showMenu = ref(false)
const alertText = ref('')
const alertEmoji = ref('')
const alertActions = ref<Array<{ label: string; action: () => void | Promise<void> }>>([])

// 小人在窗口的锚定角，根据拖拽后小人在屏幕上的位置自动选。
// rb=右下(默认) / rt=右上 / lb=左下 / lt=左上。
// 由 data-anchor 驱动 CSS：面板 inset 翻转、avatar-group 角位翻转，
// 始终让面板向"小人朝向屏幕中心的那一侧"展开，避免面板飞出屏幕。
type AvatarAnchor = 'rb' | 'rt' | 'lb' | 'lt'
const avatarAnchor = ref<AvatarAnchor>('rb')

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
  if (undockedWinPos) undockedWinPos = { ...undockedWinPos, x: newX, y: newY }
  await animateWindowToLogical(newX, newY, 220)
}

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

function menuOpenManualHours() {
  closeAllPanels()
  showMenu.value = false
  invoke('manual_hours_open').catch(e => console.error('manual_hours_open 失败:', e))
}

function menuCheckUpdate() {
  showMenu.value = false
  openUpdateWindow()
}

// --- 拖拽 + 点击 ---
// 历史教训：原本走 invoke('drag_window') → Rust 端 start_dragging 让 OS 接管拖拽。
// 但 OS 拖拽会保持"鼠标在窗口内的相对位置不变"，而小人在 400×560 透明窗口的
// 右下角（约 (320, 480) 偏移）。OS 限制鼠标不能离开屏幕 → 鼠标最高到屏幕 y=0
// 时小人最高也只能到 y=480 → 小人永远卡在屏幕下半部分。
//
// 改成手动 JS 拖拽：mousedown 记录窗口起点 + 鼠标起点，window 级 mousemove
// 用 setPosition 直接把窗口移到 (起点 + 鼠标位移)。这样窗口可以完全飞出屏幕
// 之外，小人能跟到屏幕任何位置。requestAnimationFrame 节流，60fps 内最多一次
// setPosition，避免 IPC 堆积。
//
// 拖拽结束后调 recomputeAnchor() 自动选择 4 个角之一，让面板向"小人朝向屏幕
// 中心的那一侧"展开，避免面板被屏幕边界裁掉。
const WINDOW_LOGICAL_W = 400
const WINDOW_LOGICAL_H = 560
const AVATAR_HALF = 36
const AVATAR_MARGIN = 10
const ANCHOR_AVATAR_CENTER: Record<AvatarAnchor, { x: number; y: number }> = {
  rb: { x: WINDOW_LOGICAL_W - AVATAR_MARGIN - AVATAR_HALF, y: WINDOW_LOGICAL_H - AVATAR_MARGIN - AVATAR_HALF },
  rt: { x: WINDOW_LOGICAL_W - AVATAR_MARGIN - AVATAR_HALF, y: AVATAR_MARGIN + AVATAR_HALF },
  lb: { x: AVATAR_MARGIN + AVATAR_HALF, y: WINDOW_LOGICAL_H - AVATAR_MARGIN - AVATAR_HALF },
  lt: { x: AVATAR_MARGIN + AVATAR_HALF, y: AVATAR_MARGIN + AVATAR_HALF },
}

let mouseDownTime = 0
let mouseDownX = 0
let mouseDownY = 0
let isDragging = false

let dragStartWinLogicalX = 0
let dragStartWinLogicalY = 0
let pendingDragX: number | null = null
let pendingDragY: number | null = null
let dragRafId: number | null = null

function flushDragPosition() {
  dragRafId = null
  if (pendingDragX === null || pendingDragY === null) return
  const x = pendingDragX
  const y = pendingDragY
  pendingDragX = null
  pendingDragY = null
  getCurrentWindow().setPosition(new LogicalPosition(x, y)).catch(() => {})
}

async function recomputeAnchor() {
  const win = getCurrentWindow()
  let winLogicalX: number
  let winLogicalY: number
  try {
    const pos = await win.outerPosition()
    const scale = await win.scaleFactor()
    winLogicalX = pos.x / scale
    winLogicalY = pos.y / scale
  } catch {
    return
  }
  const oldOffset = ANCHOR_AVATAR_CENTER[avatarAnchor.value]
  const avatarScreenX = winLogicalX + oldOffset.x
  const avatarScreenY = winLogicalY + oldOffset.y
  // 用当前屏幕的全局边界判断象限，支持多屏幕
  const mon = await getMonitorBounds()
  const relX = avatarScreenX - mon.x
  const relY = avatarScreenY - mon.y
  const horiz = relX >= mon.w / 2 ? 'r' : 'l'
  const vert = relY >= mon.h / 2 ? 'b' : 't'
  const newAnchor = (horiz + vert) as AvatarAnchor
  if (newAnchor === avatarAnchor.value) return

  // 切 anchor 时调窗口位置，让 avatar 视觉上保持在屏幕原位（CSS 改 anchor 角
  // 后，avatar 在窗口里的偏移变了，必须反向移窗口补偿才不会"小人突然跳"）。
  const newOffset = ANCHOR_AVATAR_CENTER[newAnchor]
  const newWinX = avatarScreenX - newOffset.x
  const newWinY = avatarScreenY - newOffset.y
  try {
    await win.setPosition(new LogicalPosition(Math.round(newWinX), Math.round(newWinY)))
  } catch {}
  avatarAnchor.value = newAnchor
}

// ===== Avatar dock (QQ 宠物贴边收纳) =====
// 行为：
//  - 拖到距屏幕某条边 < DOCK_AUTO_THRESHOLD 自动 dock（也可菜单手动）
//  - dock 后窗口缓动到只露 DOCK_SHOW_PX 在屏幕内，主体藏屏幕外
//  - hover dock 区域 或 showAlert 触发 → pokeOut 临时露出完整 avatar
//  - 鼠标离开 / 气泡消失后 DOCK_RECOIL_MS 后缓动回 dock 位置
//  - 用户在 dock 状态下手动拖小人 → 自动退出 dock（拖拽优先）
const DOCK_AUTO_THRESHOLD = 30
const DOCK_SHOW_PX = 18
const DOCK_RECOIL_MS = 5000
const DOCK_ANIM_MS = 200

type DockEdge = 'top' | 'right' | 'bottom' | 'left'

const dockEdge = ref<DockEdge | null>(null)
const isPoked = ref(false)
let dockUndockTimer: number | null = null
let dockAnimFrame: number | null = null
let dockedWinPos: { x: number; y: number; anchor: AvatarAnchor } | null = null
let undockedWinPos: { x: number; y: number; anchor: AvatarAnchor } | null = null

function cancelDockAnim() {
  if (dockAnimFrame !== null) {
    cancelAnimationFrame(dockAnimFrame)
    dockAnimFrame = null
  }
}

/** RAF 缓动窗口到目标 logical 位置。重入会先 cancel 上一帧。 */
async function animateWindowToLogical(targetX: number, targetY: number, durationMs: number): Promise<void> {
  cancelDockAnim()
  const win = getCurrentWindow()
  let fromX = 0, fromY = 0
  try {
    const pos = await win.outerPosition()
    const scale = await win.scaleFactor()
    fromX = pos.x / scale
    fromY = pos.y / scale
  } catch {
    return
  }
  await new Promise<void>((resolve) => {
    const startT = performance.now()
    function step(now: number) {
      const elapsed = now - startT
      const t = Math.min(1, elapsed / durationMs)
      const e = t * (2 - t)  // easeOutQuad
      const x = fromX + (targetX - fromX) * e
      const y = fromY + (targetY - fromY) * e
      win.setPosition(new LogicalPosition(Math.round(x), Math.round(y))).catch(() => {})
      if (t < 1) {
        dockAnimFrame = requestAnimationFrame(step)
      } else {
        dockAnimFrame = null
        resolve()
      }
    }
    dockAnimFrame = requestAnimationFrame(step)
  })
}

function computeDockTarget(
  edge: DockEdge,
  avatarScreenX: number, avatarScreenY: number,
  mon: { x: number; y: number; w: number; h: number },
): { winX: number; winY: number; newAnchor: AvatarAnchor } {
  // 选 dock 时的 anchor：avatar 必须在窗口靠屏幕一侧的角，否则窗口推出去
  // 小人就跟着到屏幕外了 —— 这是 dock 算法的核心约束。
  // 用当前屏幕的全局边界计算，支持多屏幕。
  let newAnchor: AvatarAnchor
  let targetCenterX = avatarScreenX
  let targetCenterY = avatarScreenY
  const relX = avatarScreenX - mon.x
  const relY = avatarScreenY - mon.y
  if (edge === 'right') {
    newAnchor = relY >= mon.h / 2 ? 'rb' : 'rt'
    targetCenterX = mon.x + mon.w - DOCK_SHOW_PX + AVATAR_HALF
  } else if (edge === 'left') {
    newAnchor = relY >= mon.h / 2 ? 'lb' : 'lt'
    targetCenterX = mon.x + DOCK_SHOW_PX - AVATAR_HALF
  } else if (edge === 'top') {
    newAnchor = relX >= mon.w / 2 ? 'rt' : 'lt'
    targetCenterY = mon.y + DOCK_SHOW_PX - AVATAR_HALF
  } else {
    newAnchor = relX >= mon.w / 2 ? 'rb' : 'lb'
    targetCenterY = mon.y + mon.h - DOCK_SHOW_PX + AVATAR_HALF
  }
  const offset = ANCHOR_AVATAR_CENTER[newAnchor]
  return { winX: targetCenterX - offset.x, winY: targetCenterY - offset.y, newAnchor }
}

async function currentAvatarScreenCenter(): Promise<{ x: number; y: number } | null> {
  const win = getCurrentWindow()
  try {
    const pos = await win.outerPosition()
    const scale = await win.scaleFactor()
    const off = ANCHOR_AVATAR_CENTER[avatarAnchor.value]
    return { x: pos.x / scale + off.x, y: pos.y / scale + off.y }
  } catch { return null }
}

/** 拖拽结束触发：avatar 离屏幕某边 < 阈值就 dock 到那边 */
async function maybeAutoDock() {
  if (dockEdge.value) return
  const c = await currentAvatarScreenCenter()
  if (!c) return
  const mon = await getMonitorBounds()
  const dTop = c.y - mon.y - AVATAR_HALF
  const dBottom = mon.y + mon.h - (c.y + AVATAR_HALF)
  const dLeft = c.x - mon.x - AVATAR_HALF
  const dRight = mon.x + mon.w - (c.x + AVATAR_HALF)
  const min = Math.min(dTop, dBottom, dLeft, dRight)
  if (min > DOCK_AUTO_THRESHOLD) return
  let edge: DockEdge
  if (min === dRight) edge = 'right'
  else if (min === dLeft) edge = 'left'
  else if (min === dBottom) edge = 'bottom'
  else edge = 'top'
  await dockTo(edge)
}

async function dockTo(edge: DockEdge) {
  const c = await currentAvatarScreenCenter()
  if (!c) return
  // 记下 dock 前的窗口位置，用于退出 dock 时回弹
  const win = getCurrentWindow()
  try {
    const pos = await win.outerPosition()
    const scale = await win.scaleFactor()
    undockedWinPos = { x: pos.x / scale, y: pos.y / scale, anchor: avatarAnchor.value }
  } catch { return }
  const mon = await getMonitorBounds()
  const t = computeDockTarget(edge, c.x, c.y, mon)
  // 切 anchor → CSS 立刻应用（avatar DOM 跳到新角）→ 缓动到 dock 位置
  // 中间几十毫秒小人位置略漂，但 200ms 缓动很快盖过去，体感是"嗖一下贴上去"
  avatarAnchor.value = t.newAnchor
  dockEdge.value = edge
  dockedWinPos = { x: t.winX, y: t.winY, anchor: t.newAnchor }
  await animateWindowToLogical(t.winX, t.winY, DOCK_ANIM_MS)
}

/**
 * 临时弹出露完整 avatar。
 * - recoil:true（默认） → 弹出后挂 5s 计时自动 retract，适合 showAlert/menu 等"一次性露脸"
 * - recoil:false → 不启动计时，由调用者（hover）自己控制何时回收，避免 hover 中突然缩回
 */
async function pokeOut(opts: { recoil?: boolean } = {}) {
  if (!dockEdge.value) return
  const wantRecoil = opts.recoil !== false
  if (dockUndockTimer) { clearTimeout(dockUndockTimer); dockUndockTimer = null }
  if (!isPoked.value) {
    isPoked.value = true
    if (undockedWinPos) {
      await animateWindowToLogical(undockedWinPos.x, undockedWinPos.y, DOCK_ANIM_MS)
    }
  }
  if (wantRecoil) {
    dockUndockTimer = window.setTimeout(retract, DOCK_RECOIL_MS)
  }
}

function onAvatarHover() {
  if (!dockEdge.value) return
  // hover 不带 recoil — 用户主动看小人，没必要倒计时缩回去
  pokeOut({ recoil: false })
}

function onAvatarLeave() {
  if (!dockEdge.value || !isPoked.value) return
  // 离开 hover 区 5s 后缩回，给用户一点缓冲（防止误触发滑过就缩回）
  if (dockUndockTimer) clearTimeout(dockUndockTimer)
  dockUndockTimer = window.setTimeout(retract, DOCK_RECOIL_MS)
}

async function retract() {
  dockUndockTimer = null
  if (!dockEdge.value || !isPoked.value) return
  isPoked.value = false
  if (dockedWinPos) {
    avatarAnchor.value = dockedWinPos.anchor
    await animateWindowToLogical(dockedWinPos.x, dockedWinPos.y, DOCK_ANIM_MS)
  }
}

/** 用户手动取消 dock（菜单项 / 拖拽 break out） */
async function exitDock() {
  if (!dockEdge.value) return
  dockEdge.value = null
  isPoked.value = false
  if (dockUndockTimer) { clearTimeout(dockUndockTimer); dockUndockTimer = null }
  if (undockedWinPos) {
    avatarAnchor.value = undockedWinPos.anchor
    await animateWindowToLogical(undockedWinPos.x, undockedWinPos.y, DOCK_ANIM_MS)
  }
  dockedWinPos = null
  undockedWinPos = null
}

async function menuToggleDock() {
  showMenu.value = false
  if (dockEdge.value) {
    await exitDock()
  } else {
    // 手动 dock：根据小人当前位置最近的边
    const c = await currentAvatarScreenCenter()
    if (!c) return
    const mon = await getMonitorBounds()
    const dRight = mon.x + mon.w - c.x
    const dLeft = c.x - mon.x
    const dBottom = mon.y + mon.h - c.y
    const dTop = c.y - mon.y
    const min = Math.min(dRight, dLeft, dBottom, dTop)
    let edge: DockEdge = 'right'
    if (min === dLeft) edge = 'left'
    else if (min === dBottom) edge = 'bottom'
    else if (min === dTop) edge = 'top'
    await dockTo(edge)
  }
}

async function onMouseDown(e: MouseEvent) {
  if (e.button !== 0) return
  isDragging = false
  mouseDownTime = Date.now()
  mouseDownX = e.screenX
  mouseDownY = e.screenY
  try {
    const win = getCurrentWindow()
    const pos = await win.outerPosition()
    const scale = await win.scaleFactor()
    // outerPosition 是 physical，转 logical 以匹配 e.screenX/Y（CSS px / logical）
    dragStartWinLogicalX = pos.x / scale
    dragStartWinLogicalY = pos.y / scale
  } catch {
    return
  }
  // 挂 window 级监听 —— 万一拖到边角鼠标短暂离开 .avatar 元素也不丢事件
  window.addEventListener('mousemove', onWindowMouseMove)
  window.addEventListener('mouseup', onWindowMouseUp)
}

function onWindowMouseMove(e: MouseEvent) {
  // 鼠标按键已释放但 mouseup 没派发（例如鼠标焦点被 OS 拿走）→ 主动清理
  if (!(e.buttons & 1)) {
    onWindowMouseUp(e)
    return
  }
  if (!isDragging) {
    const dx = Math.abs(e.screenX - mouseDownX)
    const dy = Math.abs(e.screenY - mouseDownY)
    if (dx <= 5 && dy <= 5) return
    isDragging = true
    // 真的开始拖拽了，dock 状态立刻让位——不缓动、不留 recoil timer，让窗口
    // 完全跟着手指走。否则 pokeOut 的动画会和拖拽 setPosition 抢窗口位置。
    if (dockEdge.value) {
      dockEdge.value = null
      isPoked.value = false
      if (dockUndockTimer) { clearTimeout(dockUndockTimer); dockUndockTimer = null }
      cancelDockAnim()
      dockedWinPos = null
      undockedWinPos = null
    }
  }
  const dxLogical = e.screenX - mouseDownX
  const dyLogical = e.screenY - mouseDownY
  pendingDragX = dragStartWinLogicalX + dxLogical
  pendingDragY = dragStartWinLogicalY + dyLogical
  if (dragRafId === null) {
    dragRafId = requestAnimationFrame(flushDragPosition)
  }
}

function onWindowMouseUp(e: MouseEvent) {
  window.removeEventListener('mousemove', onWindowMouseMove)
  window.removeEventListener('mouseup', onWindowMouseUp)
  if (dragRafId !== null) {
    cancelAnimationFrame(dragRafId)
    dragRafId = null
  }
  // 最后一帧的位置可能还在 pending，立刻 flush 保证终态准确
  flushDragPosition()

  const duration = Date.now() - mouseDownTime
  const dx = Math.abs(e.screenX - mouseDownX)
  const dy = Math.abs(e.screenY - mouseDownY)
  if (!isDragging && duration < 300 && dx < 5 && dy < 5) {
    // 左键点击小人：按 config.leftClickAction 分发。打开前先关掉其他所有面板/菜单。
    // 当前打开的就是目标面板时点一下应该收起 —— 保持原有 toggle 行为。
    handleAvatarLeftClick()
  }
  const wasDragging = isDragging
  isDragging = false
  if (wasDragging) {
    // 真正发生过拖拽再算 anchor，避免点击也触发窗口位置抖动；anchor 算完再
    // 试自动 dock —— 顺序很重要，maybeAutoDock 依赖 avatarAnchor 算 avatar 中心位置
    recomputeAnchor().then(() => maybeAutoDock())
  }
}

function handleAvatarLeftClick() {
  // dock 状态点击 → 退出 dock 再开面板。否则窗口大部分在屏幕外，面板也跟着藏，
  // 用户点完看不到反馈。exitDock 把窗口缓动回 undock 位置，面板随窗口一起入场。
  if (dockEdge.value) {
    exitDock()
  }
  const action = configStore.config.leftClickAction
  if (action === 'review') {
    // 复盘窗口由 useDailyReview 控制可见态（store.showReviewWindow）。
    // 已经显示则收起；否则关其它面板后打开
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
    state.value = 'warning'
  } else if (store.todayCount > 0) {
    showAlert(`${u}，今天有 ${store.todayCount} 个任务到期`, '⏰', 'warning', 10000)
    state.value = 'warning'
  } else if (store.stackedDays.length > 0) {
    const s = store.stackedDays[0]
    showAlert(`${u}，${s.date} 有 ${s.count} 个任务堆在一天，建议提前处理`, '⚠️', 'warning', 10000)
    state.value = 'warning'
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

onMounted(() => {
  configStore.load()
  store.refreshTaskBindings()

  // 窗口失焦时关闭所有面板和菜单：用户点击桌面或其他应用时自动收起
  getCurrentWindow().onFocusChanged(({ payload: focused }) => {
    if (!focused) closeAllPanels()
  })

  setTimeout(() => showAlert(`${configStore.config.userTitle}，${configStore.config.assistantName} 来啦`, '🤖', 'idle', 3000), 500)
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
  unlistenNewTasks?.()
  unlistenSettingsClosed?.()
})
</script>

<template>
  <div class="jarvis-container" :data-anchor="avatarAnchor" @contextmenu.prevent="toggleMenu">
    <!-- 菜单打开时铺满窗口的透明遮罩，点击任意位置关闭菜单 -->
    <div v-if="showMenu" class="menu-backdrop pointer-target" @click="showMenu = false" @contextmenu.prevent="showMenu = false" />
    <div v-if="showMenu" class="menu pointer-target">
      <button class="menu-item" @click="menuShowAlerts">
        <span>🔔</span><span>任务提醒</span>
        <span v-if="store.overdueCount > 0" class="menu-badge badge-danger">{{ store.overdueCount }}</span>
        <span v-else-if="store.todayCount > 0" class="menu-badge badge-warn">{{ store.todayCount }}</span>
        <span v-else-if="store.soonCount > 0" class="menu-badge badge-soon">{{ store.soonCount }}</span>
      </button>
      <button class="menu-item" @click="menuShowRisk"><span>⚠️</span><span>风险分析</span></button>
      <button class="menu-item" @click="menuShowReview"><span>📋</span><span>今日复盘</span></button>
      <button class="menu-item" @click="menuOpenManualHours"><span>✍️</span><span>写工时</span></button>
      <button class="menu-item" @click="menuOpenChat"><span>💬</span><span>聊天（大窗）</span></button>
      <button class="menu-item" @click="menuShowSettings"><span>⚙️</span><span>设置</span></button>
      <button class="menu-item" @click="menuToggleDock">
        <span>{{ dockEdge ? '📤' : '📥' }}</span>
        <span>{{ dockEdge ? '从边缘弹出' : '贴到屏幕边' }}</span>
      </button>
      <button class="menu-item" @click="menuCheckUpdate">
        <span>✨</span><span>检查更新</span>
        <span v-if="updater.available.value" class="menu-badge badge-soon">新</span>
      </button>
      <div class="menu-divider" />
      <button class="menu-item menu-item-danger" @click="menuQuit"><span>🚪</span><span>退出 {{ configStore.config.assistantName }}</span></button>
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
  color: rgba(255, 255, 255, 0.78);
  background: rgba(0, 0, 0, 0.45);
  border-radius: 8px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.status-label.active {
  color: rgba(16, 185, 129, 0.95);
  background: rgba(16, 185, 129, 0.15);
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
  background: linear-gradient(135deg, rgba(20, 30, 56, 0.96), rgba(15, 23, 42, 0.96));
  border: 1px solid rgba(100, 200, 255, 0.18);
  border-radius: 12px;
  box-shadow: 0 8px 24px rgba(0, 0, 0, 0.45);
  color: rgba(255, 255, 255, 0.92);
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
  color: rgba(255, 255, 255, 0.9);
  background: rgba(255, 255, 255, 0.08);
  border: 1px solid rgba(255, 255, 255, 0.12);
  border-radius: 7px;
  font: inherit;
  font-size: 11px;
  cursor: pointer;
}
.alert-bubble__action:hover {
  color: #fff;
  background: rgba(0, 212, 255, 0.16);
  border-color: rgba(0, 212, 255, 0.32);
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
  color: rgba(255, 255, 255, 0.4);
  background: transparent;
  border: none;
  border-radius: 4px;
  cursor: pointer;
}
.alert-bubble__close:hover {
  color: rgba(255, 255, 255, 0.85);
  background: rgba(255, 255, 255, 0.08);
}

/* 气泡下方小尾巴，指向 avatar */
.alert-bubble::after {
  content: '';
  position: absolute;
  right: 36px;                     /* 大致对准 avatar 中心 */
  bottom: -5px;
  width: 10px;
  height: 10px;
  background: rgba(15, 23, 42, 0.96);
  border-right: 1px solid rgba(100, 200, 255, 0.18);
  border-bottom: 1px solid rgba(100, 200, 255, 0.18);
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
  font-size: 14px; color: rgba(255, 255, 255, 0.3);
  background: rgba(255, 255, 255, 0.05); border-radius: 6px;
  cursor: pointer; line-height: 1;
}
.menu-btn:hover { color: rgba(255, 255, 255, 0.7); background: rgba(255, 255, 255, 0.1); }
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
  background: rgba(15, 23, 42, 0.97); backdrop-filter: blur(16px);
  border-radius: 10px; border: 1px solid rgba(255, 255, 255, 0.06);
  box-shadow: 0 8px 32px rgba(0, 0, 0, 0.5);
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
  font-size: 12px; color: rgba(255, 255, 255, 0.85);
  background: transparent; border: none; cursor: pointer;
  text-align: left;
}
.menu-item:hover { background: rgba(255, 255, 255, 0.08); }
.menu-item-danger { color: rgba(248, 113, 113, 0.95); }
.menu-item-danger:hover { background: rgba(239, 68, 68, 0.15); color: rgba(254, 202, 202, 1); }
.menu-divider {
  height: 1px;
  margin: 4px 8px;
  background: rgba(255, 255, 255, 0.08);
}
.menu-badge {
  margin-left: auto;
  font-size: 10px;
  padding: 1px 6px;
  border-radius: 8px;
}
.badge-danger { background: rgba(239, 68, 68, 0.8); color: white; }
.badge-warn { background: rgba(245, 158, 11, 0.8); color: white; }
.badge-soon { background: rgba(59, 130, 246, 0.8); color: white; }
</style>
