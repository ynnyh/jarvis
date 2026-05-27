import { invoke } from '@tauri-apps/api/core'
import { useAppStore, type DailyReviewData } from '../stores/app'
import { useConfigStore } from '../stores/config'

interface ToolResult {
  success: boolean
  data?: any
  error?: string
}

function unpack(result: ToolResult): DailyReviewData | null {
  if (!result?.success) return null
  const data = result.data
  if (!data) return null
  if (data.plainText && data.summary) return data as DailyReviewData
  if (typeof data.output === 'string') {
    const start = data.output.indexOf('{')
    if (start < 0) return null
    try {
      return JSON.parse(data.output.slice(start)) as DailyReviewData
    } catch {
      return null
    }
  }
  return null
}

export type ReviewRange = 'today' | 'yesterday' | 'thisWeek'

export function useDailyReview() {
  const store = useAppStore()
  const configStore = useConfigStore()

  async function fetchReview(range: ReviewRange = 'today') {
    if (store.reviewLoading) return
    store.reviewLoading = true
    try {
      const raw = await invoke<ToolResult>('tool_execute', {
        name: 'get_daily_review',
        input: {
          range,
          // 把用户配置的日工时传给后端，用来做工时反推
          hoursPerWorkDay: configStore.hoursPerWorkDay,
        },
      })
      const payload = unpack(raw)
      if (!payload) {
        store.reviewLastError = raw?.error || 'get_daily_review 返回为空'
        store.reviewLoaded = true
        return
      }
      store.reviewData = payload
      store.reviewLastError = null
      store.reviewLoaded = true
    } catch (e) {
      store.reviewLastError = e instanceof Error ? e.message : String(e)
      store.reviewLoaded = true
    } finally {
      store.reviewLoading = false
    }
  }

  async function openReview(range: ReviewRange = 'today') {
    store.showReviewWindow = true
    if (!store.reviewData || store.reviewData.range.label !== '') {
      await fetchReview(range)
    }
  }

  async function copyPlainText(): Promise<boolean> {
    const text = store.reviewData?.plainText
    if (!text) return false
    try {
      await navigator.clipboard.writeText(text)
      return true
    } catch {
      return false
    }
  }

  return {
    fetchReview,
    openReview,
    copyPlainText,
  }
}
