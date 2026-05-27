<script setup lang="ts">
import { ref, computed, watch, onMounted, onUnmounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { getCurrentWindow, LogicalPosition } from '@tauri-apps/api/window'
import { useAppStore } from './stores/app'
import { useConfigStore } from './stores/config'
import { useTaskAlerts } from './composables/useTaskAlerts'
import { useTaskCommits } from './composables/useTaskCommits'
import { useDailyReview } from './composables/useDailyReview'
import { useEveningReminder } from './composables/useEveningReminder'
import { useWorkdayNudges } from './composables/useWorkdayNudges'
import { useCursorPassthrough } from './composables/useCursorPassthrough'
import { useUpdater } from './composables/useUpdater'
import TaskWindow from './components/TaskWindow.vue'
import SettingsWindow from './components/SettingsWindow.vue'
import RiskWindow from './components/RiskWindow.vue'
import ReviewWindow from './components/ReviewWindow.vue'
import UpdateWindow from './components/UpdateWindow.vue'
import BindTaskWindow from './components/BindTaskWindow.vue'
import WelcomeWizard from './components/WelcomeWizard.vue'

const store = useAppStore()
const configStore = useConfigStore()
const { refresh: refreshAlerts } = useTaskAlerts()
const { fetchCommits } = useTaskCommits({ autoLoad: true })
const { openReview } = useDailyReview()
useEveningReminder({
  onTrigger: () => {
    // 不传 duration=0 — 那会让气泡永久挂着、状态卡在 thinking 直到用户手动 ×。
    // 复盘窗口已经自动打开了，气泡只是个提示动作，15s 足够注意到。
    showAlert('今天的复盘看一下？', '📋', 'thinking', 15000)
    store.showReviewWindow = true
  },
})
useWorkdayNudges({
  onTrigger: (text, emoji) => {
    // 上班时段的小提示走 happy 表情、12s 自动消失，不打断工作
    showAlert(text, emoji, 'happy', 12000)
  },
})
// passthrough：让小人窗口的空白区域穿透到桌面，详见 composable 内部说明
useCursorPassthrough()

const updater = useUpdater({
  onAvailable: (version) => {
    // 新版本到位，挂常驻气泡（duration=0），告诉用户去菜单里看详情。
    // 不自动弹更新窗口 —— 用户可能正在专注做事，被弹窗打断很烦。
    showAlert(`新版本 v${version} 可用（菜单→检查更新）`, '✨', 'happy', 0)
  },
})
updater.start()

function openUpdateWindow() {
  closeAllPanels()
  store.showUpdateWindow = true
}

type JarvisState = 'idle' | 'thinking' | 'working' | 'warning' | 'happy'

const state = ref<JarvisState>('idle')
const showMenu = ref(false)
const alertText = ref('')
const alertEmoji = ref('')

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
    text: 'V2待命',
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
}
const current = computed(() => stateMap[state.value])
const hasAlert = computed(() => alertText.value !== '')

let alertTimer: number | null = null

function showAlert(text: string, emoji: string, s: JarvisState, duration = 5000) {
  state.value = s
  alertText.value = text
  alertEmoji.value = emoji
  if (alertTimer) clearTimeout(alertTimer)
  if (duration > 0) {
    alertTimer = window.setTimeout(() => {
      alertText.value = ''
      alertEmoji.value = ''
      // 气泡消失时状态也归还 idle，避免小人卡在 thinking/warning 直到下次主动改 state
      state.value = 'idle'
    }, duration)
  }
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
  if (!showMenu.value) closeAllPanels()
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
  // 用 screen.width/height 判断小人在屏幕的象限。多显示器只看主屏尺寸是个
  // 简化 —— 用户拖到副屏可能误判一次，但代价只是面板可能朝错方向，下次拖动
  // 会再修正，不至于卡死。
  const sw = window.screen.width
  const sh = window.screen.height
  const horiz = avatarScreenX >= sw / 2 ? 'r' : 'l'
  const vert = avatarScreenY >= sh / 2 ? 'b' : 't'
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
    // 真正发生过拖拽再算 anchor，避免点击也触发窗口位置抖动
    recomputeAnchor()
  }
}

