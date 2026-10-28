<script setup lang="ts">
import { ref, computed, watch, onBeforeUnmount } from 'vue'
import { useWindowOrchestration } from './composables/useWindowOrchestration'
import TaskWindow from './components/TaskWindow.vue'
import SettingsWindow from './components/SettingsWindow.vue'
import RiskWindow from './components/RiskWindow.vue'
import ReviewWindow from './components/ReviewWindow.vue'
import UpdateWindow from './components/UpdateWindow.vue'
import BindTaskWindow from './components/BindTaskWindow.vue'
import WelcomeWizard from './components/WelcomeWizard.vue'
import PetAvatar from './components/PetAvatar.vue'
import PetChat from './components/PetChat.vue'
import ErrorBoundary from './components/ErrorBoundary.vue'
import { getPetById, MASCOT_ACTION_CLIPS } from './petManifest'

const {
  avatarAnchor, showMenu, state, current, hasAlert, stateFlashing,
  alertText, alertEmoji, alertActions,
  dailyMenuItems, systemMenuItems, costMenuItem,
  dockEdge, isPoked, updater, needsWizard,
  configStore, store, hearts, isDragging,
  // 漫游引擎（E2）：petAvatarEl 在 orchestration 声明（点播菜单与漫游共用），
  // 模板 ref="petAvatarEl" 绑定到这里
  petAvatarEl, petMoving, notifyPetInteraction,
  toggleMenu, menuShowAlerts, menuShowRisk, menuShowReview, menuOpenTodayPlan,
  menuOpenCost, menuOpenChat, menuOpenPetChat, closePetChat,
  menuShowSettings, menuCheckUpdate, menuQuit,
  onAvatarHover, onAvatarLeave, onMouseDown,
  runAlertAction, dismissAlert, onWizardDone,
} = useWindowOrchestration()

// ===== 点击 squish =====
// mousedown 即播 320ms 弹回动画；一旦发展成真拖拽（isDragging，位移 >5px 时置位）
// 立即撤掉，避免小人带着压扁形变被拖走。用 classList 直操作 + 强制 reflow，
// 是为了让同名动画在连续快速点击时能从头重播（响应式 class 无法重触发）。
// 作用在内层 .pet-squish-layer 而非 .avatar 本体：squish 关键帧 scale(1.12, 0.88)
// 会把元素纵向压掉 ~8%，若直接变形 .avatar（穿透判定的 pointer-target），按下点
// 靠近上下边缘时会瞬时落到变形后的盒子外 → 穿透轮询 elementFromPoint 判定
// 「不在 UI 上」→ 按压中途开穿透、事件流断掉。内层变形不影响 .avatar 自身
// 的命中边界（尺寸随 --pet-size 静态变化，动画期恒定），按压事件流永远稳定。
const avatarEl = ref<HTMLDivElement | null>(null)
const squishEl = ref<HTMLDivElement | null>(null)
let squishTimer: number | null = null

// ===== 小人尺寸 =====
// 以 CSS 变量 --pet-size 注入 .jarvis-container：avatar 命中盒、气泡/菜单/面板
// 的 calc 基准、PetAvatar 容器全部从它派生，改值只改变量、无重载。
// petSizePx 在 config.load 里已钳制进 [48,176] 且保证是数字，直读即可。
const petSizeStyle = computed(() => ({ '--pet-size': `${configStore.config.petSizePx}px` }))

function playSquish() {
  // 视频/帧动画类宠物（pack）不做 squish：素材自带动作反馈，外面再叠压扁形变
  // 读起来像「顿一下」（2026-10-28 用户实测否掉）；squish 留给无自动反馈的形象
  if (getPetById(configStore.config.petId).petType === 'pack') return
  // 漫游移动中不启动新 squish（E2：模式照 isDragging 门控——移动窗口上的
  // 形变动画没有锚点，视觉发飘）
  if (petMoving.value) return
  const el = squishEl.value
  if (!el) return
  el.classList.remove('pet-squish')
  void el.offsetWidth  // 强制 reflow：重置动画起点
  el.classList.add('pet-squish')
  if (squishTimer) clearTimeout(squishTimer)
  squishTimer = window.setTimeout(() => {
    squishTimer = null
    el.classList.remove('pet-squish')
  }, 340)
}

function cancelSquish() {
  if (squishTimer) { clearTimeout(squishTimer); squishTimer = null }
  squishEl.value?.classList.remove('pet-squish')
}

