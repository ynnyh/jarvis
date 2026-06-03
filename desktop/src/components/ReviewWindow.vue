<template src="./ReviewWindow.template.html"></template>
<script setup lang="ts">
import { ref, computed, watch, nextTick } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { emit } from '@tauri-apps/api/event'
import { useAppStore } from '../stores/app'
import { useConfigStore } from '../stores/config'
import { useDailyReview, type ReviewRange } from '../composables/useDailyReview'
import { cleanCommitTitle } from '../composables/cleanCommitTitle'
import { useReviewWriteHours } from '../composables/useReviewWriteHours'

interface InlineWriteState {
  taskId: string
  taskName: string
  suggestedHours?: number
  content: string
  kind: 'task' | 'orphan'
}

interface TaskInfo {
  id: string
  name: string
}

const store = useAppStore()
const configStore = useConfigStore()
const { fetchReview, copyPlainText } = useDailyReview()
const {
  isTaskWritten,
  markWritten,
} = useReviewWriteHours()

const refreshing = ref(false)
const copyState = ref<'idle' | 'ok' | 'fail'>('idle')
const showRaw = ref(false)
const range = ref<ReviewRange>('today')

const RANGE_LABELS: Record<ReviewRange, string> = {
  today: '今天',
  yesterday: '昨天',
  thisWeek: '本周',
  lastWeek: '上周',
}

/** 每个任务的复制状态：taskId → 'ok' | 'fail' */
const taskCopyState = ref<Record<string, 'ok' | 'fail'>>({})

// ===== 内联写工时表单状态 =====
const inlineWrite = ref<InlineWriteState | null>(null)
const inlineSearch = ref('')
const inlineSearchFocus = ref(false)
const inlineHours = ref('')
const inlineContent = ref('')
const inlineSubmitting = ref(false)
const inlineError = ref('')
const inlineResult = ref<'idle' | 'ok' | 'fail'>('idle')
const inlineSearchEl = ref<HTMLInputElement | null>(null)

const allTasks = computed<TaskInfo[]>(() => store.reviewData?.allTasks ?? [])

/** 按搜索关键词过滤任务 */
const filteredTasks = computed(() => {
  const q = inlineSearch.value.trim().toLowerCase()
  if (!q) return []
  return allTasks.value
    .filter(t => t.id.startsWith(q) || t.name.toLowerCase().includes(q))
    .slice(0, 30)
})

function selectInlineTask(t: TaskInfo) {
  if (!inlineWrite.value) return
  inlineWrite.value.taskId = t.id
  inlineWrite.value.taskName = t.name
  inlineSearch.value = `#${t.id} ${t.name}`
  inlineSearchFocus.value = false
  inlineError.value = ''
  nextTick(() => document.getElementById('iw-hours')?.focus())
}

function clearInlineTask() {
  if (!inlineWrite.value) return
  inlineWrite.value.taskId = ''
  inlineWrite.value.taskName = ''
  inlineSearch.value = ''
  inlineSearchFocus.value = true
}

function openInlineWrite(state: InlineWriteState) {
  inlineWrite.value = { ...state }
  inlineSearch.value = state.taskId ? `#${state.taskId} ${state.taskName}` : state.taskName
  inlineHours.value = state.suggestedHours ? String(state.suggestedHours) : ''
  inlineContent.value = state.content || ''
  inlineError.value = ''
  inlineResult.value = 'idle'
  inlineSubmitting.value = false
  nextTick(() => {
    // 自动滚动到表单位置
    document.querySelector('.inline-write-section')?.scrollIntoView({ behavior: 'smooth', block: 'center' })
    if (!state.taskId) {
      inlineSearchEl.value?.focus()
      inlineSearchFocus.value = true
    } else {
      document.getElementById('iw-hours')?.focus()
    }
  })
}

let blurTimer: ReturnType<typeof setTimeout> | null = null

