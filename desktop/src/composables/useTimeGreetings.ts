// 按时间段切换小人状态 + 弹气泡：早晨"早安"、下午"咖啡时间"、夜晚"该休息了"。
//
// 跟 useWorkdayNudges 的区别：
//   - nudges 是工作时段的健康提醒（喝水/起身/提肛），受 workdayNudges 开关、
//     phase==='working'、quiet 时段控制
//   - greetings 是日内固定时段的"友好问候"，不算打扰，所以**忽略 quiet 和 phase**，
//     让用户在早上/咖啡时间/晚上都能感受到小人的存在
//
// 每天每个时段只触发一次（localStorage 按日 key 去重），跨天自动失效。

import { onMounted, onUnmounted } from 'vue'
import { useConfigStore } from '../stores/config'

const CHECK_INTERVAL = 60 * 1000

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
  startMin: number  // HHMM 转分钟
  endMin: number
  build: (u: string) => { text: string; emoji: string; state: GreetingState }
}

const SLOTS: Slot[] = [
  {
    id: 'morning',
    startMin: 7 * 60 + 30, endMin: 9 * 60,
    build: (u) => ({ text: `${u}，早安，新的一天加油 ✨`, emoji: '🌅', state: 'morning' }),
  },
  {
    id: 'coffee',
    startMin: 15 * 60, endMin: 16 * 60,
    build: (u) => ({ text: `${u}，喝杯咖啡放松下`, emoji: '☕', state: 'coffee' }),
  },
  {
    id: 'late',
    startMin: 22 * 60, endMin: 23 * 60 + 30,
    build: (u) => ({ text: `${u}，该休息了，别熬太晚`, emoji: '🌙', state: 'late' }),
  },
]

interface Options {
  onTrigger: (text: string, emoji: string, state: GreetingState) => void
}

export function useTimeGreetings(options: Options) {
  const configStore = useConfigStore()
  let timer: ReturnType<typeof setInterval> | null = null

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

  onMounted(() => {
    // 启动短延迟首查，避开 wizard / 启动 toast 同时弹
    setTimeout(tick, 7000)
    timer = setInterval(tick, CHECK_INTERVAL)
  })

  onUnmounted(() => {
    if (timer) clearInterval(timer)
    timer = null
  })
}
