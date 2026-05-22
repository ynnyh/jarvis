import { onMounted, onUnmounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from '@tauri-apps/plugin-notification'
import { useAppStore } from '../stores/app'

interface TaskAlertRaw {
  id: string
  title: string
  deadline: string
  assignee: string
  alert_type: string
  days_until_due: number
  status: string
  priority: string
  estimated_hours: number
  consumed_hours: number
  left_hours: number
  is_team: boolean
}

const POLL_INTERVAL = 5 * 60 * 1000 // 5 minutes

// 已通知过的 (taskId|alertType) 组合，避免重复打扰
const notified = new Set<string>()

async function ensureNotificationPermission(): Promise<boolean> {
  try {
    let granted = await isPermissionGranted()
    if (!granted) {
      const r = await requestPermission()
      granted = r === 'granted'
    }
    return granted
  } catch {
    return false
  }
}

export function useTaskAlerts() {
  const store = useAppStore()
  let timer: ReturnType<typeof setInterval> | null = null
  let permissionGranted = false
  let isFirstFetch = true

  async function fetchAlerts() {
    try {
      const alerts = await invoke<TaskAlertRaw[]>('fetch_task_alerts')
      const mapped = alerts.map(a => ({
        id: a.id,
        title: a.title,
        deadline: a.deadline,
        assignee: a.assignee,
        alertType: a.alert_type as 'overdue' | 'today' | 'soon' | 'upcoming',
        daysUntilDue: a.days_until_due,
        status: a.status as 'wait' | 'doing',
        priority: a.priority as 'low' | 'normal' | 'high' | 'urgent',
        estimatedHours: a.estimated_hours,
        consumedHours: a.consumed_hours,
        leftHours: a.left_hours,
        isTeam: a.is_team,
      }))

      // 通知逻辑：只在非首次拉取、并且有权限时通知"新增的"高优先级提醒
      // 首次只把现有提醒标记成"已通知"，避免每次启动桌面都炸消息
      if (permissionGranted && !isFirstFetch) {
        const fresh = mapped.filter(a =>
          (a.alertType === 'overdue' || a.alertType === 'today' || a.alertType === 'soon') &&
          !notified.has(`${a.id}|${a.alertType}`)
        )
        for (const a of fresh) {
          const prefix = a.alertType === 'overdue'
            ? `🔥 逾期 ${-a.daysUntilDue} 天`
            : a.alertType === 'today'
              ? '⏰ 今天到期'
              : `⚡ ${a.daysUntilDue} 天后到期`
          sendNotification({
            title: `Jarvis · ${prefix}`,
            body: a.title,
          })
        }
      }
      // 不管是不是首次，都把所有当前提醒记入已通知集合，避免下次重复
      for (const a of mapped) notified.add(`${a.id}|${a.alertType}`)
      isFirstFetch = false

      store.taskAlerts = mapped
      store.alertsLastError = null
      store.alertsLoaded = true
    } catch (e) {
      store.alertsLastError = e instanceof Error ? e.message : String(e)
      store.alertsLoaded = true
    }
  }

  onMounted(async () => {
    permissionGranted = await ensureNotificationPermission()
    fetchAlerts()
    timer = setInterval(fetchAlerts, POLL_INTERVAL)
  })

  onUnmounted(() => {
    if (timer) {
      clearInterval(timer)
      timer = null
    }
  })

  return {
    refresh: fetchAlerts,
  }
}