watch(isDragging, (dragging) => {
  if (dragging) cancelSquish()
})

// 漫游开始走动时撤掉在播的 squish（同拖拽置位先撤 squish 的模式）
watch(petMoving, (moving) => {
  if (moving) cancelSquish()
})

function onAvatarMouseDown(e: MouseEvent) {
  playSquish()
  // 点击小人 = 交互：漫游引擎据此唤醒睡觉、重置「连续 idle」时钟
  notifyPetInteraction()
  onMouseDown(e)
}

onBeforeUnmount(() => {
  if (squishTimer) clearTimeout(squishTimer)
})

// ===== 点播菜单（pack / mascot 宠物专属，PRD E1-6 与 M2-2，dsh-pet 同款「来一个」）=====
// 每次开菜单把可用小动作洗牌取最多 6 个；点击经 PetAvatar 暴露的
// playClip 播该片段（一次性，播完自动回 idle）。menu-themes 的固定 key
// 体系不动，动态区块直接追加在菜单模板末尾。petAvatarEl 声明在
// useWindowOrchestration（与 E2 漫游引擎共用同一个组件 ref）。
const packMenuActions = ref<string[]>([])
// 两级点播：null = 顶级（分类列表或洗牌平铺），非空 = 已展开某分类的动作列表。
// 包内任一动作带 category → 走分类模式；全不带 / mascot → 洗牌平铺现状。
const packMenuCategory = ref<string | null>(null)

const packCategories = computed<string[]>(() => {
  const pet = getPetById(configStore.config.petId)
  if (pet.petType !== 'pack') return []
  const cats: string[] = []
  for (const a of pet.pack?.clips.actions ?? []) {
    if (a.category && !cats.includes(a.category)) cats.push(a.category)
  }
  return cats
})

const packCategoryActions = computed(() => {
  const pet = getPetById(configStore.config.petId)
  return (pet.petType === 'pack' ? pet.pack?.clips.actions ?? [] : [])
    .filter(a => a.category === packMenuCategory.value)
})

function reshufflePackActions() {
  packMenuCategory.value = null
  const pet = getPetById(configStore.config.petId)
  // mascot 的动作是程序化预设（petManifest 单一来源）；pack 读包清单
  const names = pet.petType === 'pack'
    ? (pet.pack?.clips.actions ?? []).map(a => a.name)
    : pet.petType === 'mascot' ? MASCOT_ACTION_CLIPS.map(a => a.name) : []
  // Fisher-Yates 洗牌
  for (let i = names.length - 1; i > 0; i--) {
    const j = Math.floor(Math.random() * (i + 1))
    ;[names[i], names[j]] = [names[j], names[i]]
  }
  packMenuActions.value = names.slice(0, 6)
}

watch(showMenu, (open) => {
  if (open) reshufflePackActions()
})

/** name 缺省 = 「来一个」随机播一个 */
function playPackAction(name?: string) {
  const target = name ?? packMenuActions.value[Math.floor(Math.random() * packMenuActions.value.length)]
  if (!target) return
  showMenu.value = false
  void petAvatarEl.value?.playClip(target)
}

/** 召唤登场：魔法阵出场动画（PetAvatar 程序化效果，任意宠物可播） */
function menuSummon() {
  showMenu.value = false
  petAvatarEl.value?.playSummon()
}
</script>

