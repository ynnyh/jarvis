<script setup lang="ts">
import { computed, h, onBeforeUnmount, onMounted, watch, type FunctionalComponent, type VNode } from 'vue'
import { useConfigStore } from '../stores/config'
import { getStyleTheme } from '../style-themes'
import { RigAnimator } from '../petRig/engine'
import { parseRigDoc, RIG_COLOR_PLACEHOLDER, type RigClip, type RigDoc, type RigPart } from '../petRig/types'

// ============================================================================
// Rig 通用渲染器：读 RigDoc（部件树 + 动作表）渲染 SVG 并驱动动画。
// 任意「数据定义的程序化角色」都走本组件（管家是第一个，P2 起新角色只加数据）。
//
// 单一职责：只管「画什么、怎么动」。片段调度（playClip/hasClip/一次性时长收尾）
// 仍在 PetAvatar 的 mascot 映射层，漫游引擎经 packHasClip 零改动复用 clip 链路。
//
// 结构（渲染树由 doc.parts 的 parent 引用嵌套而成，z 决定同层绘制顺序）：
//   翻转层(flipPart) → 全身层 → 推进器/身体/领结/手臂/头(眼睛)/天线；zzz 挂根。
//
// 动画（WAAPI，petRig/engine.ts 双层调度）：
// - base 层：idle clip 常驻环境动画；姿态 clip 只覆盖其 tracks 声明的部件
//   （work 时身体继续悬浮呼吸），姿态结束被覆盖部件回播 idle；
// - 一次性动作 clip 播完（finished promise）自动回 idle；
// - 眨眼 = doc.clips.blink 经 playBurst 叠加（JS 3-6s 随机，sleep 跳过）；
// - show/hide：clip 激活期的静态显隐（眼形变体/zzz 组），内联 style 覆盖，
//   结束恢复部件静态 opacity。
//
// 红线对齐：
// - 穿透铁律：svg 根 pointer-events:none 且零 transform/零动画（spin 的 rotateY
//   挂 flipPart 部件，不碰 svg 根），命中盒不随任何形变；
// - minimal 门控 JS 化（WAAPI 不受 CSS animation:none 影响）：isMinimal 时
//   cancelAll 定格静态姿态，切出重启；静态姿态样式（sleep 变暗滤镜、眼形/zzz
//   显隐）保持生效——滤镜按 theming spec 例外口径保留；
// - 定时器卫生：眨眼定时器 onBeforeUnmount 必清。
// ============================================================================

const props = defineProps<{
  /** rig 文档（petManifest 已 parseRigDoc 校验过，这里再防御性 parse 一次） */
  doc: RigDoc
  /** 状态色（stateMap.color）：'$color' 占位符替换用 */
  color: string
  /** stateMap.animation：breathe/think/work/alert/happy */
  motion: string
  /** 漫游移动中（兜底走路信号；正常路径由 roamer 播的 clip='walk' 驱动） */
  moving: boolean
  /** 拖拽拎起中——最高优先级姿态 held */
  dragging: boolean
  /** PetAvatar 当前片段名（null = 常态；sleep/walk/点播动作名） */
  clip: string | null
  /** 行进朝向（roamer setFacing）；与 doc.facing 相反时 flipPart 镜像 */
  facing: 'left' | 'right'
}>()

// 防御性再校验：坏数据响亮失败（内置文档在 petManifest 启动即 parse，此处兜 P2 动态文档）
const doc = parseRigDoc(props.doc)

const actionByName = new Map(doc.actions.map(a => [a.name, a]))
/** 需要角色化动画的 stateMap motion 值（idle/breathe/morning 等都落 idle 常驻层） */
const MOTION_POSES = new Set(['think', 'work', 'alert', 'happy'])

/** 姿态优先级（对齐旧 PetMascot）：held > sleep > walk > 点播动作 > motion > idle */
const pose = computed<string>(() => {
  if (props.dragging) return 'held'
  if (props.clip === 'sleep') return 'sleep'
  if (props.clip === 'walk' || props.moving) return 'walk'
  if (props.clip && actionByName.has(props.clip)) return actionByName.get(props.clip)!.clip
  if (MOTION_POSES.has(props.motion)) return props.motion
  return 'idle'
})

/** svg 根的静态姿态类（目前只有 sleep 变暗滤镜；眼形/zzz 显隐走内联 style） */
const rootClass = computed(() => ({ 'pose-sleep': pose.value === 'sleep' }))

