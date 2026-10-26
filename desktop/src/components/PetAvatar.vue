<script setup lang="ts">
import { ref, computed, watch, onMounted, onBeforeUnmount, shallowRef, type CSSProperties } from 'vue'
import lottie, { type AnimationItem } from 'lottie-web'
import { getPetById, getPackClipSrc, MASCOT_ACTION_CLIPS } from '../petManifest'
import type { PetPackAction, PetPackClip } from '../api/petPack'
import type { RigDoc } from '../petRig/types'
import PetRig from './PetRig.vue'

// 单一职责：把选中的宠物渲染进一个尺寸可配（--pet-size，三档）的透明容器，
// 自由轮廓显示（不裁圆、无发光环）+ 地面投影 + 右下角状态点。换 petId 时销毁旧
// 动画再加载新动画，避免内存堆积。
//
// 支持五种宠物类型：
// - lottie: 使用 lottie-web 渲染 Lottie JSON
// - image: 使用 <img> 标签，可选 CSS 动画
// - gif: 使用 <img> 标签，GIF 自带动画
// - pack: 宠物包 v2，多动作片段（gif/webm/spritesheet），idle 为常态，其余
//   片段经 playClip 点播（E2 漫游引擎 + 右键「来一个」菜单共用）
// - mascot: 官方吉祥物（PetRig.vue 读 rig 文档的数据驱动程序化角色）；
//   sleep/walk/点播动作复用同一套 playClip/stopClip 调度，只是「片段」由
//   rig 数据表驱动（时长查 MASCOT_ACTION_CLIPS，收尾同 pack 一次性片段）
//
// 状态动效（motion）：stateMap.animation 驱动的 CSS overlay 动画，挂在
// .pet-motion 包裹层上。动画只作用于小人本体——透明窗上不做任何大面积
// 脉动发光（拖拽闪烁源，v0.10.6 事故，发光环已删），也不碰
// .pet-lottie/.pet-media 自身的 translate+scale transform 链
// （--pet-scale/offset 由 manifest 注入，必须原样生效）。

const props = defineProps<{
  petId: string
  color: string       // 当前 state 的主色（状态点 + flash 前后的状态色表达）
  active: boolean     // working 态时高亮
  flashing: boolean   // 状态切换瞬间脉冲一次
  motion: string      // stateMap.animation：breathe/think/work/alert/happy
  idle: boolean       // 仅 idle 态允许 fidget 微动作
  dragging: boolean   // 拖拽进行中：不启动新的 fidget（已在播的让其自然结束）
  moving?: boolean    // E2 漫游移动中：不启动新的 fidget（模式照 dragging）；
                      // 无 pack clip 在播时强制套 walk 弹跳（程序化走路，2026-10-22 全宠物开放）
}>()

const containerEl = ref<HTMLDivElement | null>(null)
const imgEl = ref<HTMLImageElement | null>(null)
// shallowRef 避免 Vue 把 lottie 的 AnimationItem 当作普通对象深度代理 —— 它内部
// 持有 SVGElement / 帧数据，被 proxy 包装可能造成 # 私有字段访问失败或性能问题。
const animation = shallowRef<AnimationItem | null>(null)

const pet = computed(() => getPetById(props.petId))
const renderConfig = computed(() => pet.value.render ?? {})
const isLottie = computed(() => !pet.value.petType || pet.value.petType === 'lottie')
const isImage = computed(() => pet.value.petType === 'image')
const isGif = computed(() => pet.value.petType === 'gif')
const isMedia = computed(() => isImage.value || isGif.value)
const isPack = computed(() => pet.value.petType === 'pack')
const isMascot = computed(() => pet.value.petType === 'mascot')
// mascot 的 rig 文档（petManifest 已 parseRigDoc 校验）；P2 起 rig 角色走同一分支
const mascotDoc = computed(() => (isMascot.value ? (pet.value.data as RigDoc) : null))

// ===== 宠物包 v2（pack 类型）：多动作片段渲染与点播 =====
// idle 是常态渲染；walk/held/sleep/actions 经 playClip 点播（一次性播完自动回
// idle，loop 片段由外部 stopClip 收尾）。素材 data URL 按 clip 惰性加载并缓存。

const pack = computed(() => pet.value.pack ?? null)

/** 当前播放的片段名（null = 常态 idle）。E2 漫游引擎经 defineExpose 消费 */
const currentClip = ref<string | null>(null)
/** 素材当前朝向；翻转只挂素材容器（.pet-pack-flip），不碰 .avatar 命中盒 */
const facingDir = ref<'left' | 'right'>('left')

watch(pack, (p) => {
  // 换包：朝向复位为素材默认朝向，并打断在播片段
  stopClip()
  facingDir.value = p?.facing === 'right' ? 'right' : 'left'
})

/** 片段名 → 片段定义（idle / walk / held / sleep 直查，其余查 actions） */
function resolveClip(name: string): PetPackClip | PetPackAction | null {
  const clips = pack.value?.clips
  if (!clips) return null
  if (name === 'walk' || name === 'held' || name === 'sleep') return clips[name] ?? null
  const action = clips.actions?.find(a => a.name === name)
  if (action) return action
  if (name === 'idle') return clips.idle ?? null
  return null
}

function hasClip(name: string): boolean {
  // mascot：sleep/walk 恒可播（程序化内置片段，PRD M2-2），其余查点播动作清单。
  // 与 petManifest.packHasClip 的 mascot 映射保持同一口径（roamer 走那份）
  if (isMascot.value) {
    return name === 'sleep' || name === 'walk' || MASCOT_ACTION_CLIPS.some(a => a.name === name)
  }
  const clips = pack.value?.clips
  if (!clips) return false
  if (name === 'idle') return !!clips.idle
  if (name === 'walk' || name === 'held' || name === 'sleep') return !!clips[name]
  return (clips.actions ?? []).some(a => a.name === name)
}

/** E2 漫游引擎用：按行进方向翻转素材（素材层 pointer-events:none，命中盒不动） */
function setFacing(dir: 'left' | 'right') {
  facingDir.value = dir
}

/** 实际渲染的片段：没在点播时就渲染 idle（pack 必有 idle） */
const activePackClip = computed(() => resolveClip(currentClip.value ?? 'idle'))

