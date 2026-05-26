import { onMounted, onUnmounted } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { invoke } from '@tauri-apps/api/core'

/**
 * 让小人窗口的空白区域真穿透到桌面。
 *
 * 原理（v2 重写）：
 *   - 每 80ms 调 Rust 的 cursor_pos_in_window 拿到鼠标相对窗口的 CSS 坐标
 *     （直接读 OS 的鼠标位置，绕开 WebView 在 ignoreCursorEvents=true 时收
 *     不到事件的问题）
 *   - document.elementFromPoint(x, y) 看看那个像素下是不是 .pointer-target
 *   - 是 → 关穿透（窗口接事件）；不是 → 开穿透（鼠标穿过去落桌面）
 *
 * 为什么不沿用 v1 的 mousemove + :hover + 150ms 探针：
 *   开了 ignoreCursorEvents 之后 OS 根本不向 WebView 派发鼠标事件，:hover
 *   状态卡在 OFF。临时关穿透再查 :hover 也救不回来 —— 静止鼠标不触发
 *   WM_MOUSEMOVE，hover 仍是旧值。结果就是用户从空白区移到小人上后小人
 *   永远不可点。
 *
 * 标记可交互元素：在 root 元素（小人、菜单、气泡、各 panel）上加 class="pointer-target"。
 */
const POLL_INTERVAL = 120
const SELECTOR = '.pointer-target'

export function useCursorPassthrough() {
  const win = getCurrentWindow()
  // 起始记 null —— 表示"OS 实际状态未知"。这样首次 setIgnore() 一定真调下去，
  // 避免本地 tracker 和 OS 不一致导致永久卡 passthrough（macOS transparent 窗口
  // 复现过：本地以为 false 实际 true，从此所有点击都被穿透到桌面）。
  let isIgnoring: boolean | null = null
  let timer: ReturnType<typeof setInterval> | null = null
  let polling = false

  async function setIgnore(value: boolean) {
    if (value === isIgnoring) return
    try {
      await win.setIgnoreCursorEvents(value)
      isIgnoring = value
    } catch (e) {
      // 历史教训：capabilities/default.json 漏了 set-ignore-cursor-events
      // 权限时这里被静默吞了，整套穿透看上去什么都没发生。出问题要 console
      // 报，别让人查半天。
      console.error('[passthrough] setIgnoreCursorEvents 失败：', e)
    }
  }

  async function tick() {
    if (polling) return
    polling = true
    try {
      const [x, y] = await invoke<[number, number]>('cursor_pos_in_window')
      // 鼠标在窗口外 → 维持当前状态（不主动改）。0..rect 之间才在窗口内。
      const w = window.innerWidth
      const h = window.innerHeight
      if (x < 0 || y < 0 || x >= w || y >= h) return

      const el = document.elementFromPoint(x, y)
      const onUI = !!(el && el.closest && el.closest(SELECTOR))
      // onUI=true  → 关穿透（让窗口收事件）
      // onUI=false → 开穿透（鼠标穿过窗口空白区到桌面）
      await setIgnore(!onUI)
    } catch (e) {
      // 调 cursor_pos_in_window 失败一般是 capability 没配 / 权限问题。
      // 安全策略：失败时关穿透，至少保证 UI 可用 —— 否则用户点不到小人。
      console.error('[passthrough] cursor_pos_in_window 失败：', e)
      await setIgnore(false)
    } finally {
      polling = false
    }
  }

  onMounted(() => {
    // 启动时显式同步状态 —— 默认不穿透，保证 wizard / 小人立刻可点
    setIgnore(false)
    timer = setInterval(tick, POLL_INTERVAL)
  })

  onUnmounted(() => {
    if (timer) clearInterval(timer)
    timer = null
    // 关穿透避免遗留状态影响其它窗口（同 app 多窗口场景）
    setIgnore(false)
  })
}
