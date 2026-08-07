import { invoke } from '@tauri-apps/api/core'
import { useAppStore } from '../stores/app'
import { useConfigStore } from '../stores/config'
import { useDailyReview } from './useDailyReview'
import { useSharedTick } from './useSharedTick'
import { todayStr } from './dailyFired'

interface Options {
  onTrigger: () => void
}

export function useEveningReminder(options: Options) {
  const store = useAppStore()
  const configStore = useConfigStore()
  const { fetchReview } = useDailyReview()

  function maybeTrigger() {
    if (!configStore.loaded) return
    if (!configStore.config.notifications.eveningSummary) return
    const phase = configStore.phase
    if (phase === 'weekend' || phase === 'dayoff') return
    const today = todayStr()
    if (store.reviewTriggeredOn === today) return
    const before = configStore.config.notifications.eveningSummaryMinutesBefore
    const mUntilEnd = configStore.minutesUntilEndOfDay
    // 统一的触发窗口语义：
    //   before > 0 → 下班前 before 分钟内触发（窗口大小 = before，即 0 < mUntilEnd <= before）
    //   before <= 0 → 下班时间点或下班后 |before| 分钟，以 2 分钟窗口触发以保证可靠性
    //     例：before = 0   → 下班时间点附近触发  (mUntilEnd ∈ (-2, 0])
    //     例：before = -10 → 下班后 10 分钟触发 (mUntilEnd ∈ (-12, -10])
    const windowSize = before >= 1 ? before : 2
    const hasPassedTarget = mUntilEnd <= before
    const stillWithinWindow = mUntilEnd > before - windowSize
    if (!hasPassedTarget || !stillWithinWindow) return
    store.reviewTriggeredOn = today
    fetchReview('today').then(() => {
      options.onTrigger()
      // 推送到手机渠道
      pushReviewToChannels()
    })
  }

  async function pushReviewToChannels() {
    if (!configStore.config.notifications.eveningSummaryChannelNotify) return
    const text = store.reviewData?.plainText
    if (!text) return
    try {
      await invoke('channels_notify', {
        text: `📋 ${todayStr()} 工作复盘\n\n${text}`,
      })
    } catch {
      // 渠道未运行或未配置，静默忽略
    }
  }

  useSharedTick(maybeTrigger)
}
