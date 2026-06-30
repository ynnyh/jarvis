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
// 诊断开关：macOS 穿透回归排查期，把后端返回的原始 OS 坐标打日志，真机读数定公式。
// 复核完（cursor 到底是 logical 还是 physical）确认后改回 false 并清掉日志。
const DIAG = true

export function useCursorPassthrough() {
  const win = getCurrentWindow()
  // 起始 null：本地 tracker 不知道 OS 真实状态，首次 setIgnore 必下发
  let isIgnoring: boolean | null = null
  let timer: ReturnType<typeof setInterval> | null = null
  let polling = false

  async function setIgnore(value: boolean) {
    if (value === isIgnoring) return
    try {
      await win.setIgnoreCursorEvents(value)
      isIgnoring = value
    } catch (e) {
      console.error('[passthrough] setIgnoreCursorEvents 失败：', e)
    }
  }

  async function tick() {
    if (polling) return
    polling = true
    try {
      // 后端诊断期返回 7 元组：(css_x, css_y, cursor_x, cursor_y, win_x, win_y, scale)
      // 后 5 个原始值仅 macOS 排查穿透回归用，确认完公式后回退成 (x, y)。
      const [x, y, cursorX, cursorY, winX, winY, scale] =
        await invoke<[number, number, number, number, number, number, number]>('cursor_pos_in_window')
      const w = window.innerWidth
      const h = window.innerHeight
      if (x < 0 || y < 0 || x >= w || y >= h) {
        // 鼠标在窗口外，开穿透（小人挂在桌面右下，鼠标在桌面区域时应该能正常用桌面）
        if (DIAG) {
          // eslint-disable-next-line no-console
          console.log('[passthrough][diag] 判窗外穿透', {
            css: { x: x.toFixed(1), y: y.toFixed(1) },
            raw: { cursorX: cursorX.toFixed(0), cursorY: cursorY.toFixed(0), winX, winY, scale },
            viewport: { w, h },
          })
        }
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
    stopPolling()
    visCleanup?.()
    setIgnore(false)
  })
}