<template>
  <ErrorBoundary>
  <div class="jarvis-container" :data-anchor="avatarAnchor" :style="petSizeStyle" @contextmenu.prevent="toggleMenu">
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
      <!-- 召唤登场：魔法阵出场动画（PetAvatar 程序化，任意宠物类型可播） -->
      <button class="menu-item" @click="menuSummon">
        <span>✦</span><span>召唤登场</span>
      </button>

      <div class="menu-divider" />

      <!-- 系统组 -->
      <button class="menu-item" @click="menuOpenChat">
        <span>{{ systemMenuItems[0].emoji }}</span><span>{{ systemMenuItems[0].label }}</span>
      </button>
      <button class="menu-item" @click="menuOpenPetChat">
        <span>{{ systemMenuItems[1].emoji }}</span><span>{{ systemMenuItems[1].label }}</span>
      </button>
      <button class="menu-item" @click="menuShowSettings">
        <span>{{ systemMenuItems[2].emoji }}</span><span>{{ systemMenuItems[2].label }}</span>
      </button>
      <button class="menu-item" @click="menuCheckUpdate">
        <span>{{ systemMenuItems[3].emoji }}</span><span>{{ systemMenuItems[3].label }}</span>
        <span v-if="updater.available.value" class="menu-badge badge-soon">新</span>
      </button>
      <button class="menu-item menu-item-danger" @click="menuQuit">
        <span>{{ systemMenuItems[4].emoji }}</span><span>{{ systemMenuItems[4].label }}</span>
      </button>

      <!-- 点播区块（pack / mascot 宠物且有动作时出现，PRD 指定放菜单末尾）：
           「来一个」播随机动作，下面是本次开菜单洗出的动作名（≤6 个）。
           menu-themes 固定 key 体系不进这里。 -->
      <template v-if="packMenuActions.length">
        <div class="menu-divider" />
        <button class="menu-item menu-item-pack-title" @click="playPackAction()">
          <span>▶</span><span>来一个</span>
        </button>
        <!-- 分类模式（动作带 category）：分类列表 → 该分类动作列表，两级点播 -->
        <template v-if="packCategories.length">
          <template v-if="!packMenuCategory">
            <button
              v-for="c in packCategories"
              :key="c"
              class="menu-item"
              @click="packMenuCategory = c"
            >
              <span>📁</span><span>{{ c }}</span>
            </button>
          </template>
          <template v-else>
            <button class="menu-item menu-item-pack-title" @click="packMenuCategory = null">
              <span>↩</span><span>{{ packMenuCategory }}</span>
            </button>
            <button
              v-for="a in packCategoryActions"
              :key="a.name"
              class="menu-item"
              @click="playPackAction(a.name)"
            >
              <span>🎬</span><span>{{ a.name }}</span>
            </button>
          </template>
        </template>
        <!-- 洗牌平铺（无分类数据 / mascot 现状） -->
        <template v-else>
          <button
            v-for="name in packMenuActions"
            :key="name"
            class="menu-item"
            @click="playPackAction(name)"
          >
            <span>🎬</span><span>{{ name }}</span>
          </button>
        </template>
      </template>
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
          <button class="alert-bubble__close" @mousedown.stop @click.stop="dismissAlert" aria-label="关闭">×</button>
        </div>
      </transition>

      <div v-show="false" class="status-label" />

      <!-- 状态条已删：状态用宠物外圈颜色 + 气泡传达，「待命中」常驻条只是视觉噪音，
           dock 时还会被屏幕边切掉。如果以后想加回来，把这一行删了把原 div 还原即可。 -->

      <!-- dragDropEnabled:false（tauri.conf.json）从 WebView2 层根治拖拽 ghost image 方框：
           startDragging 触发 OS 原生窗口移动，不再走 HTML5 drag-drop 路径生成 ghost。
           前端无需 @dragstart.prevent / draggable=false 任何花瓣拦截。 -->
      <div
        ref="avatarEl"
        class="avatar pointer-target"
        :class="{ docked: dockEdge && !isPoked }"
        @mousedown="onAvatarMouseDown"
      >
        <!-- squish 变形层：pointer-target 是外层 .avatar，这里变形不影响命中区 -->
        <div ref="squishEl" class="pet-squish-layer">
          <PetAvatar
            ref="petAvatarEl"
            :pet-id="configStore.config.petId"
            :color="current.color"
            :active="state === 'working'"
            :flashing="stateFlashing"
            :motion="current.animation"
            :idle="state === 'idle'"
            :dragging="isDragging"
            :moving="petMoving"
          />
        </div>
        <!-- 摸头心心：从头顶漂起淡出。容器 pointer-events:none——.avatar 是
             拖拽/点击热区，粒子绝不能截住鼠标事件（穿透判定依赖热区精确） -->
        <div v-if="hearts.length" class="pet-hearts">
          <span
            v-for="h in hearts"
            :key="h.id"
            class="pet-heart"
            :style="{
              left: `${h.x}px`,
              fontSize: `${h.size}px`,
              animationDuration: `${h.duration}ms`,
              animationDelay: `${h.delay}ms`,
            }"
          >{{ h.emoji }}</span>
        </div>
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
    <!-- 贴身迷你对话面板：avatar 窗内 DOM 面板，与小人同窗（拖动天然跟随）。
         不加入 onFocusChanged 的失焦收起清单——输入法/切窗会误关，只有
         Esc / × / 打开其它面板才收起 -->
    <Transition name="petchat">
      <PetChat v-if="store.isPetChatOpen" @close="closePetChat" />
    </Transition>
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
     二期居中布局（窗口 900×900，小人渲染盒 --pet-size 恒居窗口正中心）：
     anchor（data-anchor）不再移动小人/窗口，只翻转面板、气泡、菜单的
     「开向」——始终朝小人对侧（= 屏幕中心方向）展开，避免出屏。

     定位基准一律用百分比（50% = 窗口中心）：百分比让小人在任何窗口
     尺寸下自动居中，窗口 resize 时浏览器在同一帧布局完成，无跨管线间隙。

     小人渲染盒尺寸可变（48–176px 连续，见 stores/config.ts petSizePx），
     以 --pet-size（模板内联注入）派生 --pet-half = 尺寸一半。下面所有
     calc 里的 36px 时代硬编码全部换成 var(--pet-half) 表达式，等价换算
     以 72px（half=36）逐值对齐旧值：
     - 小人盒：中心 50% ± half；
     - 面板保持旧视觉尺寸 384×462：水平贴小人对侧、内缘留 8px 间距
       → 近侧 inset = 50% + half + 8px（72px 时 494px），
       远侧 = 50% - half - 8px - 384px（72px 时 22px）；
       垂直与小人同侧边缘对齐（b 锚底对齐 50%+half、t 锚顶对齐 50%-half）
       → 同侧 inset = 50% - half（72px 时 414px），
       对侧 = 同侧对齐值 - 462px（72px 时 24px）。
     默认 anchor=rb，data-anchor 切换时被同名规则覆盖。 */
  /* var() 回退值跟默认尺寸走（petSizePx 默认 96）；--pet-size 恒由模板注入，
     回退只是防御，公式本身不动 */
  --pet-half: calc(var(--pet-size, 96px) / 2);
  --avatar-top: auto;
  --avatar-right: calc(50% - var(--pet-half));
  --avatar-bottom: calc(50% - var(--pet-half));
  --avatar-left: auto;
  --panel-top: calc(50% + var(--pet-half) - 462px);
  --panel-right: calc(50% + var(--pet-half) + 8px);
  --panel-bottom: calc(50% - var(--pet-half));
  /* 远侧 inset 钳到 ≥8px：medium/large 下小人盒变宽，384 宽面板若按纯公式
     排，远缘会越过窗口左界被 OS 裁掉（128px 时 -6px、176px 时 -30px）。
     钳住后面板整体收窄十几~几十 px（内容区自带滚动），既不出窗也不盖小人。 */
  --panel-left: max(calc(50% - var(--pet-half) - 392px), 8px);
  /* 迷你对话面板（PetChat，320×440）：同一套 anchor 翻转体系，贴小人对侧
     展开、竖向与任务面板同基准（b 锚底对齐 50%+half）。默认 anchor=rb。 */
  --petchat-top: auto;
  --petchat-right: calc(50% + var(--pet-half) + 8px);
  --petchat-bottom: calc(50% - var(--pet-half));
  --petchat-left: auto;
  /* 透明窗浮层直接悬在桌面上，主题里的彩色泛光段（如 cyber 的青色 glow）会形成脏晕，
     这里覆写为纯黑投影（保留 cyber 原值的黑色段）；实底窗口不受影响。 */
  --panel-shadow: 0 8px 28px rgba(0, 0, 0, 0.7);
  --menu-shadow: 0 6px 24px rgba(0, 0, 0, 0.6);
}
.jarvis-container[data-anchor="rt"] {
  --avatar-top: calc(50% - var(--pet-half));
  --avatar-right: calc(50% - var(--pet-half));
  --avatar-bottom: auto;
  --avatar-left: auto;
  --panel-top: calc(50% - var(--pet-half));
  --panel-bottom: calc(50% + var(--pet-half) - 462px);
  --petchat-top: calc(50% - var(--pet-half));
  --petchat-bottom: auto;
}
.jarvis-container[data-anchor="lb"] {
  --avatar-top: auto;
  --avatar-right: auto;
  --avatar-bottom: calc(50% - var(--pet-half));
  --avatar-left: calc(50% - var(--pet-half));
  --panel-right: max(calc(50% - var(--pet-half) - 392px), 8px);
  --panel-left: calc(50% + var(--pet-half) + 8px);
  --petchat-left: calc(50% + var(--pet-half) + 8px);
  --petchat-right: auto;
}
.jarvis-container[data-anchor="lt"] {
  --avatar-top: calc(50% - var(--pet-half));
  --avatar-right: auto;
  --avatar-bottom: auto;
  --avatar-left: calc(50% - var(--pet-half));
  --panel-top: calc(50% - var(--pet-half));
  --panel-bottom: calc(50% + var(--pet-half) - 462px);
  --panel-right: max(calc(50% - var(--pet-half) - 392px), 8px);
  --panel-left: calc(50% + var(--pet-half) + 8px);
  --petchat-top: calc(50% - var(--pet-half));
  --petchat-bottom: auto;
  --petchat-left: calc(50% + var(--pet-half) + 8px);
  --petchat-right: auto;
}

