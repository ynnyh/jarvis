import { onMounted, onUnmounted } from 'vue'
import { useAppStore } from '../stores/app'
import { useConfigStore } from '../stores/config'

const CHECK_INTERVAL = 60 * 1000 // 每分钟检查一次

function todayStr(): string {
  const d = new Date()
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`
}

function nowHHMM(): string {
  const d = new Date()
  return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`
}

interface Options {
  /** 触发时回调，通常用来弹气泡引导用户定今日计划 */
  onTrigger: () => void
}

/**
 * 早上到点（默认 09:10）主动提醒"定今日计划"。
 *
 * 逻辑同 useEveningReminder：每分钟检查、仅工作日、当天只触发一次（store.todayPlanPromptedOn）。
 * 区别是触发条件为"当前时间已过 todayPlanPromptTime"——晚开机（如 10 点才开）也会补触发一次。
 */
export function useTodayPlanPrompt(options: Options) {
  const store = useAppStore()
  const configStore = useConfigStore()
  let timer: ReturnType<typeof setInterval> | null = null

  function maybeTrigger() {
    if (!configStore.loaded) return
    if (!configStore.config.notifications.todayPlanPromptEnabled) return

    // 非工作日（含 dayoff、weekend）不触发
    const phase = configStore.phase
    if (phase === 'weekend' || phase === 'dayoff') return

    // 当天已触发过
    const today = todayStr()
    if (store.todayPlanPromptedOn === today) return

    // 到达配置的提示时间（HH:MM 零填充，可直接字典序比较）才触发
    const target = (configStore.config.notifications.todayPlanPromptTime || '09:10').trim()
    if (nowHHMM() < target) return

    store.todayPlanPromptedOn = today
    options.onTrigger()
  }

  onMounted(() => {
    // 启动后稍延迟检查一次（防止恰好启动时已过提示时间）
    setTimeout(maybeTrigger, 5000)
    timer = setInterval(maybeTrigger, CHECK_INTERVAL)
  })

  onUnmounted(() => {
    if (timer) clearInterval(timer)
    timer = null
  })
}
