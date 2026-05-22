import { onMounted, onUnmounted } from 'vue'
import { useAppStore } from '../stores/app'
import { useConfigStore } from '../stores/config'
import { useDailyReview } from './useDailyReview'

const CHECK_INTERVAL = 60 * 1000 // 每分钟检查一次

function todayStr(): string {
  const d = new Date()
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`
}

interface Options {
  /** 触发时回调，通常用来弹气泡 + 自动打开窗口 */
  onTrigger: () => void
}

/**
 * 下班前 N 分钟主动触发复盘气泡。
 *
 * 逻辑：
 *   1. 每分钟检查一次配置的"下班时间 - eveningSummaryMinutesBefore"是否到达
 *   2. 必须是工作日（configStore.phase 不是 weekend/dayoff）
 *   3. 当天只触发一次（store.reviewTriggeredOn === todayStr() 时跳过）
 *   4. 触发后：异步预拉数据，再调用 onTrigger 回调
 */
export function useEveningReminder(options: Options) {
  const store = useAppStore()
  const configStore = useConfigStore()
  const { fetchReview } = useDailyReview()
  let timer: ReturnType<typeof setInterval> | null = null

  function maybeTrigger() {
    // 配置未加载完成或 eveningSummary 关闭则不触发
    if (!configStore.loaded) return
    if (!configStore.config.notifications.eveningSummary) return

    // 非工作日（含 dayoff、weekend）不触发
    const phase = configStore.phase
    if (phase === 'weekend' || phase === 'dayoff') return

    // 当天已触发过
    const today = todayStr()
    if (store.reviewTriggeredOn === today) return

    // 距离下班时间在 [0, eveningSummaryMinutesBefore] 之内 → 触发窗口
    const before = configStore.config.notifications.eveningSummaryMinutesBefore
    const mUntilEnd = configStore.minutesUntilEndOfDay
    if (mUntilEnd <= 0) return
    if (mUntilEnd > before) return

    // 触发
    store.reviewTriggeredOn = today
    fetchReview('today').then(() => {
      options.onTrigger()
    })
  }

  onMounted(() => {
    // 启动后立刻检查一次（防止恰好启动时已经进入触发窗口）
    setTimeout(maybeTrigger, 5000)
    timer = setInterval(maybeTrigger, CHECK_INTERVAL)
  })

  onUnmounted(() => {
    if (timer) clearInterval(timer)
    timer = null
  })
}
