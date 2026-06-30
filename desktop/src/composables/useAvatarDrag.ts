import { type Ref } from 'vue'
import { getCurrentWindow, LogicalPosition, currentMonitor } from '@tauri-apps/api/window'
import { invoke } from '@tauri-apps/api/core'
import type { AvatarAnchor, DockEdge } from './useAvatarDock'
import { ANCHOR_AVATAR_CENTER } from './useAvatarDock'

/** 读 OS 全局逻辑鼠标坐标（top-left 原点，CSS px）。avatar 全部坐标交互的唯一真值源，
 *  retina 真机已验证。拖拽不再用 MouseEvent.screenX/screenY（WKWebView 上量纲不一致）。 */
async function cursorAbs(): Promise<{ x: number; y: number } | null> {
  try {
    const [x, y] = await invoke<[number, number]>('cursor_abs_logical')
    return { x, y }
  } catch {
    return null
  }
}

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
  // mousedown 时的鼠标全局逻辑坐标，用于「是否真拖动」阈值与单击判定
  let downCursorX = 0
  let downCursorY = 0
  // 抓取偏移：mousedown 时鼠标逻辑位 - 窗口逻辑位，拖拽全程保持不变
  let grabOffsetX = 0
  let grabOffsetY = 0
  // 最近一次读到的鼠标逻辑坐标（RAF 循环更新），mouseup 时算位移用
  let lastCursorX = 0
  let lastCursorY = 0
  let isDragging = false
  let dragActive = false
  let dragRafId: number | null = null

  // ===== Cursor-driven RAF loop =====
  // 每帧读一次 OS 鼠标逻辑坐标，窗位 = cursor - grabOffset。await 自带节流：
  // 上一帧的 invoke 没回来就不排下一帧，天然不会叠加调用。

  function startDragLoop() {
    dragActive = true
    const loop = async () => {
      if (!dragActive) return
      const c = await cursorAbs()
      if (c) {
        lastCursorX = c.x
        lastCursorY = c.y
        if (!isDragging) {
          if (Math.abs(c.x - downCursorX) > 5 || Math.abs(c.y - downCursorY) > 5) {
            isDragging = true
            if (dockEdge.value) onDragStart()
          }
        }
        if (isDragging) {
          getCurrentWindow()
            .setPosition(new LogicalPosition(Math.round(c.x - grabOffsetX), Math.round(c.y - grabOffsetY)))
            .catch(() => {})
        }
      }
      if (dragActive) dragRafId = requestAnimationFrame(loop)
    }
    dragRafId = requestAnimationFrame(loop)
  }

  function stopDragLoop() {
    dragActive = false
    if (dragRafId !== null) {
      cancelAnimationFrame(dragRafId)
      dragRafId = null
    }
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
    // 抓取偏移 = 当前鼠标逻辑位 - 当前窗口逻辑位；二者都走已验证的 OS 坐标，不碰 screenX/Y
    const c = await cursorAbs()
    if (!c) return
    try {
      const win = getCurrentWindow()
      const pos = await win.outerPosition()
      const scale = await win.scaleFactor()
      grabOffsetX = c.x - pos.x / scale
      grabOffsetY = c.y - pos.y / scale
    } catch {
      return
    }
    downCursorX = c.x
    downCursorY = c.y
    lastCursorX = c.x
    lastCursorY = c.y
    window.addEventListener('mousemove', onWindowMouseMove)
    window.addEventListener('mouseup', onWindowMouseUp)
    startDragLoop()
  }

  // mousemove 只作安全网：浏览器侧发现左键已松开就收尾（防 mouseup 丢失）。
  // 真正的位置跟踪在 startDragLoop 的 RAF 循环里读 OS 鼠标位完成。
  function onWindowMouseMove(e: MouseEvent) {
    if (!(e.buttons & 1)) {
      onWindowMouseUp()
    }
  }

  function onWindowMouseUp() {
    window.removeEventListener('mousemove', onWindowMouseMove)
    window.removeEventListener('mouseup', onWindowMouseUp)
    stopDragLoop()

    const duration = Date.now() - mouseDownTime
    const dx = Math.abs(lastCursorX - downCursorX)
    const dy = Math.abs(lastCursorY - downCursorY)
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
