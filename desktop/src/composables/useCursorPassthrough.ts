import { onMounted, onUnmounted } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { invoke } from '@tauri-apps/api/core'

/**
 * 让小人窗口的空白区域真穿透到桌面。
 *
 * 原理：
 *   - 每 120ms 调 Rust 的 cursor_pos_in_window 拿到鼠标相对窗口的 CSS 坐标
 *     （直接读 OS 的鼠标位置，绕开 WebView 在 ignoreCursorEvents=true 时收
 *     不到事件的问题）
 *   - document.elementFromPoint(x, y) 看看那个像素下是不是 .pointer-target
 *   - 是 → 关穿透（窗口接事件）；不是 → 开穿透（鼠标穿过去落桌面）
 *
 * 标记可交互元素：在 root 元素（小人、菜单、气泡、各 panel）上加 class="pointer-target"。
 */
const POLL_INTERVAL = 120
const SELECTOR = '.pointer-target'

// ===== 模块级拖拽锁 =====
// 拖拽期间由 useAvatarDrag 置 true：窗口高速移动时，鼠标与窗口位置两次读取存在
// 时间差，换算出的窗口内坐标误差可达几十像素 → 轮询误判「鼠标不在 UI 上」→
// 中途给窗口切穿透：原生移动循环突然收不到鼠标（窗口卡住→下个 tick 判定翻回→
// 瞬移追赶），ignoreCursorEvents 反复翻转还引发窗口反复重合成闪烁，手动路径下
// 更会吃掉浏览器 mouseup。拖拽时鼠标必然按在小人上，无需判定，直接锁定窗口接收事件。
let dragHold = false
// 当前窗口实例的 setIgnore：composable 初始化时注册，供模块级 API 立即下发
let activeSetIgnore: ((value: boolean) => Promise<void>) | null = null

export function setPassthroughDragHold(hold: boolean) {
  dragHold = hold
  // 上锁时立即锁定接收事件，不等下一 tick；解锁后由下一 tick 恢复正常判定
  if (hold) void activeSetIgnore?.(false)
}

export function useCursorPassthrough() {
  const win = getCurrentWindow()
  // 起始 null：本地 tracker 不知道 OS 真实状态，首次 setIgnore 必下发
  let isIgnoring: boolean | null = null
  let timer: ReturnType<typeof setInterval> | null = null
  let polling = false

  async function setIgnore(value: boolean) {
    // 拖拽锁定期禁止切穿透：防止上锁前已在途的 tick 完成后覆盖锁定
    if (dragHold && value) return
    if (value === isIgnoring) return
    try {
      await win.setIgnoreCursorEvents(value)
      isIgnoring = value
    } catch (e) {
      console.error('[passthrough] setIgnoreCursorEvents 失败：', e)
    }
  }

  async function tick() {
    // 拖拽中：跳过判定，保持窗口锁定接收事件（见 setPassthroughDragHold）
    if (dragHold) return
    if (polling) return
    polling = true
    try {
      const [x, y] = await invoke<[number, number]>('cursor_pos_in_window')
      const w = window.innerWidth
      const h = window.innerHeight
      if (x < 0 || y < 0 || x >= w || y >= h) {
        // 鼠标在窗口外，开穿透（小人挂在桌面右下，鼠标在桌面区域时应该能正常用桌面）
        await setIgnore(true)
        return
      }
      const el = document.elementFromPoint(x, y)
      const onUI = !!(el && el.closest && el.closest(SELECTOR))
      await setIgnore(!onUI)
    } catch (e) {
      console.error('[passthrough] cursor_pos_in_window 失败：', e)
      await setIgnore(false)
    } finally {
      polling = false
    }
  }

  function startPolling() {
    if (timer) return
    // 窗口回来先关穿透，避免 120ms 盲区
    setIgnore(false)
    timer = setInterval(tick, POLL_INTERVAL)
  }

  function stopPolling() {
    if (timer) {
      clearInterval(timer)
      timer = null
    }
  }

  let visCleanup: (() => void) | null = null

  onMounted(() => {
    // 把本窗口实例的 setIgnore 注册到模块级，供 setPassthroughDragHold 立即锁定
    activeSetIgnore = setIgnore
    setIgnore(false)
    if (!document.hidden) startPolling()

    const onVis = () => {
      if (document.hidden) stopPolling()
      else startPolling()
    }
    document.addEventListener('visibilitychange', onVis)
    visCleanup = () => document.removeEventListener('visibilitychange', onVis)
  })

  onUnmounted(() => {
    activeSetIgnore = null
    stopPolling()
    visCleanup?.()
    setIgnore(false)
  })
}
