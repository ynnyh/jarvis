import { type Ref } from 'vue'
import { getCurrentWindow, LogicalPosition, currentMonitor } from '@tauri-apps/api/window'
import type { AvatarAnchor, DockEdge } from './useAvatarDock'
import { ANCHOR_AVATAR_CENTER } from './useAvatarDock'

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

export interface UseAvatarDragOptions {
  avatarAnchor: Ref<AvatarAnchor>
  dockEdge: Ref<DockEdge | null>
  /** Called when a real drag breaks out of dock state */
  onDragStart: () => void
  /** Called when drag ends; should run recomputeAnchor + maybeAutoDock */
  onDragEnd: () => void
  /** Called on left-click (short press, no significant movement) */
  onClick: () => void
}

export function useAvatarDrag(options: UseAvatarDragOptions) {
  const { avatarAnchor, dockEdge, onDragStart, onDragEnd, onClick } = options

  // ===== Drag state =====
  let mouseDownTime = 0
  let mouseDownX = 0
  let mouseDownY = 0
  let isDragging = false

  let dragStartWinLogicalX = 0
  let dragStartWinLogicalY = 0
  let pendingDragX: number | null = null
  let pendingDragY: number | null = null
  let dragRafId: number | null = null

  // ===== RAF flush =====

  function flushDragPosition() {
    dragRafId = null
    if (pendingDragX === null || pendingDragY === null) return
    const x = pendingDragX
    const y = pendingDragY
    pendingDragX = null
    pendingDragY = null
    getCurrentWindow().setPosition(new LogicalPosition(x, y)).catch(() => {})
  }

  // ===== Anchor computation =====

  async function recomputeAnchor() {
    const win = getCurrentWindow()
    let winLogicalX: number
    let winLogicalY: number
    try {
      const pos = await win.outerPosition()
      const scale = await win.scaleFactor()
      winLogicalX = pos.x / scale
      winLogicalY = pos.y / scale
    } catch {
      return
    }
    const oldOffset = ANCHOR_AVATAR_CENTER[avatarAnchor.value]
    const avatarScreenX = winLogicalX + oldOffset.x
    const avatarScreenY = winLogicalY + oldOffset.y
    const mon = await getMonitorBounds()
    const relX = avatarScreenX - mon.x
    const relY = avatarScreenY - mon.y
    const horiz = relX >= mon.w / 2 ? 'r' : 'l'
    const vert = relY >= mon.h / 2 ? 'b' : 't'
    const newAnchor = (horiz + vert) as AvatarAnchor
    if (newAnchor === avatarAnchor.value) return

    const newOffset = ANCHOR_AVATAR_CENTER[newAnchor]
    const newWinX = avatarScreenX - newOffset.x
    const newWinY = avatarScreenY - newOffset.y
    try {
      await win.setPosition(new LogicalPosition(Math.round(newWinX), Math.round(newWinY)))
    } catch {}
    avatarAnchor.value = newAnchor
  }

  // ===== Mouse handlers =====

  async function onMouseDown(e: MouseEvent) {
    if (e.button !== 0) return
    isDragging = false
    mouseDownTime = Date.now()
    mouseDownX = e.screenX
    mouseDownY = e.screenY
    try {
      const win = getCurrentWindow()
      const pos = await win.outerPosition()
      const scale = await win.scaleFactor()
      dragStartWinLogicalX = pos.x / scale
      dragStartWinLogicalY = pos.y / scale
    } catch {
      return
    }
    window.addEventListener('mousemove', onWindowMouseMove)
    window.addEventListener('mouseup', onWindowMouseUp)
  }

  function onWindowMouseMove(e: MouseEvent) {
    if (!(e.buttons & 1)) {
      onWindowMouseUp(e)
      return
    }
    if (!isDragging) {
      const dx = Math.abs(e.screenX - mouseDownX)
      const dy = Math.abs(e.screenY - mouseDownY)
      if (dx <= 5 && dy <= 5) return
      isDragging = true
      if (dockEdge.value) {
        onDragStart()
      }
    }
    const dxLogical = e.screenX - mouseDownX
    const dyLogical = e.screenY - mouseDownY
    pendingDragX = dragStartWinLogicalX + dxLogical
    pendingDragY = dragStartWinLogicalY + dyLogical
    if (dragRafId === null) {
      dragRafId = requestAnimationFrame(flushDragPosition)
    }
  }

  function onWindowMouseUp(e: MouseEvent) {
    window.removeEventListener('mousemove', onWindowMouseMove)
    window.removeEventListener('mouseup', onWindowMouseUp)
    if (dragRafId !== null) {
      cancelAnimationFrame(dragRafId)
      dragRafId = null
    }
    flushDragPosition()

    const duration = Date.now() - mouseDownTime
    const dx = Math.abs(e.screenX - mouseDownX)
    const dy = Math.abs(e.screenY - mouseDownY)
    if (!isDragging && duration < 300 && dx < 5 && dy < 5) {
      onClick()
    }
    const wasDragging = isDragging
    isDragging = false
    if (wasDragging) {
      recomputeAnchor().then(() => onDragEnd())
    }
  }

  return {
    onMouseDown,
    recomputeAnchor,
  }
}
