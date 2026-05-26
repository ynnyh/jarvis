<script setup lang="ts">
import { ref, computed, watch, onMounted, onUnmounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useAppStore } from './stores/app'
import { useConfigStore } from './stores/config'
import { useTaskAlerts } from './composables/useTaskAlerts'
import { useTaskCommits } from './composables/useTaskCommits'
import { useDailyReview } from './composables/useDailyReview'
import { useEveningReminder } from './composables/useEveningReminder'
import { useWorkdayNudges } from './composables/useWorkdayNudges'
import { useCursorPassthrough, type PassthroughDebug } from './composables/useCursorPassthrough'
import { useUpdater } from './composables/useUpdater'
import TaskWindow from './components/TaskWindow.vue'
import SettingsWindow from './components/SettingsWindow.vue'
import RiskWindow from './components/RiskWindow.vue'
import ReviewWindow from './components/ReviewWindow.vue'
import UpdateWindow from './components/UpdateWindow.vue'
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
// passthrough 诊断面板：mac 上排查"整窗不可点"必需。窗口左上角浮一个小框，
// 显示 Rust 给的原始 cursor / win_pos / scale 以及前端推得的 (x,y) 和当前
// elementFromPoint 命中的元素。鼠标移到小人上时数字应该指向 svg/.avatar；如果
// 永远是 body 或者数字明显不对就能定位计算 bug。
const passthroughDebug = ref<PassthroughDebug | null>(null)
const isMac = navigator.userAgent.includes('Mac')
useCursorPassthrough((info) => {
  if (!isMac) return  // 只在 mac 上显示，Windows 上没问题不打扰
  passthroughDebug.value = info
})

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
let mouseDownTime = 0
let mouseDownX = 0
let mouseDownY = 0
let isDragging = false

function onMouseDown(e: MouseEvent) {
  if (e.button !== 0) return
  isDragging = false
  mouseDownTime = Date.now()
  mouseDownX = e.screenX
  mouseDownY = e.screenY
}

function onMouseMove(e: MouseEvent) {
  if (!(e.buttons & 1)) return
  if (isDragging) return
  const dx = Math.abs(e.screenX - mouseDownX)
  const dy = Math.abs(e.screenY - mouseDownY)
  if (dx > 5 || dy > 5) {
    isDragging = true
    invoke('drag_window').catch(() => {})
  }
}

function onMouseUp(e: MouseEvent) {
  const duration = Date.now() - mouseDownTime
  const dx = Math.abs(e.screenX - mouseDownX)
  const dy = Math.abs(e.screenY - mouseDownY)
  if (!isDragging && duration < 300 && dx < 5 && dy < 5) {
    // 左键点击小人：按 config.leftClickAction 分发。打开前先关掉其他所有面板/菜单。
    // 当前打开的就是目标面板时点一下应该收起 —— 保持原有 toggle 行为。
    handleAvatarLeftClick()
  }
  isDragging = false
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
  setTimeout(() => showAlert(`${configStore.config.assistantName} V2 已启动`, '🤖', 'idle', 3000), 500)
  // 等数据加载后显示提醒
  watch(() => store.alertsLoaded, (loaded) => {
    if (loaded) showTaskAlertBubble()
  }, { once: true })
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
})
</script>

<template>
  <div class="jarvis-container" @contextmenu.prevent="toggleMenu">
    <!-- mac passthrough 诊断面板（临时） -->
    <div v-if="passthroughDebug" class="debug-overlay pointer-target">
      <div>cursor css: {{ passthroughDebug.x.toFixed(0) }}, {{ passthroughDebug.y.toFixed(0) }}</div>
      <div>raw cur: {{ passthroughDebug.rawCursorX.toFixed(0) }}, {{ passthroughDebug.rawCursorY.toFixed(0) }}</div>
      <div>raw win: {{ passthroughDebug.rawWinX }}, {{ passthroughDebug.rawWinY }} · s={{ passthroughDebug.scale }}</div>
      <div>inner: {{ passthroughDebug.innerW }}x{{ passthroughDebug.innerH }}</div>
      <div>el: {{ passthroughDebug.elTag }}</div>
      <div>onUI: {{ passthroughDebug.onUI }} · ignoring: {{ passthroughDebug.ignoring }}</div>
      <div v-if="passthroughDebug.err">err: {{ passthroughDebug.err }}</div>
    </div>
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
        @mousemove="onMouseMove"
        @mouseup="onMouseUp"
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
}

/* mac 调试面板：固定窗口左上角，半透明背景，等宽字体好看数字 */
.debug-overlay {
  position: fixed;
  top: 8px;
  left: 8px;
  z-index: 999;
  padding: 6px 8px;
  font-size: 10px;
  line-height: 1.4;
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  color: rgba(255, 255, 255, 0.85);
  background: rgba(0, 0, 0, 0.7);
  border: 1px solid rgba(255, 255, 255, 0.15);
  border-radius: 6px;
  pointer-events: auto;
  white-space: nowrap;
}

.avatar-group {
  position: absolute;
  bottom: 10px;
  right: 10px;
  display: flex;
  flex-direction: column;
  align-items: flex-end;   /* 子元素全部贴右边对齐 */
  gap: 6px;
  touch-action: none;
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
