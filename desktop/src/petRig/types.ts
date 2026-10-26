// ============================================================================
// Rig 数据格式：数据驱动 2D 骨骼动画（角色 = 一份文档：部件树 + 动作表）。
//
// 设计参照 Blender/Three.js 的骨架 + 关键帧轨道 + 动作重定向概念，运行时用
// SVG + WAAPI 实现（components/PetRig.vue 渲染，petRig/engine.ts 播放）。
// 角色是数据不是组件：换形状/配色 = 新角色；动作是数据：同拓扑角色共享动作库。
//
// 坐标与单位约定：
// - 所有部件坐标与 pivot 都在 viewBox 用户坐标系里（px = 用户单位）；
// - transform-origin 按 pivot 绝对坐标设置（SVG 默认 transform-box: view-box）；
// - keyframe.t 为 0..1 归一化进度；track 可带自己的 durationMs（idle 各部件
//   环境动画周期不同），缺省用 clip.durationMs。
//
// 状态色：fill/stroke 写 '$color' 占位符，渲染时替换为 stateMap 状态色
// （对齐旧 PetMascot 的 --m-color 继承口径）。
//
// P1 限制：image 部件类型仅 schema 占位（P2/P3 PNG 切片立绘用），运行时跳过。
// ============================================================================

/** 状态色占位符：fill/stroke 值为 '$color' 时替换为当前状态色 */
export const RIG_COLOR_PLACEHOLDER = '$color'

export type RigEasing = 'linear' | 'ease' | 'ease-in' | 'ease-out' | 'ease-in-out'

/** 单个关键帧。transform 合成顺序固定：translate → rotate → rotateY → scale。 */
export interface RigKeyframe {
  /** 归一化时间点 0..1（末帧渲染时强制 offset=1） */
  t: number
  x?: number
  y?: number
  /** 2D 旋转（度） */
  rotate?: number
  /** 3D Y 轴旋转（度，转圈动作用） */
  rotateY?: number
  scaleX?: number
  scaleY?: number
  opacity?: number
  /** 覆盖填充色（alert 红眼用；支持 '$color' 占位符） */
  fill?: string
  /** 进入本帧的缓动（作用于上一帧→本帧区间），缺省 ease-in-out */
  easing?: RigEasing
}

/** 一条部件动画轨道：同一部件的一组关键帧 */
export interface RigTrack {
  keyframes: RigKeyframe[]
  /** 本轨道自用周期毫秒（缺省用 clip.durationMs） */
  durationMs?: number
  /** 起播延迟毫秒（zzz 三段错峰用），缺省 0 */
  delayMs?: number
}

/** 一个动作片段：姿态层播的内容（idle/walk 常驻循环，点播动作一次性） */
export interface RigClip {
  durationMs: number
  /** 循环片段（idle/walk/held/sleep）true；一次性动作 false（finished 收尾回 idle） */
  loop: boolean
  /** 部件 id → 轨道。姿态 clip 只覆盖声明的部件，其余部件继续播 idle（换挡语义） */
  tracks: Record<string, RigTrack>
  /** clip 激活期间显示的部件（眼形变体/zzz 组），结束时恢复静态 opacity */
  show?: string[]
  /** clip 激活期间隐藏的部件 */
  hide?: string[]
}

/** 部件图元种类。group 是纯 transform 节点；image 为 P2/P3 PNG 切片预留（P1 跳过） */
export type RigPartKind = 'group' | 'rect' | 'ellipse' | 'circle' | 'path' | 'text' | 'image'

/** 部件：一个可独立动画的节点（transform-origin 由 pivot 决定） */
export interface RigPart {
  id: string
  kind: RigPartKind
  /** 父部件 id，缺省挂 svg 根（zzz 在翻转层外 = 镜像时不反写，就是这个用途） */
  parent?: string
  /** transform-origin（viewBox 坐标） */
  pivot: [number, number]
  /** 绘制顺序（小者先画），缺省 0；同序按声明顺序 */
  z?: number
  // —— 图元几何（按 kind 取用，渲染时透传为 SVG 属性）——
  x?: number
  y?: number
  width?: number
  height?: number
  /** rect 圆角半径（circle/ellipse 是半径向 rx） */
  rx?: number
  cx?: number
  cy?: number
  r?: number
  ry?: number
  /** path 的 d 属性 */
  d?: string
  /** text 内容 */
  text?: string
  fontSize?: number
  // —— 静态样式 ——
  fill?: string
  fillOpacity?: number
  stroke?: string
  strokeOpacity?: number
  strokeWidth?: number
  strokeLinecap?: 'round' | 'butt' | 'square'
  /** 静态基础透明度（glint 0.85 / 眼形变体与 zzz 组 0 = 默认隐藏，靠 clip.show 点亮） */
  opacity?: number
  // —— P2/P3 预留：PNG 切片部件 ——
  href?: string
}

