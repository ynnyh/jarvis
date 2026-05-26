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
 * 标记可交互元素：在 root 元素（小人、菜单、气泡、各 panel）上加 class="pointer-target"。
 */
const POLL_INTERVAL = 120
const SELECTOR = '.pointer-target'

export interface PassthroughDebug {
  /** Rust 给的窗口本地 CSS x（除过 scale） */
  x: number
  y: number
  /** Rust 给的原始物理 cursor.x、win_pos.x */
  rawCursorX: number
  rawCursorY: number
  rawWinX: number
  rawWinY: number
  scale: number
  innerW: number
  innerH: number
  elTag: string
  onUI: boolean
  ignoring: boolean | null
  err: string
}

export function useCursorPassthrough(debug?: (info: PassthroughDebug) => void) {
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
    const info: PassthroughDebug = {
      x: 0, y: 0,
      rawCursorX: 0, rawCursorY: 0,
      rawWinX: 0, rawWinY: 0, scale: 1,
      innerW: window.innerWidth, innerH: window.innerHeight,
      elTag: '-', onUI: false, ignoring: isIgnoring, err: '',
    }
    try {
      // cursor_pos_in_window 返回 6 元 tuple：[x, y, rawCursorX, rawCursorY, rawWinX, rawWinY, scale]
      // 老版本只返回 [x, y]，向后兼容：长度 < 7 时把 raw 字段填 0
      const raw = await invoke<number[]>('cursor_pos_in_window')
      const [x, y, rcx = 0, rcy = 0, rwx = 0, rwy = 0, sc = 1] = raw
      info.x = x; info.y = y
      info.rawCursorX = rcx; info.rawCursorY = rcy
      info.rawWinX = rwx; info.rawWinY = rwy
      info.scale = sc

      const w = window.innerWidth
      const h = window.innerHeight
      if (x < 0 || y < 0 || x >= w || y >= h) {
        // 鼠标在窗口外
        info.elTag = '(outside)'
        debug?.(info)
        return
      }

      const el = document.elementFromPoint(x, y)
      info.elTag = el ? `${el.tagName.toLowerCase()}${el.className ? '.' + String(el.className).split(' ')[0] : ''}` : '(null)'
      const onUI = !!(el && el.closest && el.closest(SELECTOR))
      info.onUI = onUI
      await setIgnore(!onUI)
      info.ignoring = isIgnoring
      debug?.(info)
    } catch (e: any) {
      info.err = e?.message ?? String(e)
      console.error('[passthrough] cursor_pos_in_window 失败：', e)
      await setIgnore(false)
      info.ignoring = isIgnoring
      debug?.(info)
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