// —— 片段播完收尾：一次只挂一个 resolve，finishClip 幂等可重入 ——
let clipEndTimer: number | null = null
let clipResolve: (() => void) | null = null
// loop 状态用 ref：模板里 <video :loop> 需要随点播参数响应式切换
const clipLoop = ref(false)

/** gif 一次性片段的兜底时长：GIF 没有播放结束事件，感知不了真实时长 */
const PACK_GIF_ONESHOT_MS = 3500

function finishClip() {
  if (clipEndTimer !== null) { clearTimeout(clipEndTimer); clipEndTimer = null }
  const resolve = clipResolve
  clipResolve = null
  clipLoop.value = false
  currentClip.value = null
  resolve?.()
}

/**
 * 播放指定片段（E2 漫游引擎 + 右键点播菜单共用入口）。
 * - 非 loop：播完自动回 idle 并 resolve（spritesheet 按 frames/fps 计时长，
 *   webm 等视频 ended 事件，gif 无结束信号按常量兜底）；
 * - loop：一直播，由外部 stopClip() 收尾（stopClip 会 resolve 挂起的 promise）。
 * 片段不存在时直接 resolve（调用方应先用 hasClip 过滤）。
 */
function playClip(name: string, opts?: { loop?: boolean }): Promise<void> {
  if (isMascot.value) return playMascotClip(name, opts)
  if (!isPack.value || !hasClip(name)) return Promise.resolve()
  if (clipResolve) finishClip() // 打断上一个片段（清掉它的收尾定时器）
  clipLoop.value = opts?.loop ?? false
  currentClip.value = name
  return new Promise<void>((resolve) => {
    clipResolve = resolve
    if (clipLoop.value) return
    const clip = activePackClip.value
    if (clip?.type === 'spritesheet') {
      const durationMs = ((clip.frames ?? 1) / (clip.fps || 8)) * 1000
      clipEndTimer = window.setTimeout(finishClip, durationMs + 60)
    } else if (clip?.type === 'gif') {
      clipEndTimer = window.setTimeout(finishClip, PACK_GIF_ONESHOT_MS)
    }
  })
}

/**
 * mascot 片段播放（PRD M2-2/M3）：walk/sleep 天然循环（漫游引擎播了靠 stopClip
 * 收尾，与 pack 真素材同语义）；点播动作一次性，按 MASCOT_ACTION_CLIPS 的固定
 * 时长定时收尾（程序化动画没有真实播放结束事件）。currentClip/收尾 promise 与
 * pack 共用同一套机制，roamer/点播菜单零感知差异。
 */
function playMascotClip(name: string, opts?: { loop?: boolean }): Promise<void> {
  if (!hasClip(name)) return Promise.resolve()
  if (clipResolve) finishClip() // 打断上一个片段（清掉它的收尾定时器）
  const loop = name === 'walk' || name === 'sleep' || opts?.loop === true
  clipLoop.value = loop
  currentClip.value = name
  return new Promise<void>((resolve) => {
    clipResolve = resolve
    if (loop) return
    const durationMs = MASCOT_ACTION_CLIPS.find(a => a.name === name)?.durationMs ?? 1500
    clipEndTimer = window.setTimeout(finishClip, durationMs + 60)
  })
}

function stopClip() {
  if (clipResolve) finishClip()
}

/** 素材实际朝向与包默认朝向相反时才翻转 */
const packFlipped = computed(() =>
  isPack.value && facingDir.value !== (pack.value?.facing === 'right' ? 'right' : 'left'),
)

const packStyle = computed<CSSProperties>(() => ({
  '--pack-scale': String(pack.value?.scale ?? 1),
}))

// —— 素材加载：切片段即触发惰性加载，过期响应（期间已切走）直接丢弃 ——

const packClipSrc = ref('')
const packClipFailed = ref(false)
const packVideoEl = ref<HTMLVideoElement | null>(null)

// —— spritesheet 渲染：多行雪碧图没法用单条 steps() 表达二维步进，
//    只能逐帧生成 keyframes 注入 document 头部的 <style>（卸载时移除）。
//    steps(1,end) 让相邻关键帧之间保持起始帧值 → 每段恰好定格一帧。

let spriteStyleEl: HTMLStyleElement | null = null
let spriteStyleSeq = 0
const spriteAnimName = ref('')

// 切片段/换包即重载素材；immediate 保证挂载时（缓存已就绪的场景）idle 立即开载。
// immediate 同步执行回调：其引用的 ref/函数都声明在本 watch 之前，无 TDZ 问题。
watch(activePackClip, async (clip) => {
  packClipFailed.value = false
  packClipSrc.value = ''
  spriteAnimName.value = ''
  const packData = pack.value
  if (!clip || !packData) return
  const src = await getPackClipSrc(packData.id, clip.file)
  if (clip !== activePackClip.value) return // 加载期间已切到别的片段
  if (src) {
    packClipSrc.value = src
    if (clip.type === 'spritesheet') injectSpriteKeyframes(clip)
  } else {
    // 素材读取失败：不白屏，隐藏该片段；在播的点播片段直接结束回 idle
    packClipFailed.value = true
    if (clipResolve) finishClip()
  }
}, { immediate: true })

/** webm 加载失败降级：隐藏素材；在播片段结束回 idle。
 *  注：webm alpha 是实验特性 —— WebView2 对 VP9-alpha 的支持未真机验证，
 *  PRD 接受「不透明底」降级；加载失败则整段隐藏回 idle，不白屏。 */
function onPackMediaError() {
  packClipFailed.value = true
  if (clipResolve) finishClip()
}

/** 一次性 webm 播完（ended 事件）收尾；loop 播放不会触发 */
function onPackVideoEnded(e: Event) {
  if (packVideoEl.value && e.target !== packVideoEl.value) return
  if (clipResolve) finishClip()
}

function onPackSpriteEnded() {
  if (clipResolve) finishClip()
}

