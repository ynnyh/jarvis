import { type Ref } from 'vue'
import { getCurrentWindow, currentMonitor } from '@tauri-apps/api/window'
import { invoke } from '@tauri-apps/api/core'
import type { UnlistenFn } from '@tauri-apps/api/event'
import type { AvatarAnchor, DockEdge } from './useAvatarDock'
import { ANCHOR_AVATAR_CENTER } from './useAvatarDock'
import { setPassthroughDragHold } from './useCursorPassthrough'

// ============================================================================
// 拖拽机制平台分叉（2026-07）：
// - Windows：走 Rust 自驱移窗（manual_drag_start/stop）。原 startDragging 会落入
//   DefWindowProc 的 NC move loop，内核把窗口顶边钳到工作区顶（rcWork.top，扣任务栏）；
//   avatar 窗 900 高、小人居中心（头顶 450px 空窗），导致小人最高只能拖到屏幕中线。
//   自驱路径在 Rust 线程里用 GetCursorPos + SetWindowPos(SWP_NOSENDCHANGING) 每帧定位，
//   不进 NC move loop → 无内核钳制 → 小人可拖到屏幕任意位置（含上半屏顶）。
//   跨多屏 DPI 时让窗口中心（=小人居所）始终对齐光标，按当前物理半宽重算，跨屏不飞。
// - macOS：startDragging 走 NSWindow 原生拖拽，无该钳制，保持原生丝滑。
const IS_MAC = /Mac/i.test(navigator.userAgent)
const MANUAL_DRAG = !IS_MAC

// ============================================================================
// 二期重构（2026-07）：小人恒居 900×900 窗口正中心，两平台统一原生 startDragging。
//
// 旧设计「小人贴窗角 + 松手换锚需平移窗口几百 px + DOM 翻角」里，窗口平移走
// IPC/系统调用管线、DOM 翻角走渲染管线，二者永不同帧 → 间隙帧就是用户看到的
// 松手闪跳，任何时序补丁都治不了（一期实测失败）。二期消除发生条件：
//
// - 小人永远在窗口中心（ANCHOR_AVATAR_CENTER 四键全部 = {450,450}）；
// - anchor 只决定面板/气泡开向（App.vue 纯 CSS），换锚零窗口平移；
// - recomputeAnchor 退化为纯状态赋值，没有任何 setPosition；
// - 拖拽 = 原生 startDragging（丝滑、全屏可达、零坐标换算）+ 拖拽期穿透锁
//   （原生拖拽中窗口由 OS 移动，穿透轮询两次 IPC 读取带时间差会误判「鼠标
//   不在 UI 上」→ 中途切穿透打断移动循环）。
// - 松手链 = onMoved 防抖/startDragging resolve 兜底收尾 → recomputeAnchor
//   （纯赋值）→ auto-dock 判定。dock 动画成为唯一主动动窗的场景。
//
// 已试验并否决（2026-07 真机实测，勿再重试）——「拖拽期原子缩窗」：为绕开
// Windows 原生移动循环「窗顶不得高于屏顶」的钳制，曾在 beginNativeDrag 前把
// 900 窗原子缩成 100×100（SetWindowPos 单系统调用、小人中心不动），松手扩回。
// 实测两处硬伤：① SetWindowPos 原子只保证「窗口几何」一步到位，WebView2 的
// 内容重排走渲染管线、与窗口几何永不同帧 → 缩/扩瞬间小人闪帧（正是二期要
// 消灭的跨管线间隙，换个位置又长出来了）；② 扩回失败/未落地时，后续几何
// 判定（auto-dock 等）按 900 假设算在 100 窗上，错位 400px 引发误收纳。
// Windows 屏顶可达改由 Rust 侧解决：WM_MOVING 子类钩子在移动会话内按
// 「鼠标-窗口偏移」改写系统建议位置，解除顶边钳制，前端拖拽路径零特判
// （见 src-tauri/src/commands/window.rs 的 avatar_move_hook）。
// ============================================================================

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
  /** Called when drag ends; should run maybeAutoDock */
  onDragEnd: () => void
  /** Called on left-click (short press, no significant movement) */
  onClick: () => void
}

// 拖拽结束判定：拖拽期间 onMoved 持续触发，停手后这段时间没有新移动即视为松手。
const DRAG_END_DEBOUNCE_MS = 200
// 单击判定阈值
const CLICK_MAX_MS = 300
const MOVE_THRESHOLD_PX = 5

/** 拖拽结束后按小人落点重算面板开向。纯状态赋值：小人恒居窗口中心，换锚
 *  只翻 CSS（面板/气泡开向），不平移窗口 → 闪跳发生条件被物理消灭。 */
