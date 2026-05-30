import { onMounted, onUnmounted } from 'vue'
import { Cron } from 'croner'
import { invoke } from '@tauri-apps/api/core'
import { useConfigStore } from '../stores/config'

interface UseScheduledRemindersOptions {
  onFire: (message: string) => void
}

export function useScheduledReminders(options: UseScheduledRemindersOptions) {
  const { onFire } = options
  let timer: ReturnType<typeof setInterval> | null = null
  const firedKeys = new Set<string>()

  function tick() {
    const configStore = useConfigStore()
    if (!configStore.loaded) return
    const reminders = configStore.config.reminders
    if (!reminders || reminders.length === 0) return

    const now = new Date()
    const dateKey = `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, '0')}-${String(now.getDate()).padStart(2, '0')} ${String(now.getHours()).padStart(2, '0')}:${String(now.getMinutes()).padStart(2, '0')}`

    // 清理非当前分钟的去重 key：dedupKey 精确到分钟，过了这一分钟就不会再匹配，
    // 否则 firedKeys 会随运行时长无限增长。
    for (const k of firedKeys) {
      if (!k.endsWith(dateKey)) firedKeys.delete(k)
    }

    for (const r of reminders) {
      if (!r.enabled) continue
      const dedupKey = `jarvis.reminder.${r.id}.${dateKey}`
      if (firedKeys.has(dedupKey)) continue

      try {
        const job = new Cron(r.cron)
        if (job.match(now)) {
          firedKeys.add(dedupKey)
          onFire(r.message)
          invoke('channels_notify', { text: `⏰ 定时提醒：${r.message}` }).catch(() => {})
        }
      } catch {
        // cron 表达式无效，跳过
      }
    }
  }

  onMounted(() => {
    setTimeout(() => {
      tick()
      timer = setInterval(tick, 30_000)
    }, 10_000)
  })

  onUnmounted(() => {
    if (timer) { clearInterval(timer); timer = null }
  })
}
