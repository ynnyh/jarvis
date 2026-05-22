import { defineStore } from 'pinia'
import { ref, computed } from 'vue'

export interface Notification {
  id: string
  type: 'risk' | 'agent' | 'git' | 'ai' | 'system'
  title: string
  body: string
  priority: 'urgent' | 'normal' | 'low'
  timestamp: number
  read: boolean
  source?: string
  action?: {
    label: string
    handler: () => void
  }
}

export const useNotificationStore = defineStore('notification', () => {
  const notifications = ref<Notification[]>([])
  const maxHistory = 100

  const unreadCount = computed(() => notifications.value.filter(n => !n.read).length)
  const urgentCount = computed(() => notifications.value.filter(n => n.priority === 'urgent' && !n.read).length)

  const byType = computed(() => {
    const groups: Record<string, Notification[]> = {}
    for (const n of notifications.value) {
      if (!groups[n.type]) groups[n.type] = []
      groups[n.type].push(n)
    }
    return groups
  })

  const byPriority = computed(() => {
    const order = { urgent: 0, normal: 1, low: 2 }
    return [...notifications.value].sort(
      (a, b) => order[a.priority] - order[b.priority] || b.timestamp - a.timestamp
    )
  })

  function add(notification: Omit<Notification, 'id' | 'timestamp' | 'read'>): Notification {
    const item: Notification = {
      ...notification,
      id: `notif_${Date.now()}_${Math.random().toString(36).substr(2, 9)}`,
      timestamp: Date.now(),
      read: false,
    }

    notifications.value.unshift(item)

    // 限制历史记录
    if (notifications.value.length > maxHistory) {
      notifications.value = notifications.value.slice(0, maxHistory)
    }

    return item
  }

  function markAsRead(id: string): void {
    const notif = notifications.value.find(n => n.id === id)
    if (notif) notif.read = true
  }

  function markAllAsRead(): void {
    notifications.value.forEach(n => (n.read = true))
  }

  function remove(id: string): void {
    notifications.value = notifications.value.filter(n => n.id !== id)
  }

  function clear(): void {
    notifications.value = []
  }

  function getUnread(): Notification[] {
    return notifications.value.filter(n => !n.read)
  }

  function getRecent(limit: number = 10): Notification[] {
    return notifications.value.slice(0, limit)
  }

  return {
    notifications,
    unreadCount,
    urgentCount,
    byType,
    byPriority,
    add,
    markAsRead,
    markAllAsRead,
    remove,
    clear,
    getUnread,
    getRecent,
  }
})
