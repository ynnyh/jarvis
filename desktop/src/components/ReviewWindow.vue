<template src="./ReviewWindow.template.html"></template>
<script setup lang="ts">
import { ref, computed, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useAppStore } from '../stores/app'
import { useConfigStore } from '../stores/config'
import { useDailyReview, type ReviewRange } from '../composables/useDailyReview'
import { cleanCommitTitle } from '../composables/cleanCommitTitle'
import { useReviewWriteHours } from '../composables/useReviewWriteHours'

const store = useAppStore()
const configStore = useConfigStore()
const { fetchReview, copyPlainText } = useDailyReview()
const {
  openingWrite,
  writeOpenError,
  openWriteModalForTask,
  openWriteModalForOrphan,
  isTaskWritten,
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

async function handleRefresh() {
  if (refreshing.value) return
  refreshing.value = true
  try {
    await fetchReview(range.value)
  } finally {
    setTimeout(() => { refreshing.value = false }, 400)
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
