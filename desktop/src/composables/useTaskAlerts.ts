import { invoke } from '@tauri-apps/api/core'
import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from '@tauri-apps/plugin-notification'
import { useAppStore } from '../stores/app'
import { useConfigStore } from '../stores/config'
import { useSharedTick } from './useSharedTick'

interface TaskAlertRaw {
  id: string; title: string; deadline: string; assignee: string
  alert_type: string; days_until_due: number; status: string; priority: string
  estimated_hours: number; consumed_hours: number; left_hours: number; is_team: boolean
}

/** 每 5 分钟拉一次禅道即可，防频繁请求 */
const THROTTLE_MS = 5 * 60 * 1000

const notified = new Set<string>()

async function ensureNotificationPermission(): Promise<boolean> {
  try {
    let granted = await isPermissionGranted()
    if (!granted) {
      const r = await requestPermission()
      granted = r === 'granted'
    }
    return granted
  } catch { return false }
}

export function useTaskAlerts() {
  const store = useAppStore()
  const configStore = useConfigStore()
  let permissionGranted = false
  let isFirstFetch = true
  let lastFetch = 0

  function isConfigReady(): boolean {
    if (!configStore.loaded) return false
    const z = configStore.config.zentao
    return !!(z.baseUrl?.trim() && z.account?.trim())
  }

  async function fetchAlerts() {
    if (!isConfigReady()) return
    const now = Date.now()
    if (now - lastFetch < THROTTLE_MS) return
    lastFetch = now
    try {
      const alerts = await invoke<TaskAlertRaw[]>('fetch_task_alerts')
      const mapped = alerts.map(a => ({
        id: a.id, title: a.title, deadline: a.deadline, assignee: a.assignee,
        alertType: a.alert_type as 'overdue' | 'today' | 'soon' | 'upcoming',
        daysUntilDue: a.days_until_due,
        status: a.status as 'wait' | 'doing',
        priority: a.priority as 'low' | 'normal' | 'high' | 'urgent',
        estimatedHours: a.estimated_hours, consumedHours: a.consumed_hours,
        leftHours: a.left_hours, isTeam: a.is_team,
      }))

      if (permissionGranted && !isFirstFetch) {
        const fresh = mapped.filter(a =>
          (a.alertType === 'overdue' || a.alertType === 'today' || a.alertType === 'soon') &&
          !notified.has(`${a.id}|${a.alertType}`)
        )
        for (const a of fresh) {
          const prefix = a.alertType === 'overdue'
            ? `🔥 逾期 ${-a.daysUntilDue} 天`
            : a.alertType === 'today' ? '⏰ 今天到期' : `⚡ ${a.daysUntilDue} 天后到期`
          sendNotification({ title: `${configStore.config.assistantName} · ${prefix}`, body: a.title })
        }
      }
      const currentKeys = new Set(mapped.map(a => `${a.id}|${a.alertType}`))
      for (const k of notified) { if (!currentKeys.has(k)) notified.delete(k) }
      for (const k of currentKeys) notified.add(k)
      isFirstFetch = false
      store.taskAlerts = mapped
      store.alertsLastError = null
      store.alertsLoaded = true
    } catch (e) {
      store.alertsLastError = e instanceof Error ? e.message : String(e)
      store.alertsLoaded = true
    }
  }

  useSharedTick(fetchAlerts)

  return { refresh: fetchAlerts }
}
