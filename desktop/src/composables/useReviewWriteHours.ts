import { ref, onMounted, onUnmounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { cleanCommitTitle } from './cleanCommitTitle'

/**
 * Composable encapsulating the "write hours to ZenTao" flow for ReviewWindow.
 *
 * - Tracks which tasks have been written this session (writtenTasks)
 * - Manages opening/closing the write-hours modal window
 * - Listens for 'write-hours-done' events to mark tasks as written
 *
 * All state is returned so the consuming component can bind it in the template.
 */
export function useReviewWriteHours() {
  /** 本会话写入过的任务集合（taskId）。刷新窗口不丢，重启 app 会清空。 */
  const writtenTasks = ref<Set<string>>(new Set())

  // 全局只有一个写工时窗口；打开期间置真，task/orphan 两个按钮都禁用，防连点开多个。
  const openingWrite = ref(false)
  // 打开写工时窗口失败时的可见反馈（之前只 console.error，用户看不到）。几秒自动消失。
  const writeOpenError = ref('')
  let writeOpenErrorTimer: ReturnType<typeof setTimeout> | null = null

  function showWriteOpenError(e: unknown) {
    writeOpenError.value = `打开写工时窗口失败：${e instanceof Error ? e.message : String(e)}`
    if (writeOpenErrorTimer) clearTimeout(writeOpenErrorTimer)
    writeOpenErrorTimer = setTimeout(() => { writeOpenError.value = '' }, 4000)
  }

  let unlistenWriteDone: UnlistenFn | null = null

  onMounted(async () => {
    unlistenWriteDone = await listen<{ taskId: string }>('write-hours-done', (e) => {
      const tid = e.payload?.taskId
      if (tid) writtenTasks.value = new Set([...writtenTasks.value, tid])
    })
  })

  onUnmounted(() => {
    unlistenWriteDone?.()
    if (writeOpenErrorTimer) clearTimeout(writeOpenErrorTimer)
  })

  function buildWorkContent(commits: Array<{ title: string }>): string {
    const seen = new Set<string>()
    const lines: string[] = []
    for (const c of commits) {
      const cleaned = cleanCommitTitle(c.title)
      if (!cleaned || seen.has(cleaned)) continue
      seen.add(cleaned)
      lines.push(`- ${cleaned}`)
    }
    return lines.join('\n')
  }

  /** 从"按任务"区点开：taskId 预填，但保持可编辑 */
  async function openWriteModalForTask(t: {
    taskId: string
    taskName: string
    suggestedHours?: number
    commits: Array<{ title: string }>
  }) {
    if (writtenTasks.value.has(t.taskId) || openingWrite.value) return
    openingWrite.value = true
    const content = buildWorkContent(t.commits)
    try {
      await invoke('write_hours_open', {
        payload: {
          taskId: t.taskId,
          taskName: t.taskName,
          suggestedHours: t.suggestedHours,
          content,
          kind: 'task',
        },
      })
    } catch (e) {
      showWriteOpenError(e)
    } finally {
      setTimeout(() => { openingWrite.value = false }, 500)
    }
  }

  /** 从"未关联任务的提交"分组点开：taskId 空，让用户填 */
  async function openWriteModalForOrphan(g: {
    businessLine: string
    suggestedHours?: number
    commits: Array<{ title: string }>
  }) {
    if (openingWrite.value) return
    openingWrite.value = true
    const content = buildWorkContent(g.commits)
    try {
      await invoke('write_hours_open', {
        payload: {
          taskId: '',
          taskName: g.businessLine,
          suggestedHours: g.suggestedHours,
          content,
          kind: 'orphan',
        },
      })
    } catch (e) {
      showWriteOpenError(e)
    } finally {
      setTimeout(() => { openingWrite.value = false }, 500)
    }
  }

  function isTaskWritten(taskId: string): boolean {
    return writtenTasks.value.has(taskId)
  }

  return {
    writtenTasks,
    openingWrite,
    writeOpenError,
    openWriteModalForTask,
    openWriteModalForOrphan,
    isTaskWritten,
  }
}
