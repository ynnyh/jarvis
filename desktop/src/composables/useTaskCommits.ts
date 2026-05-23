import { invoke } from '@tauri-apps/api/core'
import { onMounted, onUnmounted } from 'vue'
import { useAppStore, type CommitLink } from '../stores/app'
import { useConfigStore } from '../stores/config'

interface ToolResult {
  success: boolean
  data?: any
  error?: string
}

interface CommitLinkPayload {
  range: { since: string; until: string; label: string }
  scannedRepos: number
  totalCommits: number
  tasks: Array<{
    taskId: string
    taskName: string
    commits: CommitLink[]
  }>
  orphanCommits: Array<{ businessLine: string; commits: CommitLink[] }>
}

const POLL_INTERVAL = 15 * 60 * 1000 // 15 分钟（commits 比 alerts 更新更慢，无需高频）
const FIRST_FETCH_DELAY = 2_000 // 让 alerts 先加载，避免双 spawn 抢资源

/**
 * Tauri 端的 tool_execute 在 stdout 不是合法 JSON 时会把整个 stdout 包成
 * `{ output: "<原始 stdout>" }` 返回。这里两种格式都要兼容。
 */
function unpack(result: ToolResult): CommitLinkPayload | null {
  if (!result?.success) return null
  const data = result.data
  if (!data) return null
  if (data.tasks && Array.isArray(data.tasks)) return data as CommitLinkPayload
  if (typeof data.output === 'string') {
    const start = data.output.indexOf('{')
    if (start < 0) return null
    try {
      return JSON.parse(data.output.slice(start)) as CommitLinkPayload
    } catch {
      return null
    }
  }
  return null
}

export function useTaskCommits(options: { autoLoad?: boolean } = { autoLoad: false }) {
  const store = useAppStore()
  const configStore = useConfigStore()
  let timer: ReturnType<typeof setInterval> | null = null
  let firstFetchTimer: ReturnType<typeof setTimeout> | null = null

  // 同 useTaskAlerts：配置未完成时不调 daemon，避免污染错误状态。
  // 同时也要求 repoRoots 非空 —— 没有代码目录就没 commit 可关联，发请求纯浪费。
  function isConfigReady(): boolean {
    if (!configStore.loaded) return false
    const c = configStore.config
    return !!(c.zentao.baseUrl?.trim() && c.zentao.account?.trim() && c.repoRoots?.length)
  }

  async function fetchCommits(range: 'today' | 'thisWeek' | 'last7days' = 'thisWeek') {
    if (!isConfigReady()) return
    try {
      const raw = await invoke<ToolResult>('tool_execute', {
        name: 'get_task_commits',
        input: { range, includeBody: true },
      })
      const payload = unpack(raw)
      if (!payload) {
        store.commitsLastError = raw?.error || 'get_task_commits 返回为空'
        store.commitsLoaded = true
        return
      }
      const map: Record<string, CommitLink[]> = {}
      for (const t of payload.tasks) {
        map[String(t.taskId)] = t.commits
      }
      store.commitsByTask = map
      store.commitsRange = range
      store.commitsLastError = null
      store.commitsLoaded = true
    } catch (e) {
      store.commitsLastError = e instanceof Error ? e.message : String(e)
      store.commitsLoaded = true
    }
  }

  function markCommitFeedback(taskId: string, sha: string, value: 'accepted' | 'rejected') {
    store.commitFeedback = { ...store.commitFeedback, [`${taskId}|${sha}`]: value }
  }

  if (options.autoLoad) {
    onMounted(() => {
      firstFetchTimer = setTimeout(() => {
        fetchCommits(store.commitsRange as any)
      }, FIRST_FETCH_DELAY)
      timer = setInterval(() => fetchCommits(store.commitsRange as any), POLL_INTERVAL)
    })

    onUnmounted(() => {
      if (firstFetchTimer) clearTimeout(firstFetchTimer)
      if (timer) clearInterval(timer)
      firstFetchTimer = null
      timer = null
    })
  }

  return {
    fetchCommits,
    markCommitFeedback,
  }
}
