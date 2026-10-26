// ============================================================================
// Rig 动画引擎：轨道数据 → WAAPI（Element.animate），双层调度。
//
// 双层语义（对齐旧 PetMascot 的「常驻底层 + 姿态换挡」）：
// - base 层：idle clip 的常驻环境动画（悬浮呼吸/天线晃/指示灯…）；
// - pose 层：当前姿态 clip 的轨道。姿态只覆盖其 tracks 声明的部件，未声明
//   部件的 base 动画不受扰动（不重启、无相位跳变）；姿态结束，被覆盖部件
//   从 baseClip 数据重建动画回 idle。
//
// minimal 主题门控在组件层做（WAAPI 动画不受 CSS animation:none 影响）：
// cancelAll() 定格基础姿态，恢复时 playBase + applyPose 全量重启。
// ============================================================================

import type { RigClip, RigEasing, RigTrack } from './types'

const DEFAULT_EASING: RigEasing = 'ease-in-out'

type Frame = Record<string, string | number>

/** 单条轨道 → WAAPI keyframes。末帧 offset 强制 1；缓动挂每帧（区间生效）。 */
export function trackToKeyframes(track: RigTrack): Frame[] {
  const kfs = track.keyframes
  if (kfs.length === 0) return []
  return kfs.map((kf, i) => {
    const frame: Frame = { offset: i === kfs.length - 1 ? 1 : Math.min(1, Math.max(0, kf.t)) }
    const tf: string[] = []
    if (kf.x !== undefined || kf.y !== undefined) {
      tf.push(`translate(${kf.x ?? 0}px, ${kf.y ?? 0}px)`)
    }
    if (kf.rotate !== undefined) tf.push(`rotate(${kf.rotate}deg)`)
    if (kf.rotateY !== undefined) tf.push(`rotateY(${kf.rotateY}deg)`)
    if (kf.scaleX !== undefined || kf.scaleY !== undefined) {
      tf.push(`scale(${kf.scaleX ?? 1}, ${kf.scaleY ?? 1})`)
    }
    if (tf.length > 0) frame.transform = tf.join(' ')
    if (kf.opacity !== undefined) frame.opacity = kf.opacity
    if (kf.fill !== undefined) frame.fill = kf.fill
    frame.easing = kf.easing ?? DEFAULT_EASING
    return frame
  })
}

function playTrack(el: Element, track: RigTrack, clipMs: number, loop: boolean): Animation {
  return el.animate(trackToKeyframes(track), {
    duration: Math.max(1, track.durationMs ?? clipMs),
    delay: track.delayMs ?? 0,
    // 一次性片段 forwards 定格末帧，等组件收尾换层时再 cancel，避免回跳闪烁
    fill: loop ? 'none' : 'forwards',
    iterations: loop ? Infinity : 1,
  })
}

/** 一个 rig 实例的动画控制器：部件 id → 当前生效动画，双层差量换挡 */
export class RigAnimator {
  private baseClip: RigClip | null = null
  private baseAnims = new Map<string, Animation>()
  private poseAnims = new Map<string, Animation>()
  private burstAnims: Animation[] = []

  constructor(private readonly els: Map<string, Element>) {}

  /**
   * 切姿态（差量换挡）：上一姿态覆盖过、新姿态不再覆盖的部件回 base 层；
   * 两姿态都覆盖的部件只换动画不打断 base 重建。返回 Promise：一次性 clip
   * 在全部轨道播完时 resolve（组件借此收尾回 idle）；循环 clip 立即 resolve
   *（无「播完」语义，收尾靠外部 stopClip/换姿态）。取消/换姿态后已 resolve
   * 的旧 Promise 由组件用序号守卫丢弃。
   */
  applyPose(clip: RigClip | null): Promise<void> {
    const prevParts = new Set(this.poseAnims.keys())
    for (const anim of this.poseAnims.values()) anim.cancel()
    this.poseAnims.clear()
    const nextParts = new Set(clip ? Object.keys(clip.tracks) : [])
    for (const id of prevParts) {
      if (!nextParts.has(id)) this.restoreBase(id)
    }
    if (!clip) return Promise.resolve()
    const pendings: Promise<unknown>[] = []
    for (const [id, track] of Object.entries(clip.tracks)) {
      const el = this.els.get(id)
      if (!el) continue
      this.baseAnims.get(id)?.cancel()
      this.baseAnims.delete(id)
      const anim = playTrack(el, track, clip.durationMs, clip.loop)
      this.poseAnims.set(id, anim)
      if (!clip.loop) pendings.push(anim.finished.catch(() => undefined))
    }
    if (clip.loop || pendings.length === 0) return Promise.resolve()
    return Promise.all(pendings).then(() => undefined)
  }

  /** （重）启 idle 常驻层 = 全量重启：base 全部重建，pose 层作废。 */
  playBase(clip: RigClip): void {
    for (const anim of this.baseAnims.values()) anim.cancel()
    this.baseAnims.clear()
    for (const anim of this.poseAnims.values()) anim.cancel()
    this.poseAnims.clear()
    this.baseClip = clip
    for (const [id, track] of Object.entries(clip.tracks)) {
      const el = this.els.get(id)
      if (!el) continue
      this.baseAnims.set(id, playTrack(el, track, clip.durationMs, clip.loop))
    }
  }

  /**
   * 一次性叠加片段（眨眼）：不参与双层换挡，创建时间最晚 = WAAPI 优先级最高，
   * 压过该部件上在播的 base/pose 动画；播完自清 forwards 定格（cancel 后露出
   * 下层仍在跑的动画）。
   */
  playBurst(clip: RigClip): void {
    this.clearBurst()
    for (const [id, track] of Object.entries(clip.tracks)) {
      const el = this.els.get(id)
      if (!el) continue
      const anim = playTrack(el, track, clip.durationMs, false)
      this.burstAnims.push(anim)
      void anim.finished
        .catch(() => undefined)
        .then(() => {
          const i = this.burstAnims.indexOf(anim)
          if (i >= 0) this.burstAnims.splice(i, 1)
          anim.cancel()
        })
    }
  }

  private clearBurst(): void {
    for (const anim of this.burstAnims) anim.cancel()
    this.burstAnims = []
  }

  /** minimal 门控：全部动画取消，元素定格静态姿态（show/hide 样式归组件管） */
  cancelAll(): void {
    for (const anim of this.baseAnims.values()) anim.cancel()
    this.baseAnims.clear()
    for (const anim of this.poseAnims.values()) anim.cancel()
    this.poseAnims.clear()
    this.clearBurst()
  }

  /** 姿态结束/被覆盖后，从 baseClip 数据重建该部件的 idle 动画 */
  private restoreBase(id: string): void {
    const base = this.baseClip
    const track = base?.tracks[id]
    const el = this.els.get(id)
    if (!base || !track || !el) return
    this.baseAnims.set(id, playTrack(el, track, base.durationMs, base.loop))
  }
}
