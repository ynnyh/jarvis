import { ref, type Ref } from 'vue'
import { getCurrentWindow, LogicalPosition, currentMonitor } from '@tauri-apps/api/window'

/** 获取当前窗口所在屏幕的逻辑像素全局边界（支持多屏幕） */
async function getMonitorBounds(): Promise<{ x: number; y: number; w: number; h: number }> {
  const mon = await currentMonitor()
  if (!mon) return { x: 0, y: 0, w: window.screen.width, h: window.screen.height }
  return {
    x: mon.position.x / mon.scaleFactor,
    y: mon.position.y / mon.scaleFactor,
    w: mon.size.width / mon.scaleFactor,
    h: mon.size.height / mon.scaleFactor,
  }
}

// ===== Constants =====
export type AvatarAnchor = 'rb' | 'rt' | 'lb' | 'lt'

export type DockEdge = 'top' | 'right' | 'bottom' | 'left'

const WINDOW_LOGICAL_W = 400
const WINDOW_LOGICAL_H = 560
const AVATAR_HALF = 36
const AVATAR_MARGIN = 10
export const ANCHOR_AVATAR_CENTER: Record<AvatarAnchor, { x: number; y: number }> = {
  rb: { x: WINDOW_LOGICAL_W - AVATAR_MARGIN - AVATAR_HALF, y: WINDOW_LOGICAL_H - AVATAR_MARGIN - AVATAR_HALF },
  rt: { x: WINDOW_LOGICAL_W - AVATAR_MARGIN - AVATAR_HALF, y: AVATAR_MARGIN + AVATAR_HALF },
  lb: { x: AVATAR_MARGIN + AVATAR_HALF, y: WINDOW_LOGICAL_H - AVATAR_MARGIN - AVATAR_HALF },
  lt: { x: AVATAR_MARGIN + AVATAR_HALF, y: AVATAR_MARGIN + AVATAR_HALF },
}

const DOCK_AUTO_THRESHOLD = 30
const DOCK_SHOW_PX = 18
const DOCK_PEEK_PX = AVATAR_HALF + DOCK_SHOW_PX
const DOCK_RECOIL_MS = 5000
const DOCK_ANIM_MS = 200

export interface UseAvatarDockOptions {
  avatarAnchor: Ref<AvatarAnchor>
  /** Called when a menu or external flow wants to close all panels */
  closeAllPanels: () => void
  showMenu: Ref<boolean>
}

