import { defineStore } from 'pinia'
import { ref, computed, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'

export interface WorkPeriod {
  start: string   // HH:MM
  end: string     // HH:MM
  label: string
}

export interface JarvisConfig {
  workSchedule: {
    workDays: number[]      // 0=Sun, 1=Mon ... 6=Sat
    periods: WorkPeriod[]
  }
  notifications: {
    quietDuringLunch: boolean
    quietAfterWork: boolean
    quietOnWeekends: boolean
    morningGreeting: boolean
    eveningSummary: boolean
    eveningSummaryMinutesBefore: number
  }
  override: {
    todayMode: 'normal' | 'overtime' | 'dayoff'
    todayModeSetOn: string   // YYYY-MM-DD
  }
}

const defaultConfig = (): JarvisConfig => ({
  workSchedule: {
    workDays: [1, 2, 3, 4, 5],
    periods: [
      { start: '08:00', end: '12:00', label: '上午' },
      { start: '14:00', end: '18:00', label: '下午' },
    ],
  },
  notifications: {
    quietDuringLunch: true,
    quietAfterWork: true,
    quietOnWeekends: true,
    morningGreeting: true,
    eveningSummary: true,
    eveningSummaryMinutesBefore: 30,
  },
  override: {
    todayMode: 'normal',
    todayModeSetOn: '',
  },
})

function todayStr(): string {
  const d = new Date()
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`
}

export const useConfigStore = defineStore('config', () => {
  const config = ref<JarvisConfig>(defaultConfig())
  const loaded = ref(false)
  const showSettingsWindow = ref(false)
  let savingTimer: ReturnType<typeof setTimeout> | null = null

  async function load() {
    try {
      const remote = await invoke<JarvisConfig>('config_load')
      // 临时覆盖只在当日有效
      if (remote.override.todayModeSetOn !== todayStr()) {
        remote.override.todayMode = 'normal'
        remote.override.todayModeSetOn = ''
      }
      config.value = remote
    } catch (e) {
      console.error('加载配置失败，使用默认值:', e)
    } finally {
      loaded.value = true
    }
  }

  async function save() {
    try {
      await invoke('config_save', { config: config.value })
    } catch (e) {
      console.error('保存配置失败:', e)
    }
  }

  // 任意字段变化 250ms 防抖后写回磁盘
  watch(config, () => {
    if (!loaded.value) return
    if (savingTimer) clearTimeout(savingTimer)
    savingTimer = setTimeout(save, 250)
  }, { deep: true })

  // 临时覆盖：今晚加班 / 今天休假
  function setTodayMode(mode: JarvisConfig['override']['todayMode']) {
    config.value.override.todayMode = mode
    config.value.override.todayModeSetOn = mode === 'normal' ? '' : todayStr()
  }

  // —— 派生：当前时间上下文 ——
  // 用一个 reactive tick 让计算属性每分钟刷新
  const tick = ref(Date.now())
  setInterval(() => { tick.value = Date.now() }, 30 * 1000)

  function parseHm(hm: string): number {
    const [h, m] = hm.split(':').map(Number)
    return h * 60 + m
  }

  type Phase = 'before-work' | 'working' | 'lunch' | 'after-work' | 'weekend' | 'dayoff' | 'overtime'

  const phase = computed<Phase>(() => {
    void tick.value   // 触发响应
    const now = new Date()
    const todayMode = config.value.override.todayMode

    if (todayMode === 'dayoff') return 'dayoff'
    if (todayMode === 'overtime') {
      // 加班模式：把下班时间往后推 2 小时
    }

    const dow = now.getDay()
    const isWorkDay = config.value.workSchedule.workDays.includes(dow)
    if (!isWorkDay && todayMode !== 'overtime') return 'weekend'

    const minutes = now.getHours() * 60 + now.getMinutes()
    const periods = config.value.workSchedule.periods
      .map(p => ({ start: parseHm(p.start), end: parseHm(p.end) }))
      .sort((a, b) => a.start - b.start)

    if (periods.length === 0) return 'after-work'

    if (minutes < periods[0].start) return 'before-work'

    for (let i = 0; i < periods.length; i++) {
      if (minutes >= periods[i].start && minutes < periods[i].end) return 'working'
      if (i < periods.length - 1 && minutes >= periods[i].end && minutes < periods[i + 1].start) return 'lunch'
    }

    // 全部时段都已过：下班
    const lastEnd = periods[periods.length - 1].end
    if (todayMode === 'overtime' && minutes < lastEnd + 120) return 'working'
    return 'after-work'
  })

  const isQuietHours = computed(() => {
    const p = phase.value
    const n = config.value.notifications
    if (p === 'weekend' && n.quietOnWeekends) return true
    if (p === 'lunch' && n.quietDuringLunch) return true
    if ((p === 'after-work' || p === 'before-work') && n.quietAfterWork) return true
    if (p === 'dayoff') return true
    return false
  })

  // 离下班还有多少分钟（负数=已下班；上午/午休/下班前都返回到"今天最后一个时段结束"的差值）
  const minutesUntilEndOfDay = computed<number>(() => {
    void tick.value
    const now = new Date()
    const periods = config.value.workSchedule.periods
    if (periods.length === 0) return -1
    // 防御性排序：用户在设置里可能把 periods 顺序写乱，直接取下标 last 会把"下午到 18:00"
    // 误判成"上午到 12:00"，导致复盘提醒永远不触发或在错误时间触发。
    const sortedEnds = periods.map(p => parseHm(p.end)).sort((a, b) => a - b)
    const lastEnd = sortedEnds[sortedEnds.length - 1]
    const m = now.getHours() * 60 + now.getMinutes()
    return lastEnd - m
  })

  // 一个工作日的工时（所有 periods 时长之和）
  const hoursPerWorkDay = computed<number>(() => {
    let mins = 0
    for (const p of config.value.workSchedule.periods) {
      mins += parseHm(p.end) - parseHm(p.start)
    }
    return mins / 60
  })

  // 未来 N 天内累计可用工时（按 workDays 排除休息日，含今天）
  function availableHoursInNextDays(n: number): number {
    void tick.value
    const days = config.value.workSchedule.workDays
    const today = new Date()
    const todayMode = config.value.override.todayMode
    let hours = 0
    for (let i = 0; i < n; i++) {
      const d = new Date(today)
      d.setDate(today.getDate() + i)
      const dow = d.getDay()
      if (i === 0) {
        // 今天有 override 优先级
        if (todayMode === 'dayoff') continue
        if (days.includes(dow) || todayMode === 'overtime') {
          hours += hoursPerWorkDay.value
        }
        continue
      }
      if (days.includes(dow)) hours += hoursPerWorkDay.value
    }
    return hours
  }

  const availableHoursIn7Days = computed(() => availableHoursInNextDays(7))

  // 未来 N 天内的工作日个数
  function workingDaysInNext(n: number): number {
    const days = config.value.workSchedule.workDays
    const today = new Date()
    const todayMode = config.value.override.todayMode
    let count = 0
    for (let i = 0; i < n; i++) {
      const d = new Date(today)
      d.setDate(today.getDate() + i)
      const dow = d.getDay()
      if (i === 0) {
        if (todayMode === 'dayoff') continue
        if (days.includes(dow) || todayMode === 'overtime') count++
        continue
      }
      if (days.includes(dow)) count++
    }
    return count
  }

  const workingDaysIn7 = computed(() => workingDaysInNext(7))

  return {
    config,
    loaded,
    showSettingsWindow,
    load,
    save,
    setTodayMode,
    phase,
    isQuietHours,
    minutesUntilEndOfDay,
    hoursPerWorkDay,
    availableHoursIn7Days,
    workingDaysIn7,
    availableHoursInNextDays,
    workingDaysInNext,
  }
})