function injectSpriteKeyframes(clip: PetPackClip) {
  const frames = Math.max(1, Math.floor(clip.frames ?? 1))
  const frameW = Math.max(1, clip.frameW ?? 64)
  const frameH = Math.max(1, clip.frameH ?? 64)
  const columns = Math.max(1, clip.columns ?? frames)
  const animName = `pack-sprite-${++spriteStyleSeq}`
  const parts: string[] = []
  for (let i = 0; i < frames; i++) {
    const x = -(i % columns) * frameW
    const y = -Math.floor(i / columns) * frameH
    parts.push(`${((i / frames) * 100).toFixed(4)}% { background-position: ${x}px ${y}px; }`)
  }
  const lastIdx = frames - 1
  const lastX = -(lastIdx % columns) * frameW
  const lastY = -Math.floor(lastIdx / columns) * frameH
  parts.push(`100% { background-position: ${lastX}px ${lastY}px; }`)
  if (!spriteStyleEl) {
    spriteStyleEl = document.createElement('style')
    document.head.appendChild(spriteStyleEl)
  }
  spriteStyleEl.textContent = `@keyframes ${animName} { ${parts.join(' ')} }`
  spriteAnimName.value = animName
}

onBeforeUnmount(() => {
  spriteStyleEl?.remove()
  spriteStyleEl = null
  // 定时器卫生：在播片段的收尾定时器随组件一起清，别留到卸载后触发
  if (clipEndTimer !== null) { clearTimeout(clipEndTimer); clipEndTimer = null }
  // 挂起的 playClip promise 一并 resolve（漫游引擎/点播菜单可能在 await，
  // 不留悬挂句柄）；不动 currentClip/clipLoop —— 卸载期不再触发
  // activePackClip watch 的重载链
  clipResolve?.()
  clipResolve = null
})

const spriteBoxStyle = computed<CSSProperties>(() => {
  const clip = activePackClip.value
  if (!clip || clip.type !== 'spritesheet') return {}
  return {
    '--sprite-fw': String(Math.max(1, Math.floor(clip.frameW ?? 64))),
    '--sprite-fh': String(Math.max(1, Math.floor(clip.frameH ?? 64))),
  }
})

const spriteStyle = computed<CSSProperties>(() => {
  const clip = activePackClip.value
  if (!clip || clip.type !== 'spritesheet' || !packClipSrc.value) return {}
  return {
    '--sprite-src': `url("${packClipSrc.value}")`,
    '--sprite-anim': spriteAnimName.value,
    '--sprite-frames': String(Math.max(1, Math.floor(clip.frames ?? 1))),
    '--sprite-fps': String(clip.fps && clip.fps > 0 ? clip.fps : 8),
    // 点播中的一次性片段只播一轮；常态 idle 与 loop 片段（漫游 walk/held/sleep）
    // 无限循环 —— 与视频路径的 :loop="currentClip === null || clipLoop" 同一口径，
    // 不然 loop 片段的雪碧图会播一轮就冻在末帧
    '--sprite-once': !currentClip.value || clipLoop.value ? 'infinite' : '1',
  }
})

const lottieStyle = computed<CSSProperties>(() => ({
  '--pet-scale': String(renderConfig.value.scale ?? 1),
  '--pet-offset-x': `${renderConfig.value.offsetX ?? 0}px`,
  '--pet-offset-y': `${renderConfig.value.offsetY ?? 0}px`,
}))

const mediaStyle = computed<CSSProperties>(() => ({
  '--pet-scale': String(renderConfig.value.scale ?? 1),
  '--pet-offset-x': `${renderConfig.value.offsetX ?? 0}px`,
  '--pet-offset-y': `${renderConfig.value.offsetY ?? 0}px`,
}))

const imageAnimationClass = computed(() => {
  if (!isImage.value) return ''
  const anim = pet.value.imageAnimation
  if (anim === 'breath') return 'anim-breath'
  if (anim === 'swing') return 'anim-swing'
  if (anim === 'rotate') return 'anim-rotate'
  if (anim === 'bounce') return 'anim-bounce'
  return ''
})

/** 图片/GIF 的 Base64 data URL */
const mediaSrc = computed(() => {
  if (!isMedia.value) return ''
  const data = pet.value.data
  if (typeof data === 'string') {
    // 已经是 data URL 或 base64 字符串
    return data.startsWith('data:') ? data : `data:image/png;base64,${data}`
  }
  return ''
})

function mountAnimation() {
  if (!containerEl.value || !isLottie.value) return
  if (animation.value) {
    animation.value.destroy()
    animation.value = null
  }
  animation.value = lottie.loadAnimation({
    container: containerEl.value,
    renderer: 'svg',
    loop: true,
    autoplay: true,
    animationData: pet.value.data,
  })
  // 换 petId 重挂载后速度按当前 motion 恢复（autoplay 默认 1.0）
  applyLottieSpeed()
}

onMounted(() => {
  mountAnimation()
})

onBeforeUnmount(() => {
  if (animation.value) {
    animation.value.destroy()
    animation.value = null
  }
})

watch(() => props.petId, () => {
  // 换宠即打断在播片段（pack 目标另有 watch(pack) 兜底，且先于本 watcher 执行；
  // 这里收口非 pack 目标——mascot 点播动作的收尾定时器还没到点就切走时，
  // 不让新宠短暂吃到 clip-playing 门控）
  stopClip()
  mountAnimation()
  // mascot 素材默认朝右（rig 文档 doc.facing 口径）：挂载/切回时对齐朝向，否则首帧按
  // facingDir 缺省 'left' 渲染成镜像姿态（新配置默认就是 mascot）。其余类型的
  // 朝向不在这里动——pack 由 watch(pack) 按包默认复位，lottie 静止渲染不读朝向
  if (pet.value.petType === 'mascot') facingDir.value = 'right'
}, { flush: 'post' }) // post：等 DOM 按 petType 重渲染完（lottie 容器挂好 ref）再挂动画。
// 默认 pre 时这是个时序地雷：新配置默认 mascot（无 lottie 容器），老用户开机从
// mascot 切到 lottie 宠物的瞬间 containerEl 还是 null → 静默早退 → 容器渲染出来
// 后没人再挂动画，小人永久空白（2026-10-23 真机实锤）。

// ===== motion → CSS overlay 动画 + Lottie 变速 =====

