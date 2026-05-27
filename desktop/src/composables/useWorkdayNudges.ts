// 上班时段的随机小提示：喝水 / 起身 / 提肛 / 午饭 / 下班。
//
// 设计：
//   - 60s tick，phase==='working' 且 !isQuietHours 才工作
//   - 每个 nudge 一天只触发一次（localStorage 按日 key 去重；明天会换 key 自然重置）
//   - 时间锚点：
//     · 午饭前 10 分钟（第一个 period 结束前 10 分钟）
//     · 下班前 10 分钟（最后一个 period 结束前 10 分钟）—— 与 useEveningReminder
//       的"日终复盘"独立，它一般早 30 分钟开窗预拉数据，这里是更晚一刻的"该走了"
//   - 周期性：从上班开始算，每 nudgeIntervalMinutes 分钟触发一次，喝水/起身交替
//
// 为什么用 localStorage 而不是内存 ref：
//   小人 UI 重启 / 重载 wizard 后内存被清，午饭提示会重弹。localStorage 跨进程
//   重启保留，跨天靠 key 里带日期自动失效。

import { onMounted, onUnmounted } from 'vue'
import { useConfigStore } from '../stores/config'

const CHECK_INTERVAL = 60 * 1000

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
  /** 触发时回调，让 App.vue 决定怎么呈现（一般是 showAlert） */
  onTrigger: (text: string, emoji: string) => void
}

export function useWorkdayNudges(options: NudgeOptions) {
  const configStore = useConfigStore()
  let timer: ReturnType<typeof setInterval> | null = null

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
    // 午饭前 10 分钟 —— 只在 ≥2 个 period 时有意义（午休夹在中间）
    if (periods.length >= 2) {
      const lunchAt = periods[0].end
      if (minutes >= lunchAt - 10 && minutes < lunchAt && !hasFired('lunch')) {
        markFired('lunch')
        options.onTrigger(`${u}，快到午饭点了，准备休息一下`, '🍱')
        return
      }
    }

    // 下班前 10 分钟
    const lastEnd = periods[periods.length - 1].end
    if (minutes >= lastEnd - 10 && minutes < lastEnd && !hasFired('leave')) {
      markFired('leave')
      options.onTrigger(`${u}，快下班了，今天辛苦了 💼`, '🌆')
      return
    }

    // 周期性：从今天上班开始算累计分钟，按 interval 分槽，每个槽一次
    const intervalMin = configStore.config.notifications.nudgeIntervalMinutes || 0
    if (intervalMin < 30) return // 间隔过短当作关闭，防呆
    const startOfDay = periods[0].start
    const elapsed = minutes - startOfDay
    if (elapsed < intervalMin) return // 第一个槽还没到
    const slot = Math.floor(elapsed / intervalMin) // 1,2,3,...
    const slotKey = `slot-${intervalMin}-${slot}`
    if (hasFired(slotKey)) return
    markFired(slotKey)
    // 喝水 / 起身 / 提肛 三选一循环
    const r = slot % 3
    if (r === 1) {
      options.onTrigger(`${u}，喝点水歇会儿吧`, '💧')
    } else if (r === 2) {
      options.onTrigger(`${u}，起身活动一下，转转脖子`, '🧘')
    } else {
      options.onTrigger(`${u}，做几次提肛，30 秒搞定`, '💪')
    }
  }

  onMounted(() => {
    // 启动后稍等再首查；避开 wizard / 启动气泡同时弹的拥挤时刻
    setTimeout(tick, 8000)
    timer = setInterval(tick, CHECK_INTERVAL)
  })

  onUnmounted(() => {
    if (timer) clearInterval(timer)
    timer = null
  })
}
