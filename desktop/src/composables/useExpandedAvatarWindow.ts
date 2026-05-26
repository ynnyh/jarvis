// 临时撑大 avatar 窗口的 composable。
//
// avatar 默认 400×560，对于"有较多交互/文本"的浮层（绑定窗、写工时编辑器）
// 太挤了，字也小。用这个 hook 在浮层 active 期间把窗口撑到 640×720，关闭
// 时按浮层关闭时的右下角位置还原回去，让窗口"向左上扩张、右下角不动"，
// 避免视觉跳位。
//
// 同时支持多个浮层并存（引用计数），全部关闭才真正缩回去。

import { watch, type Ref } from 'vue'
import {
  getCurrentWindow,
  LogicalSize,
  LogicalPosition,
} from '@tauri-apps/api/window'

const ORIGINAL = { w: 400, h: 560 }
const EXPANDED = { w: 640, h: 720 }

let expandCount = 0

async function setLogical(w: number, h: number) {
  const win = getCurrentWindow()
  const scale = await win.scaleFactor()
  const pos = await win.outerPosition()
  const size = await win.outerSize()
  // 用"当前"右下角，避免用户在扩窗期间拖动后还原位置错乱
  const brX = (pos.x + size.width) / scale
  const brY = (pos.y + size.height) / scale
  await win.setSize(new LogicalSize(w, h))
  await win.setPosition(new LogicalPosition(brX - w, brY - h))
}

async function doExpand() {
  expandCount++
  if (expandCount > 1) return // 已经在扩张态，直接计数
  try {
    await setLogical(EXPANDED.w, EXPANDED.h)
  } catch (e) {
    console.warn('[expand-avatar] 扩张失败', e)
  }
}

async function doRestore() {
  expandCount = Math.max(0, expandCount - 1)
  if (expandCount > 0) return // 还有别的浮层在用扩张态
  try {
    await setLogical(ORIGINAL.w, ORIGINAL.h)
  } catch (e) {
    console.warn('[expand-avatar] 还原失败', e)
  }
}

/**
 * 传一个 boolean ref，true 期间窗口撑大。组件卸载时若 ref 仍为 true，
 * 会自动 doRestore 一次（防止泄漏）。
 *
 * 注意：传 computed(() => store.foo) 比 toRef(store, 'foo') 更稳，
 * 因为后者在 Pinia 的 reactive proxy 下偶发不触发 watch。
 */
export function useExpandedAvatarWindow(active: Ref<boolean>) {
  let triggered = false

  watch(
    active,
    async (val) => {
      if (val && !triggered) {
        triggered = true
        await doExpand()
      } else if (!val && triggered) {
        triggered = false
        await doRestore()
      }
    },
    { immediate: true },
  )
}