function makeRecomputeAnchor(avatarAnchor: Ref<AvatarAnchor>) {
  return async function recomputeAnchor() {
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
    // 四键映射值恒为窗口中心 {450,450}，读哪个键都一样
    const off = ANCHOR_AVATAR_CENTER[avatarAnchor.value]
    const avatarScreenX = winLogicalX + off.x
    const avatarScreenY = winLogicalY + off.y
    const mon = await getMonitorBounds()
    const horiz = avatarScreenX - mon.x >= mon.w / 2 ? 'r' : 'l'
    const vert = avatarScreenY - mon.y >= mon.h / 2 ? 'b' : 't'
    avatarAnchor.value = (horiz + vert) as AvatarAnchor
  }
}

export function useAvatarDrag(options: UseAvatarDragOptions) {
  const { avatarAnchor, dockEdge, onDragStart, onDragEnd, onClick } = options

  // ===== Drag state =====
  let mouseDownTime = 0
  // mousedown 时的鼠标屏幕坐标，仅用于「是否越过阈值 = 真拖动」判定（差值，不做坐标换算）
  let downScreenX = 0
  let downScreenY = 0
  let started = false        // 原生拖拽是否已启动
  let sessionActive = false  // 一次 mousedown→mouseup 生命周期是否进行中
  let useManualDrag = false  // 本次拖拽是否走方案2 Rust 自驱路径

  let moveUnlisten: UnlistenFn | null = null
  let dragEndTimer: number | null = null

  const recomputeAnchor = makeRecomputeAnchor(avatarAnchor)

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

  /** 收尾：拆 onMoved 监听、重算面板开向（纯赋值）、触发 auto-dock、解锁穿透。幂等。 */
  async function finishDrag() {
    if (!sessionActive) return
    sessionActive = false
    clearDragEndTimer()
    const wasManual = useManualDrag
    if (useManualDrag) {
      useManualDrag = false
      invoke('manual_drag_stop').catch(() => {})
    }
    if (moveUnlisten) {
      moveUnlisten()
      moveUnlisten = null
    }
    if (started) {
      started = false
      await recomputeAnchor()
      // 自驱路径下"停哪是哪"：跳过 maybeAutoDock——它会因小人离屏边 < 阈值而把窗口
      // 推到 dock 收纳位（如拖到屏顶被 dockTo('top') 弹回屏幕中线），正是用户反馈
      // "拖到上半屏顶、松手被弹回中"的根因。原生拖拽路径保留 auto-dock 不变。
      if (!wasManual) onDragEnd()
      setPassthroughDragHold(false)
    }
  }

  /** 越过阈值：操作系统接管拖拽。
   *  Windows：走 Rust 自驱移窗（manual_drag_start），Rust 线程轮询 GetCursorPos +
   *  SetWindowPos 每帧定位，不进 NC move loop → 无内核顶边钳制 → 小人可拖到屏幕任意
   *  位置（含上半屏顶）。跨多屏 DPI 时让窗口中心对齐光标，按当前物理半宽重算 → 不飞。
   *  macOS：原生 startDragging（NSWindow），无钳制问题，丝滑。 */
  async function beginNativeDrag() {
    if (started) return
    started = true
    // 拖拽中窗口被高速移动，穿透轮询两次 IPC 读取带时间差会误判「鼠标不在
    // UI 上」→ 中途切穿透打断移动循环；拖拽期整体挂起轮询
    setPassthroughDragHold(true)
    if (dockEdge.value) onDragStart()  // 从收纳态挣脱
    const win = getCurrentWindow()
    // onMoved 在拖拽期间持续触发；每次刷新防抖计时，停手后 DEBOUNCE 内无移动即收尾
    try {
      moveUnlisten = await win.onMoved(() => {
        if (sessionActive) scheduleDragEnd()
      })
    } catch {
      moveUnlisten = null
    }
    if (MANUAL_DRAG) {
      // 方案2 自驱路径：Rust 轮询线程驱动 SetWindowPos，不进 NC move loop → 无顶边钳制。
      // 浏览器在此路径下能正常收到 mouseup（OS 不接管），保留前端 mouse 监听以便停钩。
      // 穿透锁 + onMoved 防抖收尾逻辑与原生路径共用。
      useManualDrag = true
      try {
        await invoke('manual_drag_start')
      } catch {
        await finishDrag()
      }
      return
    }
    // 原生拖拽接管手势后，OS 会吞掉 mouseup（尤其 macOS），浏览器侧这两个监听
    // 既收不到事件、又不会被 onWindowMouseUp 摘除 → 先在这里主动拆，杜绝重复叠加。
    detachMouseListeners()
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