// 白名单：未知 motion 值直接不挂动画类，避免脏字符串进 DOM
const MOTION_CLASSES = new Set(['breathe', 'think', 'work', 'alert', 'happy'])
// 程序化走路（E2 漫游 2026-10-22 开放全宠物）：roamer 滑窗期间（moving）且没有
// pack 片段在播 → 强制套 walk 弹跳。优先级：moving 弹跳 > stateMap motion >
// fidget（fidget 本就被 moving 门控，watch 里再收掉在播的）。pack 走真 walk
// clip 时 clip 在播、判定为假——既有行为零变化（有真素材不套程序化动效）。
const isProceduralWalk = computed(() => props.moving === true && currentClip.value === null)
// 方向镜像：仅 lottie/图片/GIF 需要（pack 走 .pet-pack-flip 既有链路）。非 pack
// 的默认朝向视为 left，roamer setFacing('right') 时镜像；只在程序化走路期间挂，
// 到站（moving 撤销）即恢复原始朝向——静止形象与既有渲染零差异。
const walkFlipClass = computed(() =>
  isProceduralWalk.value && !isPack.value && facingDir.value !== 'left' ? 'pet-motion-flip' : '',
)
const motionClass = computed(() => {
  // mascot：think/work/alert/happy 有角色化版本（PetRig 内层动画），通用
  // overlay 形变不叠加（双层 bounce/rotate 会打架）；flashing 脉冲与 clip-playing
  // 门控（.pet-motion 上）不受影响，照常作用
  if (isMascot.value) return ''
  if (isProceduralWalk.value) return 'pet-motion-walk'
  return MOTION_CLASSES.has(props.motion) ? `pet-motion-${props.motion}` : ''
})

// Lottie 变速让本体动画节奏跟状态匹配（image/gif 无法变速，接受原速）。
// petId 重挂载后 mountAnimation 里会按当前 motion 恢复速度。
const LOTTIE_SPEED: Record<string, number> = { work: 1.35, think: 0.8, alert: 1.5 }
function applyLottieSpeed() {
  if (!animation.value) return
  animation.value.setSpeed(LOTTIE_SPEED[props.motion] ?? 1)
}
watch(() => props.motion, () => {
  applyLottieSpeed()
})

// ===== idle fidget 微动作（歪头看左/右、伸懒腰）=====
// 每 25–50s 随机播一次一次性微动作，1.5–2.5s 后自然结束（临时 class + 定时移除，
// 不打断 Lottie 本体）。拖拽进行中到点则跳过本轮（不启动新的，已开播的让它放完）。
const FIDGET_CLASSES = ['pet-fidget-tilt-left', 'pet-fidget-tilt-right', 'pet-fidget-stretch']
const fidgetClass = ref('')
let fidgetTimer: number | null = null
let fidgetEndTimer: number | null = null

function clearFidgetTimers() {
  if (fidgetTimer !== null) { clearTimeout(fidgetTimer); fidgetTimer = null }
  if (fidgetEndTimer !== null) { clearTimeout(fidgetEndTimer); fidgetEndTimer = null }
}

function scheduleFidget() {
  fidgetTimer = window.setTimeout(() => {
    fidgetTimer = null
    // 到点再核对资格：不在 idle、正在拖拽、正在漫游移动或在播 pack 片段 → 本轮跳过，仅排下一轮
    if (props.idle && !props.dragging && !props.moving && !currentClip.value) {
      fidgetClass.value = FIDGET_CLASSES[Math.floor(Math.random() * FIDGET_CLASSES.length)]
      fidgetEndTimer = window.setTimeout(() => {
        fidgetClass.value = ''
        fidgetEndTimer = null
      }, 1500 + Math.random() * 1000) // 1.5–2.5s 自然归位
    }
    scheduleFidget()
  }, 25000 + Math.random() * 25000) // 25–50s 随机间隔
}

onMounted(() => {
  scheduleFidget()
})

onBeforeUnmount(() => {
  clearFidgetTimers()
})

// 走路弹跳 > fidget（PRD 优先级）：移动开始时立刻收掉在播的 fidget（一次性动
// 作，摘 class 即归位），避免两个 keyframes 在同一元素上抢动画。
watch(() => props.moving, (moving) => {
  if (moving) {
    clearFidgetTimers()
    fidgetClass.value = ''
  }
})

// ===== 召唤登场（2026-10-24，《光明使者》式魔法阵出场）=====
// 纯 SVG/CSS 程序化：魔法阵描线（外环→五角星→符文环）+ 整阵缓旋 + 角色从阵中
// 升起 + 收尾闪光淡出。零素材、任意宠物类型通用、命中盒零参与（svg 根
// pointer-events:none，升起 transform 挂 .pet-rise 包裹层不碰 .pet-motion 的
// 动画链）。触发：启动首次加载 / 切换宠物 / 右键菜单「召唤登场」。
const summoning = ref(false)
let summonTimer: number | null = null

/** 播一次召唤（幂等可重入：重触发先摘类 + 强制 reflow 从头播） */
function playSummon() {
  if (summonTimer !== null) { clearTimeout(summonTimer); summonTimer = null }
  summoning.value = false
  // 摘类后强制 reflow 再挂类，同名动画从头播（SVG 无 offsetWidth，手法同眨眼）
  void document.body.offsetWidth
  summoning.value = true
  summonTimer = window.setTimeout(() => {
    summonTimer = null
    summoning.value = false
  }, 1900)
}

onBeforeUnmount(() => {
  if (summonTimer !== null) { clearTimeout(summonTimer); summonTimer = null }
})

// 触发 1：启动登场——PetAvatar 常驻，mounted 即应用启动（avatar 窗内容挂载）。
// pack 宠物 idle 素材是惰性加载的，稍等首帧就位再召唤，视觉更完整
onMounted(() => {
  window.setTimeout(() => playSummon(), 150)
})

// 触发 2：切换宠物 = 新角色登场（immediate:false，启动登场由 mounted 管）
watch(() => props.petId, () => {
  playSummon()
})

// 面板外的消费方（App.vue 点播菜单 / E2 漫游引擎）经组件 ref 调用这组接口
defineExpose({
  playClip,
  stopClip,
  hasClip,
  setFacing,
  currentClip,
  playSummon,
})

