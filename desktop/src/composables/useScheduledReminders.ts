import { Cron } from 'croner'
import { invoke } from '@tauri-apps/api/core'
import { useConfigStore } from '../stores/config'
import { useSharedTick } from './useSharedTick'

interface UseScheduledRemindersOptions {
  onFire: (message: string) => void
}

export function useScheduledReminders(options: UseScheduledRemindersOptions) {
  const { onFire } = options
  const firedKeys = new Set<string>()

  function tick() {
    const configStore = useConfigStore()
    if (!configStore.loaded) return
    const reminders = configStore.config.reminders
    if (!reminders || reminders.length === 0) return

    const now = new Date()
    const dateKey = `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, '0')}-${String(now.getDate()).padStart(2, '0')} ${String(now.getHours()).padStart(2, '0')}:${String(now.getMinutes()).padStart(2, '0')}`

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
      } catch { /* cron 表达式无效，跳过 */ }
    }
  }

  useSharedTick(tick)
}
