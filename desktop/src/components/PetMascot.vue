<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { MASCOT_ACTION_CLIPS } from '../petManifest'

// ============================================================================
// 官方吉祥物「Jarvis 机器管家」：圆胖 chibi 悬浮机器人管家的纯 SVG 程序化渲染。
//
// 单一职责：只管「画什么、怎么动」。片段调度（playClip/hasClip/一次性时长收尾）
// 在 PetAvatar 的 mascot 映射层，漫游引擎经 packHasClip 的 mascot 映射零改动
// 复用既有 clip 链路（walk/sleep 由 roamer 播片段，本组件按 clip 名换挡）。
//
// 分层（自下而上，每层独立 transform 节点供动画驱动）：
//   推进器光晕 → 身体（胸口指示灯）→ 领结 → 手臂×2 → 头（眼睛×2）→ 天线+球。
//
// 红线对齐：
// - 命中盒不参与：svg 根 pointer-events:none（.pet-motion 已 none，显式兜底），
//   svg 根自身零 transform/动画，所有动画挂内层 <g>/<rect>/<ellipse>，
//   .pet-avatar 命中盒不随任何形变呼吸；
// - minimal 门控：循环动画节点全部在本组件子树内，由 PetAvatar 的
//   `html[data-theme="minimal"] .pet-motion .pet-mascot` 规则统一关停
//   （只点名 mascot 子树，pack 素材动画不受影响——pack 零回归）；
// - 透明窗动效纪律：无大面积脉动发光（推进器光晕是 34×11 的小椭圆、
//   低透明度纯色填色，非渐变非泛光），关键帧全 pet-m- 前缀防冲突。
//
// 姿态优先级（PRD M1-4）：held > sleep > walk > 点播动作 > stateMap motion > idle；
// 悬浮呼吸/天线晃/指示灯是常驻底层，随姿态「换挡」（更快的同位动画）而非停止。
// ============================================================================

const props = defineProps<{
  /** 状态色（stateMap.color）：眼睛/胸口指示灯/领结经 --m-color 继承 */
  color: string
  /** stateMap.animation：breathe/think/work/alert/happy（breathe 归 idle 常驻层） */
  motion: string
  /** 漫游移动中（兜底走路信号；正常路径由 roamer 播的 clip='walk' 驱动） */
  moving: boolean
  /** 拖拽拎起中（useAvatarDrag isDragging）——最高优先级姿态 held */
  dragging: boolean
  /** PetAvatar 当前片段名（null = 常态；sleep/walk/点播动作名） */
  clip: string | null
  /** 行进朝向（roamer setFacing）。素材默认朝右，left 时整体镜像 */
  facing: 'left' | 'right'
}>()

type MascotPose = 'idle' | 'held' | 'sleep' | 'walk' | 'action' | 'think' | 'work' | 'alert' | 'happy'

/** 需要角色化动画的 stateMap motion 值（idle/breathe/morning 等都落 idle 常驻层） */
const MOTION_POSES = new Set(['think', 'work', 'alert', 'happy'])
/** 点播动作名集合（hasClip 同源：petManifest 的 MASCOT_ACTION_CLIPS） */
const ACTION_NAMES = new Set(MASCOT_ACTION_CLIPS.map(a => a.name))

const pose = computed<MascotPose>(() => {
  if (props.dragging) return 'held'
  if (props.clip === 'sleep') return 'sleep'
  if (props.clip === 'walk' || props.moving) return 'walk'
  if (ACTION_NAMES.has(props.clip ?? '')) return 'action'
  if (MOTION_POSES.has(props.motion)) return props.motion as MascotPose
  return 'idle'
})

/** 点播动作的 ascii key（动作名是中文，不直接进 className） */
const actionKey = computed(() =>
  pose.value === 'action'
    ? (MASCOT_ACTION_CLIPS.find(a => a.name === props.clip)?.key ?? '')
    : '',
)

const poseClass = computed(() =>
  actionKey.value ? ['pose-' + pose.value, 'action-' + actionKey.value] : ['pose-' + pose.value],
)

/** 素材默认朝右（走路前倾 6° 朝右倾）；roamer setFacing('left') 时整体镜像 */
const facingFlipped = computed(() => props.facing !== 'right')