/** 素材朝向与当前朝向相反 → flipPart 镜像（spin 的 rotateY 动画会临时覆盖它） */
const flipped = computed(() => props.facing !== doc.facing)

// ===== 渲染树 =====

interface RigNode { part: RigPart; children: RigNode[] }

const tree = computed<RigNode[]>(() => {
  const nodes = new Map<string, RigNode>(doc.parts.map(p => [p.id, { part: p, children: [] }]))
  const roots: RigNode[] = []
  for (const node of nodes.values()) {
    const parent = node.part.parent ? nodes.get(node.part.parent) : undefined
    ;(parent ? parent.children : roots).push(node)
  }
  const sortNodes = (list: RigNode[]): void => {
    list.sort((a, b) => (a.part.z ?? 0) - (b.part.z ?? 0))
    list.forEach(n => sortNodes(n.children))
  }
  sortNodes(roots)
  return roots
})

// 部件元素注册表（动画驱动按 id 取节点）；函数 ref 挂载时注册、卸载时清除
const els = new Map<string, Element>()

function registerPart(id: string, el: unknown): void {
  if (el instanceof Element) els.set(id, el)
  else els.delete(id)
}

/** 部件 → SVG 属性（'$color' 占位符在此解析；SVG 属性用 kebab-case 直传 setAttribute） */
function svgAttrs(p: RigPart): Record<string, string | number> {
  const resolve = (v: string): string => (v === RIG_COLOR_PLACEHOLDER ? props.color : v)
  const attrs: Record<string, string | number> = {}
  if (p.x !== undefined) attrs.x = p.x
  if (p.y !== undefined) attrs.y = p.y
  if (p.width !== undefined) attrs.width = p.width
  if (p.height !== undefined) attrs.height = p.height
  if (p.rx !== undefined) attrs.rx = p.rx
  if (p.ry !== undefined) attrs.ry = p.ry
  if (p.cx !== undefined) attrs.cx = p.cx
  if (p.cy !== undefined) attrs.cy = p.cy
  if (p.r !== undefined) attrs.r = p.r
  if (p.d !== undefined) attrs.d = p.d
  if (p.fontSize !== undefined) attrs['font-size'] = p.fontSize
  if (p.fill !== undefined) attrs.fill = resolve(p.fill)
  if (p.fillOpacity !== undefined) attrs['fill-opacity'] = p.fillOpacity
  if (p.stroke !== undefined) attrs.stroke = resolve(p.stroke)
  if (p.strokeOpacity !== undefined) attrs['stroke-opacity'] = p.strokeOpacity
  if (p.strokeWidth !== undefined) attrs['stroke-width'] = p.strokeWidth
  if (p.strokeLinecap !== undefined) attrs['stroke-linecap'] = p.strokeLinecap
  if (p.opacity !== undefined) attrs.opacity = p.opacity
  if (p.id === doc.flipPart && flipped.value) attrs.class = 'pet-rig-flipped'
  return attrs
}

/** 递归渲染部件子树；transform-origin 按 pivot（SVG 默认 transform-box: view-box） */
function renderNode(node: RigNode): VNode {
  const p = node.part
  const tag = p.kind === 'group' ? 'g' : p.kind
  if (p.kind === 'image') {
    return h('g') // P2/P3 PNG 切片部件：P1 未支持，parseRigDoc 已报过警
  }
  return h(
    tag,
    {
      ...svgAttrs(p),
      style: { transformOrigin: `${p.pivot[0]}px ${p.pivot[1]}px` },
      ref: (el: unknown) => registerPart(p.id, el),
    },
    p.kind === 'text' ? p.text ?? '' : node.children.map(renderNode),
  )
}

const PartNode: FunctionalComponent<{ node: RigNode }> = nodeProps => renderNode(nodeProps.node)
PartNode.props = ['node']

// ===== 动画驱动 =====

let animator: RigAnimator | null = null

// —— minimal 门控（JS 化）：WAAPI 动画不受 CSS animation:none 影响 ——
const store = useConfigStore()
const isMinimal = computed(() => getStyleTheme(store.config.styleTheme).id === 'minimal')

// —— show/hide 静态显隐：内联 style 覆盖部件 opacity，姿态结束恢复属性值 ——
let overriddenParts: string[] = []

