<script setup lang="ts">
import { useWindowOrchestration } from './composables/useWindowOrchestration'
import TaskWindow from './components/TaskWindow.vue'
import SettingsWindow from './components/SettingsWindow.vue'
import RiskWindow from './components/RiskWindow.vue'
import ReviewWindow from './components/ReviewWindow.vue'
import UpdateWindow from './components/UpdateWindow.vue'
import BindTaskWindow from './components/BindTaskWindow.vue'
import WelcomeWizard from './components/WelcomeWizard.vue'
import PetAvatar from './components/PetAvatar.vue'
import ErrorBoundary from './components/ErrorBoundary.vue'

const {
  avatarAnchor, showMenu, state, current, hasAlert, stateFlashing,
  alertText, alertEmoji, alertActions,
  dailyMenuItems, systemMenuItems, costMenuItem,
  dockEdge, isPoked, updater, needsWizard,
  configStore, store,
  toggleMenu, menuShowAlerts, menuShowReview, menuOpenTodayPlan,
  menuOpenCost, menuOpenChat, menuShowSettings, menuCheckUpdate, menuQuit,
  onAvatarHover, onAvatarLeave, onMouseDown,
  runAlertAction, dismissAlert, onWizardDone,
} = useWindowOrchestration()
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
