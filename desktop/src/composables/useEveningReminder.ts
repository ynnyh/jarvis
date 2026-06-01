import { invoke } from '@tauri-apps/api/core'
import { useAppStore } from '../stores/app'
import { useConfigStore } from '../stores/config'
import { useDailyReview } from './useDailyReview'
import { useSharedTick } from './useSharedTick'

function todayStr(): string {
  const d = new Date()
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`
}

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
    if (mUntilEnd <= 0) return
    if (mUntilEnd > before) return
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
