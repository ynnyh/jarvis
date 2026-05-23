import { onMounted, onUnmounted } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'

/**
 * 让小人窗口的空白区域真穿透到桌面。
 *
 * 工作原理（受 Tauri/Webview2 限制做的妥协方案）：
 *   - DOM 通过 mousemove 检测鼠标进入/离开"可交互元素"（class .pointer-target），
 *     即时调 setIgnoreCursorEvents。
 *   - 穿透状态下，DOM 收不到任何鼠标事件（包括 mousemove），无法主动检测
 *     "鼠标又回到 UI 上"——所以 150ms 一次 probe：临时取消穿透，让 CSS :hover
 *     同步当前鼠标位置，再决定是否要保持非穿透或设回穿透。
 *   - mousedown 期间禁用切换，避免拖动窗口（drag_window）时穿透状态意外变化。
 *
 * 标记可交互元素：在 root 元素（小人、菜单、气泡、各 panel）上加 class="pointer-target"。
 * 同时给元素加 ":hover" 触发的 CSS 不影响样式即可（默认 `:hover` 总会生效）。
 */
const PROBE_INTERVAL = 150
const SELECTOR = '.pointer-target'

export function useCursorPassthrough() {
  const win = getCurrentWindow()
  let isIgnoring = false
  let mouseIsDown = false
  let probeTimer: ReturnType<typeof setInterval> | null = null
  let probing = false

  async function setIgnore(value: boolean) {
    if (value === isIgnoring) return
    try {
      await win.setIgnoreCursorEvents(value)
      isIgnoring = value
    } catch (e) {
      // 历史教训：这里被静默吞过 —— 当时 capabilities/default.json 漏了
      // core:window:allow-set-ignore-cursor-events，整套穿透逻辑全部失效但
      // 看上去毫无报错。出问题要 console 报，别再让人查半天死角。
      console.error('[passthrough] setIgnoreCursorEvents 失败：', e)
    }
  }

  function isOnUI(target: EventTarget | null): boolean {
    if (!target || !(target instanceof HTMLElement)) return false
    return target.closest(SELECTOR) !== null
  }

  function onMouseMove(e: MouseEvent) {
    if (mouseIsDown) return
    setIgnore(!isOnUI(e.target))
  }

  function onMouseDown() {
    mouseIsDown = true
  }

  function onMouseUp() {
    mouseIsDown = false
  }

  async function probe() {
    if (!isIgnoring || mouseIsDown || probing) return
    probing = true
    try {
      // 临时取消穿透，让 CSS :hover 同步当前鼠标位置
      await win.setIgnoreCursorEvents(false)
      // 等两帧确保 hover 状态生效
      await new Promise(r => requestAnimationFrame(r))
      await new Promise(r => requestAnimationFrame(r))
      const onUI = document.querySelector(`${SELECTOR}:hover`) !== null
      if (onUI) {
        // 鼠标确实在 UI 上，保持非穿透
        isIgnoring = false
      } else {
        // 还是空白，设回穿透
        await win.setIgnoreCursorEvents(true)
      }
    } catch {
      // ignore
    } finally {
      probing = false
    }
  }

  onMounted(() => {
    document.addEventListener('mousemove', onMouseMove)
    document.addEventListener('mousedown', onMouseDown)
    document.addEventListener('mouseup', onMouseUp)
    probeTimer = setInterval(probe, PROBE_INTERVAL)
  })

  onUnmounted(() => {
    document.removeEventListener('mousemove', onMouseMove)
    document.removeEventListener('mousedown', onMouseDown)
    document.removeEventListener('mouseup', onMouseUp)
    if (probeTimer) clearInterval(probeTimer)
    // 关闭穿透避免遗留状态
    setIgnore(false)
  })
}