.avatar-group {
  position: absolute;
  /* --avatar-* 四向 inset 均 calc(50% - half)：锚定角贴着「小人盒」的对应角
     （小人恒居窗口中心，尺寸随 --pet-size 三档变化，900 下 medium 占 386..514，
     拖拽期 100 小窗下占 14..86），气泡/状态条从固定角向屏幕中心方向堆叠生长，
     无论气泡多高、窗口多大，小人像素位置永远在窗口正中。 */
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
  right: calc(var(--pet-half) - 5px);  /* 尾巴中心对准 avatar 中心（half 处） */
  bottom: -5px;
  width: 10px;
  height: 10px;
  background: var(--popup-bg);
  border-right: var(--panel-border);
  border-bottom: var(--panel-border);
  transform: rotate(45deg);
}
/* 尾巴跟随 anchor 翻转（四锚点双向齐全）：
   - 左侧 anchor（lb/lt）：气泡贴小人左缘对齐（align-items: flex-start），
     尾巴改从气泡左缘算 half，否则落在气泡右端、偏离小人中心；
   - 上排 anchor（rt/lt）：气泡经 column-reverse 排在小人下方，
     尾巴翻到气泡顶边朝上指（换 border-left+top 形成朝上尖角）。 */
.jarvis-container[data-anchor="lb"] .alert-bubble::after,
.jarvis-container[data-anchor="lt"] .alert-bubble::after {
  right: auto;
  left: calc(var(--pet-half) - 5px);
}
.jarvis-container[data-anchor="rt"] .alert-bubble::after,
.jarvis-container[data-anchor="lt"] .alert-bubble::after {
  bottom: auto;
  top: -5px;
  border-right: none;
  border-bottom: none;
  border-left: var(--panel-border);
  border-top: var(--panel-border);
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
  /* 命中盒随 --pet-size 三档静态变化（穿透不变式允许静态放大，禁止动画期变形）；
     内容（Lottie/图片、状态点、地面投影、hover 放大、flashing 脉冲）全在
     PetAvatar.vue 里。.avatar 只做 size×size 事件钩子，事件挂在它上面（mousedown）。 */
  width: var(--pet-size, 128px);
  height: var(--pet-size, 128px);
  cursor: pointer;
}