</script>

<template>
  <!-- clip-playing：pack 片段播放期间暂停 motion overlay（见样式区门控规则） -->
  <div class="pet-avatar" :class="{ active, flashing, 'clip-playing': currentClip !== null }">
    <!-- 地面投影：贴容器底的静态椭圆软阴影，给「立在桌面上」的重量感。
         纯黑低透明（同 App.vue 面板投影的既有做法，透明窗上彩色泛光会成脏晕）。
         热区纪律：渲染像素在 .avatar 命中盒下缘附近，必须 pointer-events:none；
         静态不动画——透明窗上任何脉动都是拖拽闪烁源。 -->
    <div class="pet-shadow" />
    <!-- 召唤魔法阵：描线→缓旋→闪光淡出，全程只在本组件子树（svg 根
         pointer-events:none + 零命中盒参与）；minimal 主题整组关停。 -->
    <div v-if="summoning" class="pet-summon-circle" aria-hidden="true">
      <svg class="pet-summon-svg" viewBox="0 0 120 120" xmlns="http://www.w3.org/2000/svg">
        <g class="pet-summon-rot">
          <!-- 外环（青）：pathLength=1 + dashoffset 描线 -->
          <circle class="pet-summon-ring" cx="60" cy="60" r="52" pathLength="1" />
          <!-- 五角星（琥珀）：外接圆 r=40，五点每 72° 隔点相连 -->
          <path class="pet-summon-star" pathLength="1"
            d="M60 20 L83.5 92.4 L22.0 47.6 L98.0 47.6 L36.5 92.4 Z" />
          <!-- 符文环（青，虚线段模拟符文刻度）+ 内环 -->
          <circle class="pet-summon-runes" cx="60" cy="60" r="46" pathLength="1" />
          <circle class="pet-summon-inner" cx="60" cy="60" r="26" pathLength="1" />
        </g>
        <!-- 收尾闪光：淡青色圆盘脉冲（低透明纯色，非渐变非泛光） -->
        <circle class="pet-summon-flash" cx="60" cy="60" r="30" />
      </svg>
    </div>
    <!-- 召唤升起包裹层：transform 只在召唤期存在，不与 .pet-motion 的动画链
         同元素共存（各挂一层，零冲突） -->
    <div class="pet-rise" :class="{ 'pet-rise-active': summoning }">
    <!-- motion/fidget/程序化走路动效只挂这个包裹层，不碰 .pet-lottie/.pet-media 的
         transform 链（走路的 scaleX(-1) 镜像除外：追加在链末尾、基项原样保留） -->
    <div class="pet-motion" :class="[motionClass, fidgetClass, walkFlipClass]">
      <div v-if="isLottie" ref="containerEl" class="pet-lottie" :style="lottieStyle" />
      <img
        v-else-if="isMedia"
        ref="imgEl"
        class="pet-media"
        :class="imageAnimationClass"
        :style="mediaStyle"
        :src="mediaSrc"
        alt=""
      />
      <!-- pack 宠物：idle 为常态，点播片段同容器切换（gif/webm/spritesheet 三种素材）。
           整棵子树 pointer-events:none，翻转挂内层 .pet-pack-flip，不碰命中盒。 -->
      <div v-else-if="isPack" class="pet-pack" :style="packStyle">
        <div class="pet-pack-flip" :class="{ 'pack-flipped': packFlipped }">
          <img
            v-if="activePackClip && activePackClip.type === 'gif'"
            v-show="!packClipFailed"
            class="pet-pack-media"
            :src="packClipSrc"
            alt=""
            @error="onPackMediaError"
          />
          <!-- webm 素材：VP9-alpha 在 WebView2 上是实验特性（未真机验证，PRD 接受
               不透明底降级）；:key 按文件重建元素，保证切片段时 autoplay 必触发 -->
          <video
            v-else-if="activePackClip && activePackClip.type === 'webm'"
            v-show="!packClipFailed"
            ref="packVideoEl"
            :key="activePackClip.file"
            class="pet-pack-media"
            :src="packClipSrc"
            muted
            playsinline
            autoplay
            :loop="currentClip === null || clipLoop"
            @ended="onPackVideoEnded"
            @error="onPackMediaError"
          />
          <div
            v-else-if="activePackClip && activePackClip.type === 'spritesheet'"
            v-show="!packClipFailed"
            class="pet-sprite-box"
            :style="spriteBoxStyle"
          >
            <div class="pet-sprite" :style="spriteStyle" @animationend="onPackSpriteEnded" />
          </div>
        </div>
      </div>
      <!-- mascot 官方吉祥物：rig 数据驱动的程序化角色（PetRig）。位于 .pet-motion 内
           （pointer-events:none 继承 + 组件根显式兜底），动画全挂 rig 部件节点；
           currentClip 直接作 clip prop——sleep/walk/点播动作由 PetRig 换挡 -->
      <PetRig
        v-else-if="isMascot && mascotDoc"
        :doc="mascotDoc"
        :color="color"
        :motion="motion"
        :moving="moving === true"
        :dragging="dragging"
        :clip="currentClip"
        :facing="facingDir"
      />
    </div>
    </div>
    <div class="pet-dot" :style="{ background: color }" />
  </div>
</template>

<style scoped>
.pet-avatar {
  position: relative;
  /* 尺寸三档：--pet-size 从 App.vue 的 .jarvis-container 按配置注入，
     缺省 128 = medium 默认档。容器内全是相对尺寸（inset/百分比），
     petManifest 的 render.scale/offset 是相对倍率，天然兼容。 */
  width: var(--pet-size, 128px);
  height: var(--pet-size, 128px);
  cursor: pointer;
  transition: transform 0.18s ease;
}
.pet-avatar:hover { transform: scale(1.08); }

/* 地面投影：脚下椭圆软阴影（纯黑低透明，透明窗上彩色泛光会成脏晕）。
   pointer-events:none——渲染像素超出 .avatar 命中盒下缘，禁命中后穿透
   判定不受影响；静态不动画，也不随 motion/fidget 形变（拖拽闪烁红线）。 */
