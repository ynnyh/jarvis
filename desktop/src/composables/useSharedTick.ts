/**
 * 共享 Tick 调度器。
 *
 * 所有通知/提醒 composable 注册到此，共用同一个 30s 定时器。
 * 避免 App.vue 里 6 个独立 setInterval 同时跑。
 */

import { onMounted, onUnmounted } from 'vue'

const TICK_INTERVAL = 30_000
const INITIAL_DELAY = 10_000

type TickFn = () => void

let sharedTimer: ReturnType<typeof setInterval> | null = null
const subscribers = new Set<TickFn>()

function fireAll() {
  for (const fn of subscribers) {
    try { fn() } catch { /* 单个回调异常不影响其他 */ }
  }
}

export function useSharedTick(fn: TickFn) {
  onMounted(() => {
    subscribers.add(fn)
    if (!sharedTimer) {
      setTimeout(() => {
        fireAll()
        sharedTimer = setInterval(fireAll, TICK_INTERVAL)
      }, INITIAL_DELAY)
    }
  })

  onUnmounted(() => {
    subscribers.delete(fn)
    if (subscribers.size === 0 && sharedTimer) {
      clearInterval(sharedTimer)
      sharedTimer = null
    }
  })
}
