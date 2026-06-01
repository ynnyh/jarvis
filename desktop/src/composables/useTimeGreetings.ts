import { useConfigStore } from '../stores/config'
import { useSharedTick } from './useSharedTick'

function todayKey(suffix: string): string {
  const d = new Date()
  const ymd = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`
  return `jarvis.greet.${ymd}.${suffix}`
}
function hasFired(suffix: string): boolean {
  try { return localStorage.getItem(todayKey(suffix)) === '1' } catch { return false }
}
function markFired(suffix: string) {
  try { localStorage.setItem(todayKey(suffix), '1') } catch { /* localStorage 满了忽略 */ }
}

export type GreetingState = 'morning' | 'coffee' | 'late'

interface Slot {
  id: string
  startMin: number
  endMin: number
  build: (u: string) => { text: string; emoji: string; state: GreetingState }
}

const SLOTS: Slot[] = [
  {
    id: 'morning', startMin: 7 * 60 + 30, endMin: 9 * 60,
    build: (u) => ({ text: `${u}，早安，新的一天加油 ✨`, emoji: '🌅', state: 'morning' }),
  },
  {
    id: 'coffee', startMin: 15 * 60, endMin: 16 * 60,
    build: (u) => ({ text: `${u}，喝杯咖啡放松下`, emoji: '☕', state: 'coffee' }),
  },
  {
    id: 'late', startMin: 22 * 60, endMin: 23 * 60 + 30,
    build: (u) => ({ text: `${u}，该休息了，别熬太晚`, emoji: '🌙', state: 'late' }),
  },
]

interface Options {
  onTrigger: (text: string, emoji: string, state: GreetingState) => void
}

export function useTimeGreetings(options: Options) {
  const configStore = useConfigStore()

  function tick() {
    if (!configStore.loaded) return
    const now = new Date()
    const minutes = now.getHours() * 60 + now.getMinutes()
    const u = configStore.config.userTitle
    for (const s of SLOTS) {
      if (minutes < s.startMin || minutes >= s.endMin) continue
      if (hasFired(s.id)) continue
      markFired(s.id)
      const built = s.build(u)
      options.onTrigger(built.text, built.emoji, built.state)
      return
    }
  }

  useSharedTick(tick)
}