/** 点播动作条目：name 进右键菜单与 hasClip，key 是稳定 ascii 标识 */
export interface RigActionEntry {
  name: string
  key: string
  /** 对应 clips 里的 clip id（一次性，播完回 idle） */
  clip: string
}

/** 角色 rig 文档：部件树 + 动作表 + 点播清单 */
export interface RigDoc {
  id: string
  /** 正方形 viewBox 边长 */
  viewBox: number
  /** 素材默认朝向（镜像翻转的基准；jarvis-butler 为 'right'） */
  facing: 'left' | 'right'
  /** 朝向镜像（scaleX(-1)）挂在这个部件上；不填则镜像时整体不翻转。
   *  只该挂一个部件：zzz 之类不能被镜像的内容放本部件之外。 */
  flipPart?: string
  parts: RigPart[]
  clips: Record<string, RigClip>
  /** 点播动作清单（右键「来一个」菜单 + hasClip 口径） */
  actions: RigActionEntry[]
}

// ============================================================================
// 运行时校验：文档是打包内置数据（构建期就定性），这里只挡低级脏数据——
// 结构错误直接 throw（响亮失败优于静默白屏）；引用悬空/环路 console.error。
// ============================================================================

const PART_KINDS = new Set(['group', 'rect', 'ellipse', 'circle', 'path', 'text', 'image'])

function fail(msg: string): never {
  throw new Error(`[petRig] rig 文档不合法: ${msg}`)
}

export function parseRigDoc(data: unknown): RigDoc {
  const doc = data as RigDoc
  if (!doc || typeof doc !== 'object') fail('根节点不是对象')
  if (typeof doc.id !== 'string' || doc.id === '') fail('id 缺失')
  if (typeof doc.viewBox !== 'number' || doc.viewBox <= 0) fail('viewBox 非法')
  if (doc.facing !== 'left' && doc.facing !== 'right') fail(`facing 非法: ${String(doc.facing)}`)
  if (!Array.isArray(doc.parts) || doc.parts.length === 0) fail('parts 为空')
  if (!doc.clips || typeof doc.clips !== 'object') fail('clips 缺失')
  if (!doc.clips.idle) fail('缺 idle clip（常驻底层必填）')
  if (!Array.isArray(doc.actions)) fail('actions 缺失')

  const partIds = new Set<string>()
  for (const part of doc.parts) {
    if (!part || typeof part.id !== 'string') fail('存在无 id 部件')
    if (partIds.has(part.id)) fail(`部件 id 重复: ${part.id}`)
    if (!PART_KINDS.has(part.kind)) fail(`部件 ${part.id} kind 非法: ${String(part.kind)}`)
    partIds.add(part.id)
  }
  // 父引用存在性 + 环路检测（沿 parent 链上行，重复访问即环）
  for (const part of doc.parts) {
    const seen = new Set<string>([part.id])
    let cur: RigPart | undefined = part
    while (cur?.parent) {
      if (seen.has(cur.parent)) fail(`部件 ${part.id} 的父链成环: ${cur.parent}`)
      seen.add(cur.parent)
      cur = doc.parts.find(p => p.id === cur!.parent)
      if (!cur) fail(`部件 ${part.id} 引用了不存在的父部件`)
    }
  }
  for (const [clipId, clip] of Object.entries(doc.clips)) {
    for (const trackPart of Object.keys(clip.tracks ?? {})) {
      if (!partIds.has(trackPart)) fail(`clip ${clipId} 轨道指向不存在的部件: ${trackPart}`)
    }
    for (const id of [...(clip.show ?? []), ...(clip.hide ?? [])]) {
      if (!partIds.has(id)) fail(`clip ${clipId} show/hide 指向不存在的部件: ${id}`)
    }
  }
  if (doc.flipPart !== undefined && !partIds.has(doc.flipPart)) {
    fail(`flipPart 指向不存在的部件: ${doc.flipPart}`)
  }
  for (const action of doc.actions) {
    const clip = doc.clips[action.clip]
    if (!clip) fail(`点播动作 ${action.name} 指向不存在的 clip: ${action.clip}`)
    else if (clip.loop) console.error(`[petRig] 点播动作 ${action.name} 指向了 loop clip: ${action.clip}`)
  }
  return doc
}