.pet-shadow {
  position: absolute;
  left: 50%;
  bottom: 0;
  width: 64%;
  height: 10px;
  transform: translateX(-50%);
  background: radial-gradient(closest-side, rgba(0, 0, 0, 0.35), rgba(0, 0, 0, 0));
  z-index: 0;
  pointer-events: none;
}

.pet-lottie {
  position: absolute;
  inset: 0;
  z-index: 1;
  /* Lottie 渲染出来的 SVG 默认会带边距和 viewBox 留白；用 transform: scale
     让动画稍微撑大一点，容器内更饱满。具体倍数靠经验，1.1 视觉刚好。 */
  display: flex;
  align-items: center;
  justify-content: center;
  transform: translate(var(--pet-offset-x, 0), var(--pet-offset-y, 0)) scale(var(--pet-scale, 1));
  transform-origin: center center;
}
.pet-lottie :deep(svg) {
  width: 100% !important;
  height: 100% !important;
}

.pet-dot {
  position: absolute;
  bottom: 3px;
  right: 3px;
  width: 12px;
  height: 12px;
  border-radius: 50%;
  border: 2px solid rgba(15, 23, 42, 1);
  z-index: 2;
}

/* 图片/GIF 宠物：object-fit:contain 按自然轮廓显示——透明背景 PNG/GIF 呈自由
   形态，不裁圆、不裁边；不透明方图就照方图显示。 */
.pet-media {
  position: absolute;
  inset: 4px;
  z-index: 1;
  width: calc(100% - 8px);
  height: calc(100% - 8px);
  object-fit: contain;
  transform: translate(var(--pet-offset-x, 0), var(--pet-offset-y, 0)) scale(var(--pet-scale, 1));
  transform-origin: center center;
}

/* ===== 宠物包 v2 渲染层 =====
   穿透铁律：整棵子树 pointer-events:none（素材像素绝不截事件）；
   翻转 scaleX(-1) 挂内层 .pet-pack-flip（素材容器），不碰 .avatar 命中盒；
   --pack-scale 缩放同样挂 .pet-pack 层，与命中盒无关。 */
.pet-pack {
  position: absolute;
  inset: 4px;
  z-index: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  pointer-events: none;
  transform: scale(var(--pack-scale, 1));
  transform-origin: center center;
}
.pet-pack-flip {
  /* 绝对定位撑满 .pet-pack：给内部素材一个确定的 100% 基准，
     max-width/max-height 才能正确把大图收到 --pet-size 盒内 */
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  pointer-events: none;
  transform-origin: center center;
}
.pet-pack-flip.pack-flipped {
  transform: scaleX(-1);
}
.pet-pack-media {
  max-width: 100%;
  max-height: 100%;
  object-fit: contain;
  pointer-events: none;
}

/* spritesheet：以帧尺寸为基准的「取景框」，按容器/帧的最大边缩放装进
   --pet-size 盒。
   ⚠ 换算公式必须 `÷ 1px` 收尾把长度换算成纯数：calc(96px / max(192,200))
   的结果是 0.48px（长度），而 scale() 只接受 <number>——整条 transform 会被
   判无效而整体丢弃（2026-10-24 真浏览器 getBoundingClientRect 实测：盒子以
   自然尺寸 192×200 渲染，只吃父容器 packScale 一层 → 角色溢出命中盒 60%+、
   阴影卡膝盖，雪碧图自上线起该缩放从未生效）。
   packScale 只由父容器 .pet-pack 统一施加（gif/webm 同路径），此处不乘。
   居中用绝对定位几何居中（top/left 50% + translate 负半）：雪碧图自然尺寸
   恒大于容器，flex 超大子项居中行为跨引擎有差异，几何居中对任意尺寸确定；
   pixelated 对齐像素画素材观感。 */
.pet-sprite-box {
  position: absolute;
  top: 50%;
  left: 50%;
  overflow: hidden;
  width: calc(var(--sprite-fw, 64) * 1px);
  height: calc(var(--sprite-fh, 64) * 1px);
  pointer-events: none;
  transform: translate(-50%, -50%) scale(calc(var(--pet-size, 128px) / max(var(--sprite-fw, 64), var(--sprite-fh, 64)) / 1px));
  image-rendering: pixelated;
}
.pet-sprite {
  width: 100%;
  height: 100%;
  background-image: var(--sprite-src);
  background-repeat: no-repeat;
  pointer-events: none;
  /* steps(1,end)：相邻关键帧之间保持起始帧值 → 每段恰好定格一帧；
     点播中一次性播完（--sprite-once:1 + animationend 收尾），idle 无限循环 */
  animation: var(--sprite-anim) calc(var(--sprite-frames, 1) / var(--sprite-fps, 8) * 1s) steps(1, end) var(--sprite-once, infinite) both;
}

/* 图片动画：呼吸效果 */
.anim-breath {
  animation: pet-breath 3s ease-in-out infinite;
}
@keyframes pet-breath {
  0%, 100% { transform: translate(var(--pet-offset-x, 0), var(--pet-offset-y, 0)) scale(var(--pet-scale, 1)); }
  50% { transform: translate(var(--pet-offset-x, 0), var(--pet-offset-y, 0)) scale(calc(var(--pet-scale, 1) * 1.08)); }
}

/* 图片动画：摇摆效果 */
.anim-swing {
  animation: pet-swing 2.5s ease-in-out infinite;
}
@keyframes pet-swing {
  0%, 100% { transform: translate(var(--pet-offset-x, 0), var(--pet-offset-y, 0)) scale(var(--pet-scale, 1)) rotate(0deg); }
  25% { transform: translate(var(--pet-offset-x, 0), var(--pet-offset-y, 0)) scale(var(--pet-scale, 1)) rotate(5deg); }
  75% { transform: translate(var(--pet-offset-x, 0), var(--pet-offset-y, 0)) scale(var(--pet-scale, 1)) rotate(-5deg); }
}

/* 图片动画：旋转效果 */
.anim-rotate {
  animation: pet-rotate 8s linear infinite;
}
@keyframes pet-rotate {
  0% { transform: translate(var(--pet-offset-x, 0), var(--pet-offset-y, 0)) scale(var(--pet-scale, 1)) rotate(0deg); }
  100% { transform: translate(var(--pet-offset-x, 0), var(--pet-offset-y, 0)) scale(var(--pet-scale, 1)) rotate(360deg); }
}

