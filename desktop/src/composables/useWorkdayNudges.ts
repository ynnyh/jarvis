import { useConfigStore } from '../stores/config'
import { useSharedTick } from './useSharedTick'

function todayKey(suffix: string): string {
  const d = new Date()
  const yyyymmdd = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`
  return `jarvis.nudge.${yyyymmdd}.${suffix}`
}
function hasFired(suffix: string): boolean {
  try { return localStorage.getItem(todayKey(suffix)) === '1' } catch { return false }
}
function markFired(suffix: string) {
  try { localStorage.setItem(todayKey(suffix), '1') } catch { /* localStorage 满了忽略 */ }
}
function parseHm(hm: string): number {
  const [h, m] = hm.split(':').map(Number)
  return h * 60 + m
}

interface NudgeOptions {
  onTrigger: (text: string, emoji: string) => void
}

export function useWorkdayNudges(options: NudgeOptions) {
  const configStore = useConfigStore()

  function tick() {
    if (!configStore.loaded) return
    if (!configStore.config.notifications.workdayNudges) return
    if (configStore.phase !== 'working') return
    if (configStore.isQuietHours) return

    const now = new Date()
    const minutes = now.getHours() * 60 + now.getMinutes()
    const periods = configStore.config.workSchedule.periods
      .map(p => ({ start: parseHm(p.start), end: parseHm(p.end) }))
      .sort((a, b) => a.start - b.start)
    if (periods.length === 0) return

    const u = configStore.config.userTitle
    if (periods.length >= 2) {
      const lunchAt = periods[0].end
      if (minutes >= lunchAt - 10 && minutes < lunchAt && !hasFired('lunch')) {
        markFired('lunch')
        options.onTrigger(`${u}，快到午饭点了，准备休息一下`, '🍱')
        return
      }
    }

    const lastEnd = periods[periods.length - 1].end
    if (minutes >= lastEnd - 10 && minutes < lastEnd && !hasFired('leave')) {
      markFired('leave')
      options.onTrigger(`${u}，快下班了，今天辛苦了 💼`, '🌆')
      return
    }

    const intervalMin = configStore.config.notifications.nudgeIntervalMinutes || 0
    if (intervalMin < 30) return
    const startOfDay = periods[0].start
    const elapsed = minutes - startOfDay
    if (elapsed < intervalMin) return
    const slot = Math.floor(elapsed / intervalMin)
    const slotKey = `slot-${intervalMin}-${slot}`
    if (hasFired(slotKey)) return
    markFired(slotKey)
    const r = slot % 3
    if (r === 1) options.onTrigger(`${u}，喝点水歇会儿吧`, '💧')
    else if (r === 2) options.onTrigger(`${u}，起身活动一下，转转脖子`, '🧘')
    else options.onTrigger(`${u}，做几次提肛，30 秒搞定`, '💪')
  }

  useSharedTick(tick)
}