function handleAvatarLeftClick() {
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
  const { overdueCount, todayCount, soonCount, stackedDays, overdueTasks } = store
  if (overdueCount.value > 0) {
    const maxDays = Math.max(...overdueTasks.value.map(t => -t.daysUntilDue))
    showAlert(
      `你有 ${overdueCount.value} 个任务已逾期，最久 ${maxDays} 天`,
      '🔥', 'warning', 0,
    )
    state.value = 'warning'
  } else if (todayCount.value > 0) {
    showAlert(`今天有 ${todayCount.value} 个任务到期`, '⏰', 'warning', 10000)
    state.value = 'warning'
  } else if (stackedDays.value.length > 0) {
    const s = stackedDays.value[0]
    showAlert(`${s.date} 有 ${s.count} 个任务堆在一天，建议提前处理`, '⚠️', 'warning', 10000)
    state.value = 'warning'
  } else if (soonCount.value > 0) {
    showAlert(`3 天内有 ${soonCount.value} 个任务到期`, '⏳', 'thinking', 8000)
  } else if (store.alertsLoaded) {
    showAlert('7 天内无紧急任务 ✓', '✅', 'happy', 5000)
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
  setTimeout(() => showAlert(`${configStore.config.assistantName} V2 已启动`, '🤖', 'idle', 3000), 500)
  // 等数据加载后显示提醒
  watch(() => store.alertsLoaded, (loaded) => {
    if (loaded) showTaskAlertBubble()
  }, { once: true })
})

// 监听后端发现的新任务，推入绑定队列。
// fetch_task_alerts 每次轮询都会做 snapshot diff，新出现的任务通过事件发上来；
// 首次启动 snapshot 不存在时返回空 diff，老用户升级时不会被存量任务轰炸。
let unlistenNewTasks: UnlistenFn | null = null
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
})
</script>

<template>
  <div class="jarvis-container" :data-anchor="avatarAnchor" @contextmenu.prevent="toggleMenu">
    <div v-if="showMenu" class="menu pointer-target">
      <button class="menu-item" @click="menuShowAlerts">
        <span>🔔</span><span>任务提醒</span>
        <span v-if="store.overdueCount > 0" class="menu-badge badge-danger">{{ store.overdueCount }}</span>
        <span v-else-if="store.todayCount > 0" class="menu-badge badge-warn">{{ store.todayCount }}</span>
        <span v-else-if="store.soonCount > 0" class="menu-badge badge-soon">{{ store.soonCount }}</span>
      </button>
      <button class="menu-item" @click="menuShowRisk"><span>⚠️</span><span>风险分析</span></button>
      <button class="menu-item" @click="menuShowReview"><span>📋</span><span>今日复盘</span></button>
      <button class="menu-item" @click="menuOpenChat"><span>💬</span><span>聊天（大窗）</span></button>
      <button class="menu-item" @click="menuShowSettings"><span>⚙️</span><span>设置</span></button>
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
    -->
    <div class="avatar-group">
      <!-- 弹出气泡（位于状态条上方，绑定到右边对齐） -->
      <transition name="bubble">
        <div v-if="hasAlert" class="alert-bubble pointer-target">
          <span class="alert-bubble__emoji">{{ alertEmoji }}</span>
          <span class="alert-bubble__text">{{ alertText }}</span>
          <button class="alert-bubble__close" @click.stop="alertText = ''; alertEmoji = ''; state = 'idle'" aria-label="关闭">×</button>
        </div>
      </transition>

      <div class="status-label pointer-target" :class="{ active: state === 'working' }">
        <span class="status-label__emoji">{{ current.emotion }}</span>
        <span class="status-label__text">{{ current.text }}</span>
      </div>

      <div class="avatar pointer-target" :class="{ active: state === 'working' }"
        @mousedown="onMouseDown"
      >
        <div class="avatar-glow" :style="{ boxShadow: `0 0 20px ${current.color}40, 0 0 40px ${current.color}20`, background: current.glowColor }" />
        <div class="avatar-body" :class="`state-${state}`">
          <svg viewBox="0 0 80 80" class="avatar-svg">
            <line x1="40" y1="12" x2="40" y2="4" :stroke="current.color" stroke-width="3"/>
            <circle cx="40" cy="3" r="3" :fill="current.color" class="blink"/>
            <rect x="18" y="14" width="44" height="32" rx="10" fill="none" :stroke="current.color" stroke-width="3"/>
            <rect x="28" y="22" width="10" height="8" rx="3" :fill="current.color" class="blink"/>
            <rect x="42" y="22" width="10" height="8" rx="3" :fill="current.color" class="blink"/>
            <rect x="32" y="38" width="16" height="3" rx="1.5" :fill="current.color" opacity="0.7"/>
            <rect x="24" y="50" width="32" height="18" rx="5" fill="none" :stroke="current.color" stroke-width="2" opacity="0.5"/>
            <circle cx="40" cy="58" r="3.5" :fill="current.color" :class="{ blink: state === 'working' }"/>
          </svg>
        </div>
        <div class="status-dot" :style="{ background: current.color }" />
      </div>
    </div>

    <!-- 任务提醒窗口 -->
    <TaskWindow />
    <!-- 风险分析窗口 -->
    <RiskWindow />
    <!-- 今日复盘窗口 -->
    <ReviewWindow />
    <!-- 设置面板 -->
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
  max-width: 260px;
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

