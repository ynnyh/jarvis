import { type Ref } from 'vue'
import { getCurrentWindow, LogicalPosition, currentMonitor } from '@tauri-apps/api/window'
import type { UnlistenFn } from '@tauri-apps/api/event'
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

// 拖拽结束判定：原生拖拽期间 onMoved 持续触发，停手后这段时间没有新移动即视为松手。
const DRAG_END_DEBOUNCE_MS = 200
// 单击判定阈值
const CLICK_MAX_MS = 300
const MOVE_THRESHOLD_PX = 5

export function useAvatarDrag(options: UseAvatarDragOptions) {
  const { avatarAnchor, dockEdge, onDragStart, onDragEnd, onClick } = options

  // ===== Drag state =====
  let mouseDownTime = 0
  // mousedown 时的鼠标屏幕坐标，仅用于「是否越过阈值 = 真拖动」判定（差值，不做坐标换算）
  let downScreenX = 0
  let downScreenY = 0
  let started = false        // 原生拖拽是否已启动
  let sessionActive = false  // 一次 mousedown→mouseup 生命周期是否进行中

  let moveUnlisten: UnlistenFn | null = null
  let dragEndTimer: number | null = null

  // ===== Anchor computation =====
  // 拖拽结束后按 avatar 落点重算贴哪个角，必要时平移窗口让 avatar 中心不动、只换锚点。
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

  // ===== Drag lifecycle =====

  function clearDragEndTimer() {
    if (dragEndTimer !== null) {
      clearTimeout(dragEndTimer)
      dragEndTimer = null
    }
  }

  function scheduleDragEnd() {
    clearDragEndTimer()
    dragEndTimer = window.setTimeout(finishDrag, DRAG_END_DEBOUNCE_MS)
  }

  /** 收尾：拆 onMoved 监听、重算锚点、触发 auto-dock。幂等。 */
  async function finishDrag() {
    if (!sessionActive) return
    sessionActive = false
    clearDragEndTimer()
    if (moveUnlisten) {
      moveUnlisten()
      moveUnlisten = null
    }
    if (started) {
      started = false
      await recomputeAnchor()
      onDragEnd()
    }
  }

  /** 越过阈值：交给 OS 原生拖拽接管（丝滑、全屏、不经任何坐标换算）。 */
  async function beginNativeDrag() {
    if (started) return
    started = true
    // 原生拖拽接管手势后，OS 会吞掉 mouseup（尤其 macOS），浏览器侧这两个监听
    // 既收不到事件、又不会被 onWindowMouseUp 摘除 → 先在这里主动拆，杜绝重复叠加。
    detachMouseListeners()
    if (dockEdge.value) onDragStart()  // 从收纳态挣脱
    const win = getCurrentWindow()
    // onMoved 在原生拖拽期间持续触发；每次刷新防抖计时，停手后 DEBOUNCE 内无移动即收尾
    try {
      moveUnlisten = await win.onMoved(() => {
        if (sessionActive) scheduleDragEnd()
      })
    } catch {
      moveUnlisten = null
    }
    try {
      await win.startDragging()
    } catch {
      // 启动失败：直接收尾，避免卡死在 sessionActive
      await finishDrag()
      return
    }
    // startDragging 的 Promise 通常在拖拽结束后 resolve；作为 onMoved 之外的兜底收尾。
    scheduleDragEnd()
  }

  // ===== Mouse handlers =====

  function detachMouseListeners() {
    window.removeEventListener('mousemove', onWindowMouseMove)
    window.removeEventListener('mouseup', onWindowMouseUp)
  }

  function onMouseDown(e: MouseEvent) {
    if (e.button !== 0) return
    mouseDownTime = Date.now()
    downScreenX = e.screenX
    downScreenY = e.screenY
    started = false
    sessionActive = true
    detachMouseListeners()  // 防御：上一手势若异常残留，先摘再挂，避免重复
    window.addEventListener('mousemove', onWindowMouseMove)
    window.addEventListener('mouseup', onWindowMouseUp)
  }

  // 只用位移差判断「是否越过阈值」——差值消掉了原点，不碰 macOS 上 screenY 的量纲问题。
  // 越阈值即把控制权交给原生拖拽，之后浏览器 mousemove 停发也无所谓。
  function onWindowMouseMove(e: MouseEvent) {
    if (!sessionActive) return
    if (!(e.buttons & 1)) {
      onWindowMouseUp(e)
      return
    }
    if (started) return
    if (
      Math.abs(e.screenX - downScreenX) > MOVE_THRESHOLD_PX ||
      Math.abs(e.screenY - downScreenY) > MOVE_THRESHOLD_PX
    ) {
      void beginNativeDrag()
    }
  }

  function onWindowMouseUp(e: MouseEvent) {
    detachMouseListeners()

    const duration = Date.now() - mouseDownTime
    const dx = Math.abs(e.screenX - downScreenX)
    const dy = Math.abs(e.screenY - downScreenY)
    // 没启动原生拖拽 + 短按 + 几乎没动 = 单击
    if (!started && duration < CLICK_MAX_MS && dx < MOVE_THRESHOLD_PX && dy < MOVE_THRESHOLD_PX) {
      sessionActive = false
      onClick()
      return
    }
    // 拖过：交给 onMoved 防抖 / startDragging resolve 去收尾；这里补一次兜底。
    if (started) {
      scheduleDragEnd()
    } else {
      // 越过阈值但原生拖拽还没起来（极短窗口），直接收尾
      sessionActive = false
    }
  }

  return {
    onMouseDown,
    recomputeAnchor,
  }
}