function clearOverrides(): void {
  for (const id of overriddenParts) els.get(id)?.style.removeProperty('opacity')
  overriddenParts = []
}

function applyOverrides(clip: RigClip | null): void {
  clearOverrides()
  if (!clip) return
  for (const id of clip.show ?? []) {
    els.get(id)?.style.setProperty('opacity', '1')
    overriddenParts.push(id)
  }
  for (const id of clip.hide ?? []) {
    els.get(id)?.style.setProperty('opacity', '0')
    overriddenParts.push(id)
  }
}

// 一次性姿态的收尾守卫：姿态切换/主题切换使旧 Promise 的回 idle 作废
let poseSeq = 0

/** 应用当前姿态：show/hide 总是生效；动画层在 minimal 下挂起 */
function activatePose(): void {
  const seq = ++poseSeq
  const clipId = pose.value
  const clip = clipId === 'idle' ? null : doc.clips[clipId] ?? null
  applyOverrides(clip)
  if (!animator || isMinimal.value) return
  const pending = animator.applyPose(clip)
  // 只有一次性行动有「播完收尾」：循环片段（walk/sleep/held）立即 resolve，
  // 挂回调会当场把自己打回 idle；循环片段靠外部 stopClip 换姿态收尾
  if (clip && !clip.loop) {
    void pending.then(() => {
      if (seq !== poseSeq) return
      applyOverrides(null)
      animator?.applyPose(null)
    })
  }
}

/** 全量重启动画（挂载 / 切出 minimal）：重建 base 层再套当前姿态 */
function restartMotion(): void {
  if (!animator) return
  if (isMinimal.value) {
    animator.cancelAll()
    return
  }
  animator.playBase(doc.clips.idle)
  activatePose()
}

watch(pose, () => activatePose())
watch(isMinimal, () => restartMotion())

// ===== 眨眼（3-6s 随机，doc.clips.blink 存在才启用；sleep 态跳过）=====
let blinkTimer: number | null = null

function scheduleBlink(): void {
  blinkTimer = window.setTimeout(() => {
    blinkTimer = null
    if (!isMinimal.value && pose.value !== 'sleep') animator?.playBurst(doc.clips.blink!)
    scheduleBlink()
  }, 3000 + Math.random() * 3000)
}

onMounted(() => {
  animator = new RigAnimator(els)
  restartMotion()
  if (doc.clips.blink) scheduleBlink()
})

onBeforeUnmount(() => {
  if (blinkTimer !== null) {
    clearTimeout(blinkTimer)
    blinkTimer = null
  }
  animator?.cancelAll()
  animator = null
})
</script>

<template>
  <!-- svg 根：零 transform 零动画（穿透铁律），姿态滤镜类只做静态变暗 -->
  <svg
    class="pet-rig"
    :class="rootClass"
    :viewBox="`0 0 ${doc.viewBox} ${doc.viewBox}`"
    xmlns="http://www.w3.org/2000/svg"
    aria-hidden="true"
  >
    <PartNode v-for="node in tree" :key="node.part.id" :node="node" />
  </svg>
</template>

<style scoped>
/* 布局与旧 .pet-mascot 同口径：容器内相对尺寸，矢量随 --pet-size 缩放。
   主体配色是角色资产色（数据文件内字面值，不随主题），状态色经 '$color'
   由数据声明位置注入。 */
.pet-rig {
  position: absolute;
  inset: 4px;
  width: calc(100% - 8px);
  height: calc(100% - 8px);
  display: block;
  overflow: visible; /* zzz 上飘 / happy 蹦跳允许略出 viewBox（命中盒不受影响） */
  pointer-events: none;
  /* 只给 spin 的 rotateY 一点纵深；2D 姿态不受影响 */
  perspective: 320px;
}

/* 朝向镜像挂 flipPart 部件（素材层），svg 根零 transform。
   spin 动画（WAAPI）播放期按 WAAPI 优先级临时覆盖本静态 transform。 */
.pet-rig-flipped {
  transform: scaleX(-1);
}

/* sleep 变暗：滤镜非动画，minimal 主题按 theming spec 例外口径保留 */
.pet-rig.pose-sleep {
  filter: brightness(0.72) saturate(0.7);
}

/* 文本部件（zzz 气泡）字体：emoji 字体渲染不可控，固定显示字体 */
.pet-rig text {
  font-family: var(--font-display, 'Segoe UI', monospace);
  font-weight: 700;
}
</style>
