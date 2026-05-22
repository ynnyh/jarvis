import { onMounted, onUnmounted } from 'vue'

export function usePolling(callback: () => void | Promise<void>, intervalMs: number = 600000) {
  let timer: ReturnType<typeof setInterval> | null = null

  onMounted(() => {
    callback()
    timer = setInterval(() => {
      callback()
    }, intervalMs)
  })

  onUnmounted(() => {
    if (timer) {
      clearInterval(timer)
      timer = null
    }
  })
}
