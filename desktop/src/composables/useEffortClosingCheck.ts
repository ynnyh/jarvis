import { invoke } from '@tauri-apps/api/core'
import { useConfigStore } from '../stores/config'
import { useSharedTick } from './useSharedTick'

interface ToolResult {
  success: boolean
  data?: { totalHours?: number; count?: number; begin?: string; end?: string }
  error?: string
}

interface Options {
  onReminder: (text: string, emoji: string) => void
  onError?: (text: string, emoji: string) => void
}

function todayStr(): string {
  const d = new Date()
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`
}
function todayKey(suffix: string): string {
  return `jarvis.effortClosing.${todayStr()}.${suffix}`
}
function getFlag(suffix: string): string | null {
  try { return localStorage.getItem(todayKey(suffix)) } catch { return null }
}
function setFlag(suffix: string, value: string) {
  try { localStorage.setItem(todayKey(suffix), value) } catch { /* ignore storage quota */ }
}

export function ignoreTodayEffortClosing() { setFlag('ignored', '1') }

function parseHm(hm: string): number | null {
  const match = /^(\d{1,2}):(\d{2})$/.exec((hm || '').trim())
  if (!match) return null
  const h = Number(match[1]), m = Number(match[2])
  if (!Number.isFinite(h) || !Number.isFinite(m) || h < 0 || h > 23 || m < 0 || m > 59) return null
  return h * 60 + m
}
function currentMinutes(): number {
  const now = new Date()
  return now.getHours() * 60 + now.getMinutes()
}
function endOfWorkdayMinutes(configStore: ReturnType<typeof useConfigStore>): number | null {
  const ends = configStore.config.workSchedule.periods
    .map(p => parseHm(p.end)).filter((v): v is number => v !== null).sort((a, b) => a - b)
  return ends.length ? ends[ends.length - 1] : null
}
function shouldSkipDay(configStore: ReturnType<typeof useConfigStore>): boolean {
  return configStore.phase === 'weekend' || configStore.phase === 'dayoff'
}
function shouldRepeat(configStore: ReturnType<typeof useConfigStore>, nowMinutes: number): boolean {
  const repeat = Number(configStore.config.notifications.effortClosingRepeatMinutes || 0)
  if (repeat < 15) return false
  const last = Number(getFlag('lastReminderAt') || 0)
  if (!last) return true
  return Date.now() - last >= repeat * 60 * 1000 && nowMinutes <= latestReminderMinutes(configStore)
}
function latestReminderMinutes(configStore: ReturnType<typeof useConfigStore>): number {
  return parseHm(configStore.config.notifications.effortClosingLatestTime) ?? 21 * 60
}

export function useEffortClosingCheck(options: Options) {
  const configStore = useConfigStore()
  let checking = false

  async function notifyChannels(text: string) {
    if (!configStore.config.notifications.effortClosingChannelNotify) return
    try { await invoke('channels_notify', { text }) } catch { /* ignore */ }
  }

  async function tick() {
    if (checking) return
    if (!configStore.loaded) return
    const n = configStore.config.notifications
    if (!n.effortClosingCheck) return
    if (shouldSkipDay(configStore)) return
    if (!configStore.config.fineReport.realName?.trim()) return
    if (getFlag('ignored') === '1') return
    const lastEnd = endOfWorkdayMinutes(configStore)
    if (lastEnd === null) return
    const now = currentMinutes()
    const firstCheckAt = lastEnd + Number(n.effortClosingMinutesAfterWork || 10)
    if (now < firstCheckAt) return
    if (now > latestReminderMinutes(configStore)) return
    const alreadyMet = getFlag('met') === '1'
    if (alreadyMet) return
    const reminded = getFlag('reminded') === '1'
    if (reminded && !shouldRepeat(configStore, now)) return
    checking = true
    try {
      const result = await invoke<ToolResult>('tool_execute', {
        name: 'get_efforts', input: { range: 'today' },
      })
      if (!result.success) {
        if (!reminded) {
          setFlag('reminded', '1')
          setFlag('lastReminderAt', String(Date.now()))
          options.onError?.(`工时查询失败：${result.error || '未知错误'}`, '⚠️')
        }
        return
      }
      const total = Number(result.data?.totalHours || 0)
      const target = Number(n.effortClosingTargetHours || 8)
      if (total >= target) { setFlag('met', '1'); return }
      const missing = Math.max(0, target - total)
      setFlag('reminded', '1')
      setFlag('lastReminderAt', String(Date.now()))
      const text = `${configStore.config.userTitle}，今天已登记 ${total.toFixed(1)}h，还差 ${missing.toFixed(1)}h，要不要补一下工时？`
      options.onReminder(text, '⏱️')
      await notifyChannels(text)
    } finally { checking = false }
  }

  useSharedTick(tick)
}