// ===== 眨眼（PRD：每 3–6s 随机）=====
// 唯一的 JS 动画参与点（随机器本身），眨眼表现仍是 CSS 关键帧：挂类 +
// 强制 reflow 重启动画（SVG 元素没有 offsetWidth，用 getBoundingClientRect，
// 手法同 App.vue 的 squish 重播）。定时器卫生：onBeforeUnmount 必清。
const eyesInnerEl = ref<SVGGElement | null>(null)
let blinkTimer: number | null = null

function scheduleBlink() {
  blinkTimer = window.setTimeout(() => {
    blinkTimer = null
    // 睡觉时眼皮是「— —」线且整脸变暗，不眨；其余姿态被遮挡时眨了也看不见，无副作用
    if (pose.value !== 'sleep') {
      const el = eyesInnerEl.value
      if (el) {
        el.classList.remove('pet-m-blinking')
        void el.getBoundingClientRect() // 强制 reflow：让同名动画从头播
        el.classList.add('pet-m-blinking')
      }
    }
    scheduleBlink()
  }, 3000 + Math.random() * 3000)
}

onMounted(scheduleBlink)

onBeforeUnmount(() => {
  if (blinkTimer !== null) {
    clearTimeout(blinkTimer)
    blinkTimer = null
  }
})
</script>

<template>
  <!-- svg 根：零 transform 零动画（穿透铁律），姿态类只驱动内层节点 -->
  <svg
    class="pet-mascot"
    :class="poseClass"
    :style="{ '--m-color': color }"
    viewBox="0 0 120 120"
    xmlns="http://www.w3.org/2000/svg"
    aria-hidden="true"
  >
    <g class="pet-m-flip" :class="{ 'pet-m-flipped': facingFlipped }">
      <!-- 全身层：悬浮呼吸 / 走路前倾 bob / happy 蹦跳 / alert 抖动 -->
      <g class="pet-m-body-all">
        <!-- 推进器光晕：低透明度纯色椭圆（替代腿），脉冲即呼吸感，不做泛光 -->
        <g class="pet-m-thruster">
          <ellipse class="pet-m-thruster-outer" cx="60" cy="105" rx="17" ry="5.5" />
          <ellipse class="pet-m-thruster-inner" cx="60" cy="105" rx="9" ry="3.2" />
        </g>
        <!-- 身体：圆胖胶囊 + 深色腹板 + 胸口指示灯（状态色） -->
        <rect class="pet-m-body" x="42" y="66" width="36" height="38" rx="17" />
        <rect class="pet-m-panel" x="48.5" y="77" width="23" height="20" rx="9" />
        <circle class="pet-m-light" cx="60" cy="87" r="3.6" />
        <!-- 管家领结（状态色，任务书口径）：先画领结再画头，上缘被头自然压住 -->
        <g class="pet-m-bowtie">
          <path class="pet-m-bowtie-wing" d="M59.5 75 L47.5 68.5 L47.5 81.5 Z" />
          <path class="pet-m-bowtie-wing" d="M60.5 75 L72.5 68.5 L72.5 81.5 Z" />
          <circle class="pet-m-bowtie-knot" cx="60" cy="75" r="3.1" />
        </g>
        <!-- 手臂 ×2：肩点在 rect 顶端（fill-box origin 50% 8%），摆臂/拎起/敲键盘都绕肩转 -->
        <rect class="pet-m-arm pet-m-arm-l" x="36.5" y="70" width="9" height="24" rx="4.5" />
        <rect class="pet-m-arm pet-m-arm-r" x="74.5" y="70" width="9" height="24" rx="4.5" />
        <!-- 头（大头 ~55%）：脸面板深色，眼睛/眼形切换组都在其内 -->
        <g class="pet-m-head">
          <circle class="pet-m-head-base" cx="60" cy="45" r="27" />
          <rect class="pet-m-visor" x="37" y="32" width="46" height="26" rx="13" />
          <!-- 常态椭圆眼：外层挂 think 扫描位移，内层挂眨眼/拎起睁大 -->
          <g class="pet-m-eyes">
            <g ref="eyesInnerEl" class="pet-m-eyes-inner">
              <ellipse class="pet-m-eye" cx="51" cy="45" rx="4.6" ry="6.4" />
              <ellipse class="pet-m-eye" cx="69" cy="45" rx="4.6" ry="6.4" />
              <circle class="pet-m-glint" cx="52.6" cy="42.6" r="1.4" />
              <circle class="pet-m-glint" cx="70.6" cy="42.6" r="1.4" />
            </g>
          </g>
          <!-- happy 弧线眼（^^）：默认隐，pose 切换显示 -->
          <g class="pet-m-eyes-alt pet-m-eyes-happy">
            <path d="M45.8 47.5 Q51 41.6 56.2 47.5" />
            <path d="M63.8 47.5 Q69 41.6 74.2 47.5" />
          </g>
          <!-- sleep 线眼（— —） -->
          <g class="pet-m-eyes-alt pet-m-eyes-sleep">
            <rect x="46.4" y="43.8" width="9.2" height="2.6" rx="1.3" />
            <rect x="64.4" y="43.8" width="9.2" height="2.6" rx="1.3" />
          </g>
        </g>
        <!-- 天线 + 球：整组绕根部摆（惯性感），球是固定琥珀色信标 -->
        <g class="pet-m-antenna">
          <rect class="pet-m-antenna-stem" x="58.8" y="10.5" width="2.4" height="9" rx="1.2" />
          <circle class="pet-m-antenna-ball" cx="60" cy="8.4" r="4.2" />
        </g>
      </g>
    </g>
    <!-- Zzz 气泡放 flip 层外：镜像时字母 Z 不能反写。SVG <text> 而非系统 emoji
         （emoji 字体渲染不可控）；基态 opacity:0，minimal 关动画后自然隐没 -->
    <g class="pet-m-zzz">
      <text class="pet-m-z pet-m-z1" x="77" y="31">Z</text>
      <text class="pet-m-z pet-m-z2" x="85" y="22.5">Z</text>
      <text class="pet-m-z pet-m-z3" x="92.5" y="14.5">Z</text>
    </g>
  </svg>