export function useAvatarDock(options: UseAvatarDockOptions) {
  const { avatarAnchor, showMenu } = options

  // ===== State =====
  const dockEdge = ref<DockEdge | null>(null)
  const isPoked = ref(false)
  let dockUndockTimer: number | null = null
  let dockAnimFrame: number | null = null
  let dockedWinPos: { x: number; y: number; anchor: AvatarAnchor } | null = null
  let undockedWinPos: { x: number; y: number; anchor: AvatarAnchor } | null = null

  // ===== Animation =====

  function cancelDockAnim() {
    if (dockAnimFrame !== null) {
      cancelAnimationFrame(dockAnimFrame)
      dockAnimFrame = null
    }
  }

  /** RAF ease-out 缓动窗口到目标 logical 位置。重入会先 cancel 上一帧。 */
  async function animateWindowToLogical(targetX: number, targetY: number, durationMs: number): Promise<void> {
    cancelDockAnim()
    const win = getCurrentWindow()
    let fromX = 0, fromY = 0
    try {
      const pos = await win.outerPosition()
      const scale = await win.scaleFactor()
      fromX = pos.x / scale
      fromY = pos.y / scale
    } catch {
      return
    }
    await new Promise<void>((resolve) => {
      const startT = performance.now()
      function step(now: number) {
        const elapsed = now - startT
        const t = Math.min(1, elapsed / durationMs)
        const e = t * (2 - t)  // easeOutQuad
        const x = fromX + (targetX - fromX) * e
        const y = fromY + (targetY - fromY) * e
        win.setPosition(new LogicalPosition(Math.round(x), Math.round(y))).catch(() => {})
        if (t < 1) {
          dockAnimFrame = requestAnimationFrame(step)
        } else {
          dockAnimFrame = null
          resolve()
        }
      }
      dockAnimFrame = requestAnimationFrame(step)
    })
  }

  // ===== Dock geometry =====

  function computeDockTarget(
    edge: DockEdge,
    avatarScreenX: number, avatarScreenY: number,
    mon: { x: number; y: number; w: number; h: number },
  ): { winX: number; winY: number; newAnchor: AvatarAnchor } {
    let newAnchor: AvatarAnchor
    let targetCenterX = avatarScreenX
    let targetCenterY = avatarScreenY
    const relX = avatarScreenX - mon.x
    const relY = avatarScreenY - mon.y
    if (edge === 'right') {
      newAnchor = relY >= mon.h / 2 ? 'rb' : 'rt'
      targetCenterX = mon.x + mon.w - DOCK_SHOW_PX + AVATAR_HALF
    } else if (edge === 'left') {
      newAnchor = relY >= mon.h / 2 ? 'lb' : 'lt'
      targetCenterX = mon.x + DOCK_SHOW_PX - AVATAR_HALF
    } else if (edge === 'top') {
      newAnchor = relX >= mon.w / 2 ? 'rt' : 'lt'
      targetCenterY = mon.y + DOCK_SHOW_PX - AVATAR_HALF
    } else {
      newAnchor = relX >= mon.w / 2 ? 'rb' : 'lb'
      targetCenterY = mon.y + mon.h - DOCK_SHOW_PX + AVATAR_HALF
    }
    const offset = ANCHOR_AVATAR_CENTER[newAnchor]
    return { winX: targetCenterX - offset.x, winY: targetCenterY - offset.y, newAnchor }
  }

  async function currentAvatarScreenCenter(): Promise<{ x: number; y: number } | null> {
    const win = getCurrentWindow()
    try {
      const pos = await win.outerPosition()
      const scale = await win.scaleFactor()
      const off = ANCHOR_AVATAR_CENTER[avatarAnchor.value]
      return { x: pos.x / scale + off.x, y: pos.y / scale + off.y }
    } catch { return null }
  }

  // ===== Dock actions =====

  /** 拖拽结束触发：avatar 离屏幕某边 < 阈值就 dock 到那边 */
  async function maybeAutoDock() {
    if (dockEdge.value) return
    const c = await currentAvatarScreenCenter()
    if (!c) return
    const mon = await getMonitorBounds()
    const dTop = c.y - mon.y - AVATAR_HALF
    const dBottom = mon.y + mon.h - (c.y + AVATAR_HALF)
    const dLeft = c.x - mon.x - AVATAR_HALF
    const dRight = mon.x + mon.w - (c.x + AVATAR_HALF)
    const min = Math.min(dTop, dBottom, dLeft, dRight)
    if (min > DOCK_AUTO_THRESHOLD) return
    let edge: DockEdge
    if (min === dRight) edge = 'right'
    else if (min === dLeft) edge = 'left'
    else if (min === dBottom) edge = 'bottom'
    else edge = 'top'
    await dockTo(edge)
  }

  async function dockTo(edge: DockEdge) {
    const c = await currentAvatarScreenCenter()
    if (!c) return
    const win = getCurrentWindow()
    try {
      const pos = await win.outerPosition()
      const scale = await win.scaleFactor()
      undockedWinPos = { x: pos.x / scale, y: pos.y / scale, anchor: avatarAnchor.value }
    } catch { return }
    const mon = await getMonitorBounds()
    const t = computeDockTarget(edge, c.x, c.y, mon)
    avatarAnchor.value = t.newAnchor
    dockEdge.value = edge
    dockedWinPos = { x: t.winX, y: t.winY, anchor: t.newAnchor }
    await animateWindowToLogical(t.winX, t.winY, DOCK_ANIM_MS)
  }

  function computePeekTarget(
    edge: DockEdge,
    mon: { x: number; y: number; w: number; h: number },
  ): { winX: number; winY: number } | null {
    if (!dockedWinPos) return null
    const maxPeek = WINDOW_LOGICAL_W - DOCK_SHOW_PX
    const peek = Math.max(0, Math.min(DOCK_PEEK_PX, maxPeek))
    let winX = dockedWinPos.x
    let winY = dockedWinPos.y
    if (edge === 'right') {
      winX = dockedWinPos.x - peek
    } else if (edge === 'left') {
      winX = dockedWinPos.x + peek
    } else if (edge === 'top') {
      winY = dockedWinPos.y + peek
    } else {
      winY = dockedWinPos.y - peek
    }
    const off = ANCHOR_AVATAR_CENTER[dockedWinPos.anchor]
    const clampedX = Math.min(Math.max(winX, mon.x - off.x), mon.x + mon.w - off.x)
    const clampedY = Math.min(Math.max(winY, mon.y - off.y), mon.y + mon.h - off.y)
    return { winX: clampedX, winY: clampedY }
  }

  /**
   * 临时弹出露完整 avatar。
   * - recoil:true（默认） → 弹出后挂 5s 计时自动 retract
   * - recoil:false → 不启动计时，由调用者（hover）自己控制何时回收
   */
  async function pokeOut(opts: { recoil?: boolean } = {}) {
    if (!dockEdge.value) return
    const wantRecoil = opts.recoil !== false
    if (dockUndockTimer) { clearTimeout(dockUndockTimer); dockUndockTimer = null }
    if (!isPoked.value) {
      isPoked.value = true
      if (dockEdge.value) {
        const mon = await getMonitorBounds()
        const peekTarget = computePeekTarget(dockEdge.value, mon)
        if (peekTarget) {
          await animateWindowToLogical(peekTarget.winX, peekTarget.winY, DOCK_ANIM_MS)
        }
      }
    }
    if (wantRecoil) {
      dockUndockTimer = window.setTimeout(retract, DOCK_RECOIL_MS)
    }
  }

  function onAvatarHover() {
    if (!dockEdge.value) return
    pokeOut({ recoil: false })
  }

  function onAvatarLeave() {
    if (!dockEdge.value || !isPoked.value) return
    if (dockUndockTimer) clearTimeout(dockUndockTimer)
    dockUndockTimer = window.setTimeout(retract, DOCK_RECOIL_MS)
  }

  async function retract() {
    dockUndockTimer = null
    if (!dockEdge.value || !isPoked.value) return
    isPoked.value = false
    if (dockedWinPos) {
      avatarAnchor.value = dockedWinPos.anchor
      await animateWindowToLogical(dockedWinPos.x, dockedWinPos.y, DOCK_ANIM_MS)
    }
  }

  /** 用户手动取消 dock（菜单项 / 拖拽 break out） */
  async function exitDock() {
    if (!dockEdge.value) return
    dockEdge.value = null
    isPoked.value = false
    if (dockUndockTimer) { clearTimeout(dockUndockTimer); dockUndockTimer = null }
    if (undockedWinPos) {
      avatarAnchor.value = undockedWinPos.anchor
      await animateWindowToLogical(undockedWinPos.x, undockedWinPos.y, DOCK_ANIM_MS)
    }
    dockedWinPos = null
    undockedWinPos = null
  }

  async function menuToggleDock() {
    showMenu.value = false
    if (dockEdge.value) {
      await exitDock()
    } else {
      const c = await currentAvatarScreenCenter()
      if (!c) return
      const mon = await getMonitorBounds()
      const dRight = mon.x + mon.w - c.x
      const dLeft = c.x - mon.x
      const dBottom = mon.y + mon.h - c.y
      const dTop = c.y - mon.y
      const min = Math.min(dRight, dLeft, dBottom, dTop)
      let edge: DockEdge = 'right'
      if (min === dLeft) edge = 'left'
      else if (min === dBottom) edge = 'bottom'
      else if (min === dTop) edge = 'top'
      await dockTo(edge)
    }
  }

  return {
    // state
    dockEdge,
    isPoked,
    undockedWinPos: () => undockedWinPos,
    setUndockedWinPos: (v: { x: number; y: number; anchor: AvatarAnchor } | null) => { undockedWinPos = v },
    // animation (shared with App.vue's ensureBubbleVisible)
    animateWindowToLogical,
    // dock geometry helpers
    currentAvatarScreenCenter,
    getMonitorBounds,
    // dock actions
    maybeAutoDock,
    pokeOut,
    onAvatarHover,
    onAvatarLeave,
    retract,
    exitDock,
    menuToggleDock,
    cancelDockAnim,
    // for drag breakout: directly clear dock state without animation
    breakoutFromDock: () => {
      dockEdge.value = null
      isPoked.value = false
      if (dockUndockTimer) { clearTimeout(dockUndockTimer); dockUndockTimer = null }
      cancelDockAnim()
      dockedWinPos = null
      undockedWinPos = null
    },
  }
}
