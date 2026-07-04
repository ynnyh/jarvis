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
        @dragstart.prevent
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
  /* ============================================================
     二期居中布局（窗口 900×900，小人 72×72 恒居窗口正中心）：
     anchor（data-anchor）不再移动小人/窗口，只翻转面板、气泡、菜单的
     「开向」——始终朝小人对侧（= 屏幕中心方向）展开，避免出屏。

     定位基准一律用百分比（50% = 窗口中心）：百分比让小人在任何窗口
     尺寸下自动居中，窗口 resize 时浏览器在同一帧布局完成，无跨管线间隙。

     推导（900 态下的等价像素）：
     - 小人盒：中心 50% ± 半径 36 → calc(50% - 36px)，900 下 = 414..486；
     - 面板保持旧视觉尺寸 384×462（旧 400×560 窗减 inset 8/8/90/8）：
       面板只在 900 态出现（拖拽期菜单/面板不可见），inset 保持固定 px：
       水平贴小人对侧：内缘 = 小人内缘 414 - 8(间距) = 406
         → 近侧 inset = 900-406 = 494px，远侧 = 406-384 = 22px；
       垂直与小人同侧边缘对齐（b 锚底对齐 486、t 锚顶对齐 414）：
         → 同侧 inset = 900-486 = 414px，对侧 = 900-414-462 = 24px。
     默认 anchor=rb，data-anchor 切换时被同名规则覆盖。 */
  --avatar-top: auto;
  --avatar-right: calc(50% - 36px);
  --avatar-bottom: calc(50% - 36px);
  --avatar-left: auto;
  --panel-top: 24px;
  --panel-right: 494px;
  --panel-bottom: 414px;
  --panel-left: 22px;
  /* 透明窗浮层直接悬在桌面上，主题里的彩色泛光段（如 cyber 的青色 glow）会形成脏晕，
     这里覆写为纯黑投影（保留 cyber 原值的黑色段）；实底窗口不受影响。 */
  --panel-shadow: 0 8px 28px rgba(0, 0, 0, 0.7);
  --menu-shadow: 0 6px 24px rgba(0, 0, 0, 0.6);
}
.jarvis-container[data-anchor="rt"] {
  --avatar-top: calc(50% - 36px);
  --avatar-right: calc(50% - 36px);
  --avatar-bottom: auto;
  --avatar-left: auto;
  --panel-top: 414px;
  --panel-bottom: 24px;
}
.jarvis-container[data-anchor="lb"] {
  --avatar-top: auto;
  --avatar-right: auto;
  --avatar-bottom: calc(50% - 36px);
  --avatar-left: calc(50% - 36px);
  --panel-right: 22px;
  --panel-left: 494px;
}
.jarvis-container[data-anchor="lt"] {
  --avatar-top: calc(50% - 36px);
  --avatar-right: auto;
  --avatar-bottom: auto;
  --avatar-left: calc(50% - 36px);
  --panel-top: 414px;
  --panel-bottom: 24px;
  --panel-right: 22px;
  --panel-left: 494px;
}

.avatar-group {
  position: absolute;
  /* --avatar-* 四向 inset 均 calc(50% - 36px)：锚定角贴着「小人盒」的对应角
     （小人恒居窗口中心，900 下占 414..486，拖拽期 100 小窗下占 14..86），
     气泡/状态条从固定角向屏幕中心方向堆叠生长，无论气泡多高、窗口多大，
     小人像素位置永远在窗口正中。 */
  top: var(--avatar-top);
  right: var(--avatar-right);
  bottom: var(--avatar-bottom);
  left: var(--avatar-left);
  display: flex;
  /* anchor 在屏幕上半时反转子元素顺序：avatar 放最上、状态条/气泡在下方
     堆叠，让 group 的固定锚点始终落在 avatar 一端。 */
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
  /* 透明窗上 backdrop-filter 只会产生 artifact（比元素大一圈的白晕/竖边），底色直接给实色（--popup-bg 各主题均不透明） */
  background: var(--popup-bg);
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
  /* 贴小人角落，跟随百分比基准（小人恒居窗口中心，拖拽期缩窗也不跑位）：
     竖向在小人上/下 4px 外：底边 = 小人顶 (50%-36) - 4 → inset = 50%+40；
     横向缩进小人内 6px：右边 = 小人右 (50%+36) - 6 → inset = 50%-30。
     900 态等价旧值 490px/420px。 */
  position: fixed; bottom: calc(50% + 40px); right: calc(50% - 30px);
  width: 22px; height: 22px;
  display: flex; align-items: center; justify-content: center;
  font-size: 14px; color: var(--text-faint);
  background: var(--surface); border-radius: var(--radius-control);
  cursor: pointer; line-height: 1;
}
.menu-btn:hover { color: var(--text-ghost); background: var(--surface-item-active); }
/* menu-btn 跟随 anchor 翻转：用 CSS 变量跟 avatar-group 同步 */
.jarvis-container[data-anchor="rt"] .menu-btn { top: calc(50% + 40px); bottom: auto; }
.jarvis-container[data-anchor="lb"] .menu-btn { left: calc(50% - 30px); right: auto; }
.jarvis-container[data-anchor="lt"] .menu-btn { top: calc(50% + 40px); left: calc(50% - 30px); bottom: auto; right: auto; }

.menu-backdrop {
  position: fixed;
  inset: 0;
  z-index: 90;
}
.menu {
  /* 展开在小人对侧，跟随百分比基准：
     横向贴小人外 8px：右边 = 小人左 (50%-36) - 8 → inset = 50%+44；
     竖向缩进小人内 6px：底边 = 小人底 (50%+36) - 6 → inset = 50%-30。
     900 态等价旧值 420px/494px。 */
  position: fixed; bottom: calc(50% - 30px); right: calc(50% + 44px);
  /* 全局 .menu 工具类自带 backdrop-filter: blur(20px)，透明窗上只会产生 artifact，这里显式关掉 */
  background: var(--popup-bg); backdrop-filter: none; -webkit-backdrop-filter: none;
  border-radius: var(--radius-md); border: var(--menu-border);
  box-shadow: var(--menu-shadow);
  padding: 4px 0; z-index: 100; min-width: 130px;
  overflow: hidden;
}
/* 菜单跟随 anchor 翻转：始终在 menu-btn 靠屏幕中心的一侧 */
.jarvis-container[data-anchor="rt"] .menu { top: calc(50% - 30px); bottom: auto; }
.jarvis-container[data-anchor="lb"] .menu { left: calc(50% + 44px); right: auto; }
.jarvis-container[data-anchor="lt"] .menu { top: calc(50% - 30px); left: calc(50% + 44px); bottom: auto; right: auto; }
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