</template>

<style scoped>
/* ===== 基础层 =====
   主体冷灰白/深色面板是角色资产色（与 Lottie 宠物内嵌配色同类，不随主题，
   theming spec 的例外口径）；状态色只经 --m-color 出现在眼睛/指示灯/领结。 */
.pet-mascot {
  position: absolute;
  inset: 4px;
  width: calc(100% - 8px);
  height: calc(100% - 8px);
  display: block;
  overflow: visible; /* zzz 上飘 / happy 蹦跳的像素允许略出 viewBox（命中盒不受影响） */
  pointer-events: none;
  /* 只给「转圈」的 rotateY 一点纵深；2D 姿态不受影响 */
  perspective: 320px;
}

/* 朝向镜像挂 flip 层（素材层），svg 根零 transform */
.pet-m-flip {
  transform-origin: 60px 60px;
}
.pet-m-flip.pet-m-flipped {
  transform: scaleX(-1);
}

/* 全身层：悬浮呼吸常驻；各姿态用 (0,2,0) 规则换挡（transform-origin 取脚底，
   走路前倾绕脚转、happy 落地压扁才有「体重感」） */
.pet-m-body-all {
  transform-origin: 60px 102px;
  animation: pet-m-float 3.4s ease-in-out infinite;
}
@keyframes pet-m-float {
  0%, 100% { transform: translateY(0); }
  50% { transform: translateY(-2.4px); }
}

