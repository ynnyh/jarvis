import { type Ref } from 'vue'
import {
  getCurrentWindow,
  LogicalPosition,
  PhysicalPosition,
  currentMonitor,
  cursorPosition,
} from '@tauri-apps/api/window'
import type { UnlistenFn } from '@tauri-apps/api/event'
import type { AvatarAnchor, DockEdge } from './useAvatarDock'
import { ANCHOR_AVATAR_CENTER } from './useAvatarDock'
import { setPassthroughDragHold } from './useCursorPassthrough'

// Windows 上原生 startDragging（WM_NCLBUTTONDOWN 移动循环）会在透明无边框窗
// 周围画出系统拖拽方框；更致命的是透明区域命中测试返回 HTTRANSPARENT，快速拖拽
// 时移动循环被提前终止 → 窗口停在轨迹中途（闪现/丢失）。故 Windows 走手动移窗
// （beginManualDrag/pumpManualMove，不进 OS 移动循环），macOS 保留原生。
const IS_WINDOWS = navigator.userAgent.includes('Windows')

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

// 拖拽结束判定（macOS 原生路径）：拖拽期间 onMoved 持续触发，停手后这段时间没有新移动即视为松手。
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
  // Windows 手动拖拽：mousemove 驱动的单在途移窗泵 + mousedown 采集的基准（全物理像素）
  let moveDirty = false
  let movePump: Promise<void> | null = null
  // 手势代际：每次 mousedown 递增。跨手势存活的异步续体（迟到的基准回包、在途泵、
  // finishDrag 等泵后的收尾）据此识别自己已被新手势取代，立即让位——否则快速
  // 松手再抓起时，旧基准会写窗（起步瞬移）、旧收尾会在新拖拽中途换锚/auto-dock。
  let gestureSeq = 0
  // 基准采集是异步的（beginManualDrag 里 await）：就绪前 mousemove 只置脏不起泵，
  // 否则泵会拿初始 0 / 上次拖拽的残留基准把窗口写到错误位置（起步瞬移）
  let baseReady = false
  let baseWinX = 0
  let baseWinY = 0
  let baseCurX = 0
  let baseCurY = 0

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

  /** 收尾：等在途移窗落地、拆 onMoved 监听、重算锚点、触发 auto-dock。幂等。 */
  async function finishDrag() {
    if (!sessionActive) return
    sessionActive = false  // 移窗泵的 while 条件读到 false 即自行退出
    clearDragEndTimer()
    if (moveUnlisten) {
      moveUnlisten()
      moveUnlisten = null
    }
    if (started) {
      started = false
      // 等在途的移窗写入落地：保证 recomputeAnchor 读到的 outerPosition 是终态，
      // 换锚点的 setPosition 也不会被迟到的拖拽写入覆盖
      const seq = gestureSeq
      if (movePump) await movePump.catch(() => {})
      // 等待期间用户已再次按下（手势换代）：此刻换锚/auto-dock 会打断进行中的
      // 新拖拽，让位；锚点由新手势自己的收尾按最终落点统一重算。
      // 让位前先解锁穿透——新手势若是单击不拖（不走 beginDrag），没人再解锁，
      // hold 会永久卡住 → 穿透失效。但仅当没有新手势正在拖拽时才解锁：
      // 新手势 beginDrag 若已上锁（started 已被它置回 true），这里的 false
      // 会揭掉它的锁（它不会再次上锁），改由新手势自己的收尾负责解锁
      if (seq !== gestureSeq) {
        if (!started) setPassthroughDragHold(false)
        return
      }
      await recomputeAnchor()
      onDragEnd()
      // 拖拽收尾完毕，解锁穿透轮询，下一 tick 恢复正常判定；
      // 同上，新手势已开拖（started 为 true）时不解锁，让它自己收尾
      if (!started) setPassthroughDragHold(false)
    }
  }

  /** 越过阈值：进入拖拽。macOS 交给 OS 原生拖拽，Windows 手动移窗。 */
  async function beginDrag() {
    if (started) return
    started = true
    // 锁定穿透轮询：拖拽期间窗口高速移动，轮询按带时间差的坐标误判「鼠标不在
    // UI 上」会中途切穿透 → 打断移动循环（窗口卡住→瞬移）、反复重合成闪烁、
    // 手动路径下还会吃掉 mouseup。两平台路径都在拖拽，都要锁。
    setPassthroughDragHold(true)
    // 先挣脱收纳态（含取消 dock 动画），窗口位置稳定后再采手动拖拽基准
    if (dockEdge.value) onDragStart()
    if (IS_WINDOWS) {
      await beginManualDrag()
    } else {
      await beginNativeDrag()
    }
  }

  /** macOS：原生拖拽接管（丝滑、全屏、不经任何坐标换算）。 */
  async function beginNativeDrag() {
    // 原生拖拽接管手势后，OS 会吞掉 mouseup（尤其 macOS），浏览器侧这两个监听
    // 既收不到事件、又不会被 onWindowMouseUp 摘除 → 先在这里主动拆，杜绝重复叠加。
    detachMouseListeners()
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

  /** Windows：手动拖拽。窗位 = mousedown 基准 + 鼠标物理位移，全程物理像素零 scale
   *  换算（混合多屏不漂移）。不进 OS 移动循环 → 无系统拖拽方框；mouseup 浏览器可正常
   *  收到（保留监听），由 onWindowMouseUp 调 finishDrag 收尾。 */
  async function beginManualDrag() {
    const seq = gestureSeq
    const win = getCurrentWindow()
    try {
      const [winPos, curPos] = await Promise.all([win.outerPosition(), cursorPosition()])
      // 采集期间手势已换代（快速松手又按下）：这套基准属于旧手势，作废——
      // 否则它会冒充新手势的基准被泵拿去用，窗口按旧鼠标原点瞬移
      if (seq !== gestureSeq) return
      baseWinX = winPos.x
      baseWinY = winPos.y
      baseCurX = curPos.x
      baseCurY = curPos.y
    } catch {
      // 手势已换代时不能 finishDrag：sessionActive 此刻属于新手势，会误杀
      if (seq === gestureSeq) await finishDrag()
      return
    }
    baseReady = true
    // mousedown 起的位移（含基准采集期间攒下的脏标记）在此补追一轮；之后由 mousemove 驱动泵
    requestManualMove()
  }

  /** mousemove 到达时调用：置脏，泵空闲则起泵。移窗只由 mousemove 驱动，
   *  鼠标静止/松手后零 IPC，天然没有「松手后窗口还在追」的问题。 */
  function requestManualMove() {
    moveDirty = true
    // 基准未就绪：只保留脏标记，等 beginManualDrag 采集完成后末尾那次
    // requestManualMove() 统一消费，避免泵用错误基准写窗
    if (!baseReady) return
    if (!movePump) {
      movePump = pumpManualMove().finally(() => {
        movePump = null
      })
    }
  }

  /** 单在途移窗泵：读一次 OS 鼠标物理坐标、await 写一次窗口位置，写完还脏就再来一轮。
   *  写也 await（不 fire-and-forget）→ IPC 队列里至多一条 setPosition，不堆积，
   *  松手后不会陆续 drain 冲向最后点；节奏由 IPC 往返自定，不受窗口移动期间
   *  WebView2 合成器对 RAF 降频的影响。cursorPosition 在途期间可能已松手
   *  （finishDrag 已收尾）甚至已换代（新手势的 sessionActive/started 又为 true，
   *  但基准是新手势的），读回后必须重查 session + 代际，丢弃迟到写入，
   *  避免与 recomputeAnchor 的换锚 setPosition 或新手势的泵竞争。 */
  async function pumpManualMove() {
    const seq = gestureSeq
    const win = getCurrentWindow()
    while (moveDirty && sessionActive && started && seq === gestureSeq) {
      moveDirty = false
      let c: { x: number; y: number }
      try {
        c = await cursorPosition()
      } catch {
        break
      }
      if (!sessionActive || !started || seq !== gestureSeq) break
      try {
        await win.setPosition(
          new PhysicalPosition(
            Math.round(baseWinX + (c.x - baseCurX)),
            Math.round(baseWinY + (c.y - baseCurY)),
          ),
        )
      } catch {}
    }
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
    gestureSeq += 1   // 手势换代：旧手势所有跨 await 的续体（泵/基准回包/收尾）就此失效
    moveDirty = false  // 清掉上一手势可能残留的脏标记；残留泵会因代际不符自行退出
    baseReady = false  // 本次手势的基准尚未采集
    detachMouseListeners()  // 防御：上一手势若异常残留，先摘再挂，避免重复
    window.addEventListener('mousemove', onWindowMouseMove)
    window.addEventListener('mouseup', onWindowMouseUp)
  }

  // 只用位移差判断「是否越过阈值」——差值消掉了原点，不碰 macOS 上 screenY 的量纲问题。
  // 越阈值即进入拖拽；macOS 原生路径之后浏览器 mousemove 停发也无所谓，
  // Windows 手动路径则靠后续 mousemove 驱动移窗泵。
  function onWindowMouseMove(e: MouseEvent) {
    if (!sessionActive) return
    if (!(e.buttons & 1)) {
      onWindowMouseUp(e)
      return
    }
    if (started) {
      if (IS_WINDOWS) requestManualMove()
      return
    }
    if (
      Math.abs(e.screenX - downScreenX) > MOVE_THRESHOLD_PX ||
      Math.abs(e.screenY - downScreenY) > MOVE_THRESHOLD_PX
    ) {
      void beginDrag()
    }
  }

  function onWindowMouseUp(e: MouseEvent) {
    detachMouseListeners()

    const duration = Date.now() - mouseDownTime
    const dx = Math.abs(e.screenX - downScreenX)
    const dy = Math.abs(e.screenY - downScreenY)
    // 没启动拖拽 + 短按 + 几乎没动 = 单击
    if (!started && duration < CLICK_MAX_MS && dx < MOVE_THRESHOLD_PX && dy < MOVE_THRESHOLD_PX) {
      sessionActive = false
      onClick()
      return
    }
    if (started) {
      if (IS_WINDOWS) {
        // 手动路径：mouseup 即拖拽结束，直接收尾
        void finishDrag()
      } else {
        // 原生路径：交给 onMoved 防抖 / startDragging resolve 去收尾；这里补一次兜底
        scheduleDragEnd()
      }
    } else {
      // 越过阈值但拖拽还没起来（极短窗口），直接收尾
      sessionActive = false
    }
  }

  return {
    onMouseDown,
    recomputeAnchor,
  }
}