/* 点击 squish：mousedown 即播（JS 挂 class），发展成拖拽立即撤。
   作用于内层 .pet-squish-layer（不含命中判定本体 .avatar），一次性 320ms 弹回，
   内部 transform 链（--pet-scale/offset）不受影响。 */
.pet-squish-layer {
  position: absolute;
  inset: 0;
}
.pet-squish-layer.pet-squish {
  animation: pet-squish 0.32s ease-out both;
}
@keyframes pet-squish {
  0%  { transform: scale(1.12, 0.88); }
  55% { transform: scale(0.97, 1.02); }
  100% { transform: scale(1, 1); }
}

/* ===== 摸头心心粒子 =====
   锚在 .avatar 顶部中点，从头顶漂起（bottom 锚到顶缘下 8px，尺寸无关）。
   热区纪律：容器与粒子一律 pointer-events:none，穿透判定依赖 .avatar 的热区
   边界精确，不容许粒子截事件。 */
.pet-hearts {
  position: absolute;
  left: 50%;
  bottom: calc(100% - 8px);
  width: 0;
  height: 0;
  pointer-events: none;
  z-index: 3;
}
.pet-heart {
  position: absolute;
  bottom: 0;
  transform: translateX(-50%);
  opacity: 0;
  animation-name: pet-heart-float;
  animation-timing-function: ease-out;
  animation-fill-mode: both;   /* delay 期间保持 0% 帧（隐形），播完停在 100%（淡出） */
}
@keyframes pet-heart-float {
  0%   { transform: translateX(-50%) translateY(0) scale(0.6); opacity: 0; }
  15%  { opacity: 0.95; }
  70%  { opacity: 0.8; }
  100% { transform: translateX(-50%) translateY(-52px) scale(1.15); opacity: 0; }
}