/* ===== 配色（角色资产色）===== */
.pet-m-body,
.pet-m-head-base,
.pet-m-arm {
  fill: #e8ecf2;
  stroke: #4a566b;
  stroke-opacity: 0.28;
  stroke-width: 1.4;
}
.pet-m-panel,
.pet-m-visor { fill: #252e3d; }
.pet-m-light { fill: var(--m-color); }
.pet-m-thruster-outer { fill: #7fd0e6; fill-opacity: 0.16; }
.pet-m-thruster-inner { fill: #7fd0e6; fill-opacity: 0.34; }
.pet-m-antenna-stem { fill: #8b96a8; }
/* 天线球固定琥珀色信标（任务书：状态色只出现在眼睛/指示灯/领结） */
.pet-m-antenna-ball { fill: #f2b63c; }
.pet-m-bowtie-wing,
.pet-m-bowtie-knot { fill: var(--m-color); }
.pet-m-bowtie-knot {
  stroke: #1f2733;
  stroke-opacity: 0.35;
  stroke-width: 1;
}
.pet-m-eye { fill: var(--m-color); }
.pet-m-glint { fill: #ffffff; opacity: 0.85; }
.pet-m-eyes-happy path {
  fill: none;
  stroke: var(--m-color);
  stroke-width: 3;
  stroke-linecap: round;
}
.pet-m-eyes-sleep rect { fill: var(--m-color); opacity: 0.72; }

/* ===== 常驻底层（idle 挡）：天线惯性晃 + 指示灯慢闪 + 推进器呼吸 + 双臂微动 ===== */
.pet-m-antenna {
  transform-origin: 60px 19px;
  animation: pet-m-antenna-sway 3.8s ease-in-out infinite;
}
@keyframes pet-m-antenna-sway {
  0%, 100% { transform: rotate(0deg); }
  30% { transform: rotate(4deg); }
  70% { transform: rotate(-4deg); }
}
@keyframes pet-m-antenna-fast {
  0%, 100% { transform: rotate(-7deg); }
  50% { transform: rotate(7deg); }
}
.pet-m-light { animation: pet-m-light-blink 2.6s ease-in-out infinite; }
@keyframes pet-m-light-blink {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.4; }
}
@keyframes pet-m-light-fast {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.25; }
}
.pet-m-thruster {
  transform-origin: 60px 105px;
  animation: pet-m-thrust 2.8s ease-in-out infinite;
}
@keyframes pet-m-thrust {
  0%, 100% { opacity: 1; transform: scaleY(1); }
  50% { opacity: 0.55; transform: scaleY(0.82); }
}
@keyframes pet-m-thrust-fast {
  0%, 100% { opacity: 1; transform: scaleY(1.1); }
  50% { opacity: 0.45; transform: scaleY(0.68); }
}
@keyframes pet-m-thrust-sleep {
  0%, 100% { opacity: 0.5; transform: scaleY(0.9); }
  50% { opacity: 0.3; transform: scaleY(0.8); }
}
/* 手臂绕肩转：fill-box 取自身几何（叶子节点 bbox 稳定，不会因动画抖 origin） */
.pet-m-arm {
  transform-box: fill-box;
  transform-origin: 50% 8%;
}
.pet-m-arm-l { animation: pet-m-idle-arm-l 3.4s ease-in-out infinite; }
.pet-m-arm-r { animation: pet-m-idle-arm-r 3.4s ease-in-out infinite; }
@keyframes pet-m-idle-arm-l {
  0%, 100% { transform: rotate(10deg); }
  50% { transform: rotate(5deg); }
}
@keyframes pet-m-idle-arm-r {
  0%, 100% { transform: rotate(-10deg); }
  50% { transform: rotate(-5deg); }
}
.pet-m-head { transform-origin: 60px 66px; }
.pet-m-eyes-inner { transform-origin: 60px 45px; }

/* ===== 眼形切换：常态椭圆 / happy 弧线 / sleep 线 ===== */
.pet-m-eyes-alt { opacity: 0; }
.pose-happy .pet-m-eyes-inner,
.action-greet .pet-m-eyes-inner,
.pose-sleep .pet-m-eyes-inner { opacity: 0; }
.pose-happy .pet-m-eyes-happy,
.action-greet .pet-m-eyes-happy { opacity: 1; }
.pose-sleep .pet-m-eyes-sleep { opacity: 1; }

/* ===== walk（clip='walk' 或 moving 兜底）：前倾 6° + 快频浮动 + 摆臂 + 推进器脉冲 =====
   前倾方向随镜像层翻转：素材朝右 = rotate(6°)，朝左时 scaleX(-1) 自动镜像成左倾 */
.pose-walk .pet-m-body-all { animation: pet-m-walk-bob 0.52s ease-in-out infinite; }
@keyframes pet-m-walk-bob {
  0%, 100% { transform: rotate(6deg) translateY(0); }
  50% { transform: rotate(6deg) translateY(-3.2px); }
}
.pose-walk .pet-m-arm-l { animation: pet-m-walk-arm-l 0.52s ease-in-out infinite; }
.pose-walk .pet-m-arm-r { animation: pet-m-walk-arm-r 0.52s ease-in-out infinite; }
@keyframes pet-m-walk-arm-l {
  0%, 100% { transform: rotate(24deg); }
  50% { transform: rotate(-20deg); }
}
@keyframes pet-m-walk-arm-r {
  0%, 100% { transform: rotate(-24deg); }
  50% { transform: rotate(20deg); }
}
.pose-walk .pet-m-thruster { animation: pet-m-thrust-fast 0.4s ease-in-out infinite; }
.pose-walk .pet-m-antenna { animation: pet-m-antenna-fast 1s ease-in-out infinite; }

/* ===== held（dragging）：身体小幅摆 + 双臂下垂晃荡 + 眼睛睁大 ===== */
.pose-held .pet-m-body-all { animation: pet-m-held-sway 1.7s ease-in-out infinite; }
@keyframes pet-m-held-sway {
  0%, 100% { transform: rotate(-2.6deg); }
  50% { transform: rotate(2.6deg); }
}
.pose-held .pet-m-arm-l { animation: pet-m-dangle-l 1.5s ease-in-out infinite; }
.pose-held .pet-m-arm-r { animation: pet-m-dangle-r 1.5s ease-in-out infinite; }
@keyframes pet-m-dangle-l {
  0%, 100% { transform: rotate(-5deg); }
  50% { transform: rotate(7deg); }
}
@keyframes pet-m-dangle-r {
  0%, 100% { transform: rotate(5deg); }
  50% { transform: rotate(-7deg); }
}
.pose-held .pet-m-eyes-inner { transform: scale(1.08, 1.28); }

/* ===== sleep（clip='sleep'）：眼闭成线 + 整体变暗 + Zzz 气泡 =====
   变暗用滤镜（theming spec：滤镜类在 minimal 下可保留）；呼吸/推进器换慢挡 */
.pet-mascot.pose-sleep { filter: brightness(0.72) saturate(0.7); }
.pose-sleep .pet-m-body-all { animation: pet-m-sleep-float 5s ease-in-out infinite; }
@keyframes pet-m-sleep-float {
  0%, 100% { transform: translateY(0); }
  50% { transform: translateY(-1.2px); }
}
.pose-sleep .pet-m-thruster { animation: pet-m-thrust-sleep 4.2s ease-in-out infinite; }
.pose-sleep .pet-m-antenna { animation: pet-m-antenna-sway 6s ease-in-out infinite; }
/* Zzz：display 门控（非 sleep 时整组不渲染不动画），基态 opacity:0 保证
   minimal 关动画后无静态残影 */
.pet-m-zzz { display: none; }
.pose-sleep .pet-m-zzz { display: inline; }
.pet-m-z {
  fill: #93a0b4;
  font-family: var(--font-display, 'Segoe UI', monospace);
  font-weight: 700;
  opacity: 0;
}
.pet-m-z1 { font-size: 11px; animation: pet-m-zzz 2.7s ease-in-out infinite; }
.pet-m-z2 { font-size: 14px; animation: pet-m-zzz 2.7s ease-in-out 0.9s infinite; }
.pet-m-z3 { font-size: 17px; animation: pet-m-zzz 2.7s ease-in-out 1.8s infinite; }
@keyframes pet-m-zzz {
  0% { opacity: 0; transform: translate(0, 5px); }
  25% { opacity: 0.95; }
  60% { opacity: 0.65; }
  100% { opacity: 0; transform: translate(4px, -9px); }
}

/* ===== think：眼球左右扫描 + 天线球快晃（扫描位移挂外层，与内层眨眼互不覆盖）===== */
.pose-think .pet-m-eyes { animation: pet-m-scan 1.7s ease-in-out infinite; }
@keyframes pet-m-scan {
  0%, 35% { transform: translateX(-3.6px); }
  50%, 85% { transform: translateX(3.6px); }
  100% { transform: translateX(-3.6px); }
}
.pose-think .pet-m-antenna { animation: pet-m-antenna-fast 0.8s ease-in-out infinite; }

/* ===== work：双臂抬到身前交替敲 + 指示灯快闪 ===== */
.pose-work .pet-m-arm-l { animation: pet-m-work-arm-l 0.5s ease-in-out infinite; }
.pose-work .pet-m-arm-r { animation: pet-m-work-arm-r 0.5s ease-in-out infinite; }
@keyframes pet-m-work-arm-l {
  0%, 100% { transform: rotate(-128deg); }
  50% { transform: rotate(-110deg); }
}
@keyframes pet-m-work-arm-r {
  0%, 100% { transform: rotate(110deg); }
  50% { transform: rotate(128deg); }
}
.pose-work .pet-m-light { animation: pet-m-light-fast 0.5s ease-in-out infinite; }

/* ===== happy：蹦跳 + ^^ 弧线眼 + 双臂上举欢呼 ===== */
.pose-happy .pet-m-body-all { animation: pet-m-hop 0.62s ease-in-out infinite; }
@keyframes pet-m-hop {
  0% { transform: translateY(0) scale(1, 1); }
  30% { transform: translateY(-8px) scale(0.97, 1.04); }
  62% { transform: translateY(0) scale(1.06, 0.93); }
  80% { transform: translateY(0) scale(0.99, 1.01); }
  100% { transform: translateY(0) scale(1, 1); }
}
.pose-happy .pet-m-arm-l { animation: pet-m-cheer-l 0.62s ease-in-out infinite; }
.pose-happy .pet-m-arm-r { animation: pet-m-cheer-r 0.62s ease-in-out infinite; }
@keyframes pet-m-cheer-l {
  0%, 100% { transform: rotate(-152deg); }
  50% { transform: rotate(-140deg); }
}
@keyframes pet-m-cheer-r {
  0%, 100% { transform: rotate(152deg); }
  50% { transform: rotate(140deg); }
}

/* ===== alert：左右抖 + 双眼变红闪（红色是警示状态色，theming 例外口径）===== */
.pose-alert .pet-m-body-all { animation: pet-m-shake 0.42s ease-in-out infinite; }
@keyframes pet-m-shake {
  0%, 100% { transform: translateX(-2.4px); }
  50% { transform: translateX(2.4px); }
}
.pose-alert .pet-m-eye {
  fill: #f43f5e;
  animation: pet-m-alert-flash 0.42s ease-in-out infinite;
}
@keyframes pet-m-alert-flash {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.35; }
}

/* ===== 点播动作（PRD M3，一次性，时长收尾在 PetAvatar，播完摘类回 idle）===== */
/* 打招呼：右臂举起摆两下（起收都走 idle 角度，衔接自然）+ ^^ 眼（见眼形切换） */
.action-greet .pet-m-arm-r { animation: pet-m-greet-arm 1.8s ease-in-out both; }
@keyframes pet-m-greet-arm {
  0% { transform: rotate(-10deg); }
  18% { transform: rotate(-140deg); }
  34% { transform: rotate(-158deg); }
  50% { transform: rotate(-136deg); }
  66% { transform: rotate(-158deg); }
  82%, 100% { transform: rotate(-10deg); }
}
/* 转圈：整体 rotateY 一圈（2D 内的 3D 变换）+ 蓄力压扁；对称造型让镜像态的
   进入/退出无可见跳变 */
.action-spin .pet-m-flip { animation: pet-m-spin 1.3s ease-in-out both; }
@keyframes pet-m-spin {
  0% { transform: rotateY(0deg); }
  100% { transform: rotateY(360deg); }
}
.action-spin .pet-m-body-all { animation: pet-m-spin-antic 1.3s ease-in-out both; }
@keyframes pet-m-spin-antic {
  0% { transform: scale(1, 1); }
  15%, 85% { transform: scale(0.92, 1.06); }
  100% { transform: scale(1, 1); }
}
/* 敬礼：右臂举到眉侧定格 + 头微点（傲娇） */
.action-salute .pet-m-arm-r { animation: pet-m-salute-arm 1.5s ease-in-out both; }
@keyframes pet-m-salute-arm {
  0% { transform: rotate(-10deg); }
  16%, 84% { transform: rotate(-148deg); }
  100% { transform: rotate(-10deg); }
}
.action-salute .pet-m-head { animation: pet-m-salute-nod 1.5s ease-in-out both; }
@keyframes pet-m-salute-nod {
  0%, 100% { transform: rotate(0deg); }
  30%, 80% { transform: rotate(-4deg); }
}

/* ===== 眨眼（JS 3–6s 随机器挂类）=====
   声明在文件尾部：与 held 睁大的静态 transform 同为 (0,2,0)，后者生效时以此覆盖 */
.pet-m-eyes-inner.pet-m-blinking { animation: pet-m-blink 0.22s ease-in-out both; }
@keyframes pet-m-blink {
  0%, 100% { transform: scaleY(1); }
  45%, 55% { transform: scaleY(0.08); }
}
</style>