/* 图片动画：弹跳效果 */
.anim-bounce {
  animation: pet-bounce 1.5s ease-in-out infinite;
}
@keyframes pet-bounce {
  0%, 100% { transform: translate(var(--pet-offset-x, 0), calc(var(--pet-offset-y, 0) + 0px)) scale(var(--pet-scale, 1)); }
  50% { transform: translate(var(--pet-offset-x, 0), calc(var(--pet-offset-y, 0) - 6px)) scale(var(--pet-scale, 1)); }
}

/* 状态切换瞬间脉冲一次，给视觉信号。挂 .pet-motion（内容脉冲），不碰
   .pet-lottie/.pet-media 的 transform 链。特异性 (0,3,0) 压过单类 motion 规则，
   脉冲的 0.8s 内临时接管动画；结束（flashing 摘除）后 motion 动画原样恢复。 */
.pet-avatar.flashing .pet-motion {
  animation: pet-flash 0.4s ease-out 0s 2;
}
@keyframes pet-flash {
  0%   { transform: scale(1); }
  50%  { transform: scale(1.1); }
  100% { transform: scale(1); }
}

/* ===== 状态动效 overlay（motion prop 驱动，stateMap.animation 接线）=====
   动画全部限定在小人本体的 .pet-motion 包裹层，与 .pet-lottie/.pet-media
   的 translate+scale 分属不同元素、互相叠加不冲突；状态切换的 flash 脉冲
   也挂这层（见上）。透明窗上不做任何大面积脉动发光——发光环已删
   （大面积脉动光 = 拖拽闪烁源，v0.10.6）。 */
.pet-motion {
  position: absolute;
  inset: 0;
  z-index: 1;
  transform-origin: center center;
  /* 热区纪律：穿透判定靠 elementFromPoint→closest('.pointer-target')，动效的
     transform 会把渲染像素移出命中盒（happy 上跳 -8px、breathe 横向 +1.4px），
     不禁用命中就把框外穿透区算成 UI 区，鼠标掠过小人上方时穿透反复翻转。
     关掉后命中只落在 .pet-dot / .avatar 上，区域恒定不随动画呼吸。 */
  pointer-events: none;
}
.pet-motion-breathe { animation: pet-motion-breathe 3s ease-in-out infinite; }
.pet-motion-think   { animation: pet-motion-think 2.6s ease-in-out infinite; }
.pet-motion-work    { animation: pet-motion-work 0.9s ease-in-out infinite; }
.pet-motion-alert   { animation: pet-motion-alert 0.45s ease-in-out infinite; }
.pet-motion-happy   { animation: pet-motion-happy 0.7s ease-in-out infinite; }

@keyframes pet-motion-breathe {
  0%, 100% { transform: scale(1); }
  50%      { transform: scale(1.04); }
}
@keyframes pet-motion-think {
  0%, 100% { transform: rotate(0deg); }
  25%      { transform: rotate(2deg); }
  75%      { transform: rotate(-2deg); }
}
@keyframes pet-motion-work {
  0%, 100% { transform: translateY(0); }
  50%      { transform: translateY(-3px); }
}
@keyframes pet-motion-alert {
  0%, 100% { transform: rotate(0deg); }
  25%      { transform: rotate(3deg); }
  75%      { transform: rotate(-3deg); }
}
@keyframes pet-motion-happy {
  0%  { transform: translateY(0) scale(1, 1); }
  30% { transform: translateY(-8px) scale(0.96, 1.05); }
  60% { transform: translateY(0) scale(1.06, 0.94); }  /* 落地压扁 */
  80% { transform: translateY(0) scale(0.99, 1.01); }
  100% { transform: translateY(0) scale(1, 1); }
}

/* ===== idle fidget 微动作（一次性，播完由 JS 摘 class）=====
   放在 motion 类之后声明：同时挂两类时 fidget 覆盖 overlay 动画。 */
.pet-fidget-tilt-left {
  animation: pet-fidget-tilt-left 2s ease-in-out both;
}
@keyframes pet-fidget-tilt-left {
  0%, 100% { transform: rotate(0deg); }
  35%, 65% { transform: rotate(-7deg) translateX(-2px); }
}
.pet-fidget-tilt-right {
  animation: pet-fidget-tilt-right 2s ease-in-out both;
}
@keyframes pet-fidget-tilt-right {
  0%, 100% { transform: rotate(0deg); }
  35%, 65% { transform: rotate(7deg) translateX(2px); }
}
.pet-fidget-stretch {
  animation: pet-fidget-stretch 2.2s ease-in-out both;
}
@keyframes pet-fidget-stretch {
  0%, 100% { transform: scale(1, 1); }
  30%, 60% { transform: scale(1.07, 0.95); }
  45%      { transform: scale(1.1, 0.92); }
}

/* ===== 程序化走路（E2 漫游 2026-10-22 开放全宠物）=====
   roamer 滑窗期间（moving 且无 pack clip 在播）挂 .pet-motion-walk 弹跳，模拟
   「走起来」，到站 moving 撤销即归位。声明在 fidget 之后：同时挂时走路压过
   fidget（JS 侧 watch 已收掉在播 fidget，这里是兜底）。挂 .pet-motion 内层
   动画层（穿透铁律：命中盒不动，形变幅度与既有 motion 同量级）；minimal 主题
   (0,2,1) 的 animation:none 自动关掉；pack 真 walk clip 在播时 clip-playing
   (0,3,0) 压掉且 computed 本就不挂——pack 既有行为零变化。 */
.pet-motion-walk {
  animation: pet-motion-walk 0.55s ease-in-out infinite;
}
@keyframes pet-motion-walk {
  0%, 100% { transform: translateY(0) scale(1, 1); }
  35%      { transform: translateY(-6px) scale(0.98, 1.04); }
  70%      { transform: translateY(0) scale(1.05, 0.95); }
}