/* minimal 主题的设计定义是 no motion：新动效（squish + 心心）在此主题下全关 */
html[data-theme="minimal"] .pet-squish-layer.pet-squish { animation: none; }
html[data-theme="minimal"] .pet-heart { display: none; }

/* 迷你对话面板进出：纯淡入淡出（面板本体样式在 PetChat.vue）。
   minimal 主题 no motion，动画一并关掉（同 squish/心心先例） */
.petchat-enter-active,
.petchat-leave-active {
  transition: opacity 0.18s ease;
}
.petchat-enter-from,
.petchat-leave-to {
  opacity: 0;
}
html[data-theme="minimal"] .petchat-enter-active,
html[data-theme="minimal"] .petchat-leave-active { transition: none; }


.menu-btn {
  /* 贴小人角落，跟随百分比基准（小人恒居窗口中心，尺寸随 --pet-half 三档变化，
     拖拽期缩窗也不跑位）：竖向在小人上/下 half+4px 外；横向缩进小人内 6px。
     72px 时代等价旧值 490px/420px（half=36 → 50%+40 / 50%-30）。 */
  position: fixed; bottom: calc(50% + var(--pet-half) + 4px); right: calc(50% - var(--pet-half) + 6px);
  width: 22px; height: 22px;
  display: flex; align-items: center; justify-content: center;
  font-size: 14px; color: var(--text-faint);
  background: var(--surface); border-radius: var(--radius-control);
  cursor: pointer; line-height: 1;
}
.menu-btn:hover { color: var(--text-ghost); background: var(--surface-item-active); }
/* menu-btn 跟随 anchor 翻转：用 CSS 变量跟 avatar-group 同步 */
.jarvis-container[data-anchor="rt"] .menu-btn { top: calc(50% + var(--pet-half) + 4px); bottom: auto; }
.jarvis-container[data-anchor="lb"] .menu-btn { left: calc(50% - var(--pet-half) + 6px); right: auto; }
.jarvis-container[data-anchor="lt"] .menu-btn { top: calc(50% + var(--pet-half) + 4px); left: calc(50% - var(--pet-half) + 6px); bottom: auto; right: auto; }

.menu-backdrop {
  position: fixed;
  inset: 0;
  z-index: 90;
}
.menu {
  /* 展开在小人对侧，跟随百分比基准（half 随 --pet-size 三档变化）：
     横向贴小人外 8px：inset = 50% + half + 8px；
     竖向缩进小人内 6px：inset = 50% - half + 6px。
     72px 时代等价旧值 420px/494px。 */
  position: fixed; bottom: calc(50% - var(--pet-half) + 6px); right: calc(50% + var(--pet-half) + 8px);
  /* 全局 .menu 工具类自带 backdrop-filter: blur(20px)，透明窗上只会产生 artifact，这里显式关掉 */
  background: var(--popup-bg); backdrop-filter: none; -webkit-backdrop-filter: none;
  border-radius: var(--radius-md); border: var(--menu-border);
  box-shadow: var(--menu-shadow);
  padding: 4px 0; z-index: 100; min-width: 130px;
  overflow: hidden;
}
/* 菜单跟随 anchor 翻转：始终在 menu-btn 靠屏幕中心的一侧 */
.jarvis-container[data-anchor="rt"] .menu { top: calc(50% - var(--pet-half) + 6px); bottom: auto; }
.jarvis-container[data-anchor="lb"] .menu { left: calc(50% + var(--pet-half) + 8px); right: auto; }
.jarvis-container[data-anchor="lt"] .menu { top: calc(50% - var(--pet-half) + 6px); left: calc(50% + var(--pet-half) + 8px); bottom: auto; right: auto; }
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
/* 点播区块标题「来一个」：强调色字，与普通动作项区分 */
.menu-item-pack-title { color: var(--accent-text); }
.menu-item-pack-title:hover { background: var(--surface-item-hover); color: var(--accent-text); }
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