function onSearchBlur() {
  if (blurTimer) clearTimeout(blurTimer)
  blurTimer = setTimeout(() => { inlineSearchFocus.value = false }, 180)
}

function onSearchFocus() {
  if (blurTimer) clearTimeout(blurTimer)
  inlineSearchFocus.value = true
}

function cancelInlineWrite() {
  try {
    if (blurTimer) { clearTimeout(blurTimer); blurTimer = null }
    inlineSubmitting.value = false
    inlineWrite.value = null
    inlineSearch.value = ''
    inlineSearchFocus.value = false
    inlineError.value = ''
    inlineResult.value = 'idle'
  } catch (e) {
    console.error('[cancelInlineWrite]', e)
  }
}

async function submitInlineWrite() {
  if (inlineSubmitting.value || !inlineWrite.value) return
  // 优先用下拉选择的任务 ID，否则从搜索框里提取纯数字
  let tid = inlineWrite.value.taskId.trim().replace(/^#/, '')
  if (!/^\d+$/.test(tid)) {
    const extracted = inlineSearch.value.trim().replace(/^#/, '').match(/^(\d+)/)
    if (extracted) tid = extracted[1]
  }
  if (!/^\d+$/.test(tid)) {
    inlineError.value = '请选择或输入一个有效的禅道任务 ID（纯数字）'
    return
  }
  inlineWrite.value.taskId = tid
  const hoursNum = parseFloat(inlineHours.value)
  if (!Number.isFinite(hoursNum) || hoursNum <= 0) {
    inlineError.value = '工时必须是正数（小数也行，比如 0.5）'
    return
  }
  if (!inlineContent.value.trim()) {
    inlineError.value = '工作内容不能为空'
    return
  }
  inlineSubmitting.value = true
  inlineError.value = ''
  try {
    const r = await invoke<{ success: boolean; data?: any; error?: string }>('tool_execute', {
      name: 'log-task-effort',
      input: { taskId: tid, hours: hoursNum, work: inlineContent.value },
    })
    if (r.success && r.data?.ok) {
      inlineResult.value = 'ok'
      markWritten(tid)
      try { await emit('write-hours-done', { taskId: tid }) } catch (_e) {}
      setTimeout(() => { cancelInlineWrite() }, 1200)
    } else {
      inlineResult.value = 'fail'
      inlineError.value = r.error || '禅道返回未知错误'
    }
  } catch (e: any) {
    inlineResult.value = 'fail'
    inlineError.value = e?.message ?? String(e)
  } finally {
    inlineSubmitting.value = false
  }
}

function startWriteForTask(t: {
  taskId: string
  taskName: string
  suggestedHours?: number
  commits?: { title: string }[]
}) {
  const content = t.commits ? buildWorkContent(t.commits) : ''
  openInlineWrite({
    taskId: t.taskId,
    taskName: t.taskName,
    suggestedHours: t.suggestedHours,
    content,
    kind: 'task',
  })
}

function startWriteForOrphan(g: {
  businessLine: string
  suggestedHours?: number
  commits: { title: string }[]
}) {
  const content = buildWorkContent(g.commits)
  openInlineWrite({
    taskId: '',
    taskName: g.businessLine,
    suggestedHours: g.suggestedHours,
    content,
    kind: 'orphan',
  })
}

function startWriteSimple(taskId: string, taskName: string) {
  openInlineWrite({
    taskId,
    taskName,
    suggestedHours: undefined,
    content: '',
    kind: 'task',
  })
}

/** 通用写任务入口——taskId 为空，让用户搜索 */
function startWriteSearch() {
  openInlineWrite({
    taskId: '',
    taskName: '',
    suggestedHours: undefined,
    content: '',
    kind: 'orphan',
  })
}

function buildWorkContent(commits: { title: string }[]): string {
  const seen = new Set<string>()
  const lines: string[] = []
  for (const c of commits) {
    const cleaned = cleanCommitTitle(c.title, 200)
    if (!cleaned || seen.has(cleaned)) continue
    seen.add(cleaned)
    lines.push(`- ${cleaned}`)
  }
  return lines.join('\n')
}

async function handleRefresh() {
  if (refreshing.value) return
  refreshing.value = true
  try {
    await fetchReview(range.value)
  } finally {
    setTimeout(() => { refreshing.value = false }, 400)
  }
}

async function openBatchWrite() {
  try {
    await invoke('batch_write_open')
  } catch (e) {
    console.error('open batchWrite failed:', e)
  }
}

async function handleCopy() {
  const ok = await copyPlainText()
  copyState.value = ok ? 'ok' : 'fail'
  setTimeout(() => { copyState.value = 'idle' }, 1800)
}

/** 复制单个任务的工作内容 */
async function copyTaskCommits(task: { taskId: string; commits: { title: string }[] }) {
  const seen = new Set<string>()
  const lines: string[] = []
  for (const c of task.commits) {
    const cleaned = cleanCommitTitle(c.title)
    if (!cleaned || seen.has(cleaned)) continue
    seen.add(cleaned)
    lines.push(`- ${cleaned}`)
  }
  const text = lines.join('\n')
  try {
    await navigator.clipboard.writeText(text)
    taskCopyState.value = { ...taskCopyState.value, [task.taskId]: 'ok' }
  } catch {
    taskCopyState.value = { ...taskCopyState.value, [task.taskId]: 'fail' }
  }
  setTimeout(() => {
    const { [task.taskId]: _, ...rest } = taskCopyState.value
    taskCopyState.value = rest
  }, 1800)
}

/** 每个孤儿业务线分组的复制状态 */
const orphanCopyState = ref<Record<string, 'ok' | 'fail'>>({})

async function copyOrphanGroup(group: { businessLine: string; commits: { title: string }[] }) {
  const seen = new Set<string>()
  const lines: string[] = []
  for (const c of group.commits) {
    const cleaned = cleanCommitTitle(c.title)
    if (!cleaned || seen.has(cleaned)) continue
    seen.add(cleaned)
    lines.push(`- ${cleaned}`)
  }
  const text = lines.join('\n')
  try {
    await navigator.clipboard.writeText(text)
    orphanCopyState.value = { ...orphanCopyState.value, [group.businessLine]: 'ok' }
  } catch {
    orphanCopyState.value = { ...orphanCopyState.value, [group.businessLine]: 'fail' }
  }
  setTimeout(() => {
    const { [group.businessLine]: _, ...rest } = orphanCopyState.value
    orphanCopyState.value = rest
  }, 1800)
}

async function openTask(id: string) {
  try {
    await invoke('open_zentao_task', { id })
  } catch (e) {
    console.error('打开禅道任务失败:', e)
  }
}

async function openTransactionHours() {
  try {
    await invoke('manual_hours_open')
  } catch (e) {
    console.error('打开手动工时窗口失败:', e)
  }
}

const copyLabel = computed(() => {
  if (copyState.value === 'ok') return '✓ 已复制'
  if (copyState.value === 'fail') return '× 复制失败'
  return '📋 复制日报全文'
})

// 第一次打开窗口时自动加载
watch(() => store.showReviewWindow, (open) => {
  if (open && !store.reviewData && !store.reviewLoading) {
    fetchReview(range.value)
  }
})

// 切 range：直接显式调 fetchReview，不依赖 watch
async function switchRange(r: ReviewRange) {
  if (range.value === r) {
    await fetchReview(r)
    return
  }
  range.value = r
  await fetchReview(r)
}

function formatTime(iso: string): string {
  const m = iso.match(/(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})/)
  if (!m) return iso
  return `${m[2]}-${m[3]} ${m[4]}:${m[5]}`
}
</script>
<style src="./ReviewWindow.style.css" scoped></style>
