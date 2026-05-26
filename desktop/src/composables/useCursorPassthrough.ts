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

  onMounted(() => {
    setIgnore(false)
    timer = setInterval(tick, POLL_INTERVAL)
  })

  onUnmounted(() => {
    if (timer) clearInterval(timer)
    timer = null
    setIgnore(false)
  })
}