/* 程序化走路的方向镜像：仅 lottie/图片/GIF（pack 走 .pet-pack-flip 既有链路）。
   scaleX(-1) 追加在素材层 transform 链末尾，--pet-scale/offset 基项原样保留
   （manifest 注入的缩放/偏移不失效）；仅走路期间挂，静止归位。注：图片宠物若
   配了 imageAnimation（keyframes 会接管 transform），镜像在该动画进行中被
   覆盖——接受该边缘降级，不为每个图片动画关键帧逐一套镜像。 */
.pet-motion-flip .pet-lottie,
.pet-motion-flip .pet-media {
  transform: translate(var(--pet-offset-x, 0), var(--pet-offset-y, 0)) scale(var(--pet-scale, 1)) scaleX(-1);
}

/* minimal 主题的设计定义是 no shadow / no motion：动效（overlay + fidget +
   flash 脉冲）在此主题下全关，地面投影一并隐藏。flash 规则是 (0,3,0)，
   这里用 (0,4,1) 的同结构选择器压过它；其余单类规则 (0,2,1) 即可覆盖。 */
html[data-theme="minimal"] .pet-motion {
  animation: none;
}
html[data-theme="minimal"] .pet-avatar.flashing .pet-motion {
  animation: none;
}
html[data-theme="minimal"] .pet-shadow {
  display: none;
}

/* mascot（PetRig）无 CSS 循环动画——全部 WAAPI 由组件读主题在 JS 侧挂起/恢复
   （minimal 门控 JS 化），此处无需再点名其子树；pack 素材动画（.pet-sprite 等）
   照旧走上面的通用门控。 */

/* pack 片段播放期间暂停 motion overlay（PRD E1-4）：pack 片段自带完整动作，
   再叠 breathe/happy 的形变会打架；播完回 idle（clip-playing 摘除）自动恢复。
   选择器结构与 minimal 门控同款，(0,3,0) 压过单类 motion/fidget 规则，
   与 flashing 规则同特异性但声明在后，同时挂时以此为准。 */
.pet-avatar.clip-playing .pet-motion {
  animation: none;
}

/* ===== 召唤登场魔法阵（2026-10-24，《光明使者》式出场）=====
   青色环/符文 + 琥珀五角星 = 品牌双色。纪律：线稿 + 低透明纯色，无大面积
   脉动泛光；svg 根 pointer-events:none，全组随 minimal 门控关停。
   时间轴（总 1.9s）：0 外环描线 → 0.25 五角星描线 → 0.55 符文环/内环就位
   + 整阵 -20°→25° 缓旋 → 0.55 角色升起（回弹）→ 1.3 闪光脉冲 → 1.5 起整阵淡出 */
.pet-summon-circle {
  position: absolute;
  inset: -45%;              /* 阵比角色大一圈，中心对齐 */
  z-index: 0;               /* 阴影之上（同层 DOM 后到）、角色之下 */
  pointer-events: none;
  display: flex;
  align-items: center;
  justify-content: center;
}
.pet-summon-svg {
  width: 100%;
  height: 100%;
  overflow: visible;
  pointer-events: none;
  animation: pet-summon-fadeout 1.9s ease-out both;
}
.pet-summon-rot {
  transform-origin: 60px 60px;
  transform-box: view-box;
  animation: pet-summon-rotate 1.9s ease-in-out both;
}
.pet-summon-ring {
  fill: none;
  stroke: #24C8DB;
  stroke-width: 1.6;
  stroke-dasharray: 1;
  stroke-dashoffset: 1;
  animation: pet-summon-draw 0.45s ease-out both;
}
.pet-summon-star {
  fill: none;
  stroke: #FFC131;
  stroke-width: 1.6;
  stroke-linejoin: round;
  stroke-dasharray: 1;
  stroke-dashoffset: 1;
  animation: pet-summon-draw 0.5s ease-in-out 0.25s both;
}
.pet-summon-runes {
  fill: none;
  stroke: #24C8DB;
  stroke-width: 1;
  stroke-dasharray: 0.02 0.03;   /* pathLength=1 下的符文刻度虚线 */
  opacity: 0;
  animation: pet-summon-runes-in 0.35s ease-out 0.55s both;
}
.pet-summon-inner {
  fill: none;
  stroke: #24C8DB;
  stroke-width: 1;
  stroke-dasharray: 1;
  stroke-dashoffset: 1;
  animation: pet-summon-draw 0.3s ease-out 0.6s both;
}
.pet-summon-flash {
  fill: #7fd0e6;
  opacity: 0;
  transform-origin: 60px 60px;
  transform-box: view-box;
  animation: pet-summon-flash 1.9s ease-out both;
}
@keyframes pet-summon-draw { to { stroke-dashoffset: 0; } }
@keyframes pet-summon-runes-in { to { opacity: 0.75; } }
@keyframes pet-summon-rotate { from { transform: rotate(-20deg); } to { transform: rotate(25deg); } }
@keyframes pet-summon-flash {
  0%, 68% { opacity: 0; transform: scale(0.6); }
  78% { opacity: 0.28; transform: scale(1.15); }
  92% { opacity: 0.1; transform: scale(1.28); }
  100% { opacity: 0; transform: scale(1.32); }
}
@keyframes pet-summon-fadeout {
  0%, 80% { opacity: 1; }
  100% { opacity: 0; }
}

/* 升起包裹层：召唤期角色从阵中升起——transform 只挂本层（.pet-motion 的
   动画链在子元素，零冲突）；升起 blur→清晰制造「从阵中凝聚成形」 */
.pet-rise {
  position: absolute;
  inset: 0;
  z-index: 1;
  pointer-events: none;
}
.pet-rise-active {
  animation: pet-summon-rise 0.75s cubic-bezier(0.2, 1.3, 0.4, 1) 0.45s both;
}
@keyframes pet-summon-rise {
  0% { transform: translateY(15%) scale(0.72); opacity: 0; filter: blur(2px); }
  30% { opacity: 1; }
  65% { filter: blur(0); }
  100% { transform: translateY(0) scale(1); opacity: 1; filter: blur(0); }
}

/* minimal（no motion）：魔法阵整组不渲染，升起动画关停（角色直接现形） */
html[data-theme="minimal"] .pet-summon-circle { display: none; }
html[data-theme="minimal"] .pet-rise-active { animation: none; }

</style>