.avatar { position: relative; width: 72px; height: 72px; cursor: pointer; }
.avatar-glow { position: absolute; inset: 0; border-radius: 50%; }
.avatar-body {
  position: absolute; inset: 3px; border-radius: 50%;
  background: linear-gradient(135deg, rgba(30, 41, 59, 0.98), rgba(15, 23, 42, 0.98));
  border: 1.5px solid rgba(255, 255, 255, 0.1);
  display: flex; align-items: center; justify-content: center;
}
.avatar.active .avatar-body { border-color: rgba(16, 185, 129, 0.3); }
.avatar-svg { width: 48px; height: 48px; }
.status-dot {
  position: absolute; bottom: 3px; right: 3px;
  width: 10px; height: 10px; border-radius: 50%;
  border: 2px solid rgba(15, 23, 42, 1);
}

.menu-btn {
  position: absolute; bottom: 86px; right: 16px;
  width: 22px; height: 22px;
  display: flex; align-items: center; justify-content: center;
  font-size: 14px; color: rgba(255, 255, 255, 0.3);
  background: rgba(255, 255, 255, 0.05); border-radius: 6px;
  cursor: pointer; line-height: 1;
}
.menu-btn:hover { color: rgba(255, 255, 255, 0.7); background: rgba(255, 255, 255, 0.1); }

.menu {
  position: absolute; bottom: 16px; right: 90px;
  background: rgba(15, 23, 42, 0.97); backdrop-filter: blur(16px);
  border-radius: 10px; border: 1px solid rgba(255, 255, 255, 0.06);
  box-shadow: 0 8px 32px rgba(0, 0, 0, 0.5);
  padding: 4px 0; z-index: 100; min-width: 130px;
  overflow: hidden;
}
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

@keyframes breathe {
  0%, 100% { transform: scale(1); opacity: 0.6; }
  50% { transform: scale(1.05); opacity: 1; }
}
@keyframes think {
  0%, 100% { transform: translateY(0); }
  25% { transform: translateY(-2px); }
  75% { transform: translateY(2px); }
}
@keyframes work {
  0% { transform: rotate(0deg); }
  25% { transform: rotate(3deg); }
  75% { transform: rotate(-3deg); }
  100% { transform: rotate(0deg); }
}
@keyframes alert {
  0%, 100% { transform: scale(1); }
  50% { transform: scale(1.1); }
}
@keyframes happy {
  0%, 100% { transform: translateY(0) scale(1); }
  50% { transform: translateY(-4px) scale(1.08); }
}
@keyframes glow-pulse {
  0%, 100% { opacity: 0.3; }
  50% { opacity: 0.8; }
}

.blink { animation: pulse 2s ease-in-out infinite; }

.avatar-body.state-idle { animation: breathe 3s ease-in-out infinite; }
.avatar-body.state-thinking { animation: think 1.5s ease-in-out infinite; }
.avatar-body.state-working { animation: work 0.8s ease-in-out infinite; }
.avatar-body.state-warning { animation: alert 0.6s ease-in-out infinite; }
.avatar-body.state-happy { animation: happy 1.2s ease-in-out infinite; }

.avatar-glow {
  animation: glow-pulse 2s ease-in-out infinite;
}
</style>
