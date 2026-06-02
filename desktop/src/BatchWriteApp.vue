<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { getCurrentWindow } from '@tauri-apps/api/window'

interface CommitLink {
  sha: string
  shortSha: string
  title: string
  authoredDate: string
  repoPath: string
  businessLine: string
  repoName: string
  matchType: 'exact' | 'soft'
  matchedKeywords?: string[]
  effort: number
}

interface AdvancedTask {
  taskId: string
  taskName: string
  status: string
  commitCount: number
  commits: CommitLink[]
  businessLine: string
  effort: number
  bindingConfidence: number
  bindingReason: string
  defaultWorkContent: string
  suggestedHours?: number
}

interface OrphanGroup {
  businessLine: string
  commits: CommitLink[]
  effort?: number
  suggestedHours?: number
}

interface ReviewAllTask {
  id: string
  name: string
  status: string
}

interface DailyReviewData {
  date: string
  range: { since: string; until: string; label: string }
  summary: { totalCommits: number; businessLineCount: number; tasksAdvancedCount: number; orphanCommitCount: number }
  advancedTasks: AdvancedTask[]
  orphanCommits: OrphanGroup[]
  totalHoursForEstimate: number
  allTasks: ReviewAllTask[]
}

interface CustomPlanItem {
  id: string
  name: string
  estimatedHours: number
  kind: string
}

interface TodayPlanData {
  date: string
  taskIds: string[]
  estimatedHours: Record<string, number>
  customItems: CustomPlanItem[]
}

interface WriteEntry {
  key: string
  taskId: string
  taskName: string
  hours: number
  workContent: string
  commits: CommitLink[]
  kind: 'task' | 'orphan' | 'plan-only'
  written: boolean
  writeError?: string
  clientRequestId: string
}

interface ToolResult<T = any> {
  success: boolean
  data?: T
  error?: string
}

const loading = ref(true)
const loadError = ref('')
const reviewData = ref<DailyReviewData | null>(null)
const planData = ref<TodayPlanData | null>(null)
const entries = ref<WriteEntry[]>([])
const writing = ref(false)
const writeProgress = ref(0)
const writeTotal = ref(0)
const writeErrors = ref<string[]>([])

// task search for orphan assignment
const taskSearch = ref<Record<string, string>>({})

let cleanupClose: (() => void) | null = null

const totalHours = computed(() => entries.value.reduce((s, e) => s + (e.hours || 0), 0))
const writtenCount = computed(() => entries.value.filter(e => e.written).length)
const hasTried = computed(() => writeTotal.value > 0)

function buildClientRequestId(taskId: string, date: string) {
  return `batch-${date}-${taskId}`
}

function loadTaskSearch(entry: WriteEntry) {
  const q = taskSearch.value[entry.key] || ''
  if (!q.trim()) return reviewData.value?.allTasks.filter(t => t.status !== 'closed' && t.status !== 'cancel') || []
  const lower = q.toLowerCase()
  return (reviewData.value?.allTasks || []).filter(t => {
    if (t.status === 'closed' || t.status === 'cancel') return false
    return t.id.includes(lower) || t.name.toLowerCase().includes(lower)
  })
}

function assignTask(entry: WriteEntry, task: ReviewAllTask) {
  entry.taskId = task.id
  entry.taskName = task.name
  // check if plan has estimated hours for this task
  const planHours = planData.value?.estimatedHours?.[task.id]
  if (planHours) entry.hours = planHours
  taskSearch.value[entry.key] = ''
}

async function loadData() {
  loading.value = true
  loadError.value = ''
  try {
    const [raw, plan] = await Promise.all([
      invoke<ToolResult<DailyReviewData>>('tool_execute', {
        name: 'get_daily_review',
        input: { range: 'today' },
      }),
      invoke<TodayPlanData>('today_plan_load').catch(() => null),
    ])
    if (!raw.success || !raw.data) {
      loadError.value = raw.error || 'get_daily_review 返回为空'
      return
    }
    reviewData.value = raw.data
    planData.value = plan
    buildEntries(raw.data, plan)
  } catch (error) {
    loadError.value = error instanceof Error ? error.message : String(error)
  } finally {
    loading.value = false
  }
}

function buildEntries(review: DailyReviewData, plan: TodayPlanData | null) {
  const result: WriteEntry[] = []
  const date = review.date

  // advanced tasks with commits
  for (const t of review.advancedTasks) {
    const planHours = plan?.estimatedHours?.[t.taskId]
    const hours = planHours ?? t.suggestedHours ?? Math.round(t.effort * 100) / 100
    const content = t.defaultWorkContent || t.commits.map(c => c.title).join('；')
    result.push({
      key: `task-${t.taskId}`,
      taskId: t.taskId,
      taskName: t.taskName,
      hours,
      workContent: content,
      commits: t.commits,
      kind: 'task',
      written: false,
      clientRequestId: buildClientRequestId(t.taskId, date),
    })
  }

  // orphan commits
  for (const o of review.orphanCommits) {
    if (o.commits.length === 0) continue
    result.push({
      key: `orphan-${o.businessLine}`,
      taskId: '',
      taskName: `[未关联] ${o.businessLine}`,
      hours: o.suggestedHours || 0,
      workContent: o.commits.map(c => c.title).join('；'),
      commits: o.commits,
      kind: 'orphan',
      written: false,
      clientRequestId: buildClientRequestId(`orphan-${o.businessLine}`, date),
    })
  }

  // plan-only tasks (no commits)
  if (plan && plan.taskIds.length > 0) {
    const taskIdsWithCommits = new Set(review.advancedTasks.map(t => t.taskId))
    for (const taskId of plan.taskIds) {
      if (taskIdsWithCommits.has(taskId)) continue // already in advancedTasks
      const hours = plan.estimatedHours?.[taskId] || 0
      const taskName = review.allTasks.find(t => t.id === taskId)?.name || `任务 #${taskId}`
      result.push({
        key: `plan-${taskId}`,
        taskId,
        taskName,
        hours,
        workContent: '',
        commits: [],
        kind: 'plan-only',
        written: false,
        clientRequestId: buildClientRequestId(taskId, date),
      })
    }
    // custom items from plan
    for (const item of plan.customItems || []) {
      result.push({
        key: `custom-${item.id}`,
        taskId: item.id,
        taskName: item.name,
        hours: item.estimatedHours,
        workContent: '',
        commits: [],
        kind: 'plan-only',
        written: false,
        clientRequestId: buildClientRequestId(item.id, date),
      })
    }
  }

  // dedup by key
  result.sort((a, b) => {
    if (a.kind === 'plan-only' && b.kind !== 'plan-only') return 1
    if (a.kind !== 'plan-only' && b.kind === 'plan-only') return -1
    return b.commits.length - a.commits.length
  })

  entries.value = result
}

async function writeOne(entry: WriteEntry): Promise<boolean> {
  try {
    const r = await invoke<ToolResult<{ ok: boolean }>>('tool_execute', {
      name: 'log-task-effort',
      input: {
        taskId: entry.taskId,
        hours: entry.hours,
        work: entry.workContent,
        date: reviewData.value?.date,
        clientRequestId: entry.clientRequestId,
      },
    })
    if (!r.success || !r.data?.ok) {
      entry.writeError = r.error || '写入失败（未知原因）'
      return false
    }
    entry.written = true
    entry.writeError = undefined
    return true
  } catch (error) {
    entry.writeError = error instanceof Error ? error.message : String(error)
    return false
  }
}

async function writeAll() {
  const pending = entries.value.filter(e => !e.written && e.hours > 0 && e.taskId && e.workContent)
  if (pending.length === 0) {
    if (entries.value.every(e => e.written)) {
      loadError.value = '所有条目已写入，无需重复操作'
    } else {
      loadError.value = '没有可写入的条目（需填齐任务、工时、内容）'
    }
    return
  }
  writing.value = true
  writeErrors.value = []
  writeTotal.value = pending.length
  writeProgress.value = 0

  for (const entry of pending) {
    await writeOne(entry)
    writeProgress.value++
  }

  const failed = pending.filter(e => !e.written)
  if (failed.length > 0) {
    writeErrors.value = failed.map(e => `#${e.taskId} ${e.taskName}: ${e.writeError || '未知错误'}`)
  }
  writing.value = false
}

function addManualEntry() {
  const ts = Date.now()
  entries.value = [...entries.value, {
    key: `manual-${ts}`,
    taskId: '',
    taskName: '',
    hours: 0,
    workContent: '',
    commits: [],
    kind: 'plan-only',
    written: false,
    clientRequestId: buildClientRequestId(`manual-${ts}`, reviewData.value?.date || ''),
  }]
}

function removeEntry(key: string) {
  entries.value = entries.value.filter(e => e.key !== key)
}

async function closeWindow() {
  await invoke('batch_write_close')
}

onMounted(async () => {
  await loadData()
  const win = getCurrentWindow()
  cleanupClose = await win.onCloseRequested(async event => {
    event.preventDefault()
    if (writing.value) return // don't close while writing
    await closeWindow()
  })
})

onUnmounted(() => {
  cleanupClose?.()
})
</script>

<template>
  <div class="bw-root">
    <header class="bw-header" data-tauri-drag-region>
      <div>
        <h1>一键写工时</h1>
        <p v-if="reviewData">{{ reviewData.date }} · {{ reviewData.summary.totalCommits }} commits · {{ reviewData.summary.tasksAdvancedCount }} tasks</p>
      </div>
      <button class="bw-close" :disabled="writing" @click="closeWindow">x</button>
    </header>

    <main class="bw-body">
      <div v-if="loading" class="bw-empty">加载今日数据中...</div>
      <div v-else-if="loadError" class="bw-empty error">{{ loadError }}</div>
      <template v-else>
        <!-- entries -->
        <div v-for="entry in entries" :key="entry.key" class="bw-card" :class="{ written: entry.written, 'plan-only': entry.kind === 'plan-only' && entry.commits.length === 0 }">
          <div class="bw-card-body">
            <div
              v-if="entry.kind === 'plan-only' && entry.commits.length === 0"
              class="bw-badge warn"
            >无 commit</div>

            <div v-if="entry.written" class="bw-badge ok">已写入</div>

            <div class="bw-card-top">
              <div class="bw-task-label">
                <template v-if="entry.kind === 'orphan'">
                  <span class="bw-orphan-label">未关联 · {{ entry.commits.length }} commits</span>
                  <div class="bw-task-search">
                    <input
                      class="bw-search-input"
                      type="text"
                      placeholder="搜索禅道任务..."
                      :value="taskSearch[entry.key] || ''"
                      @input="taskSearch[entry.key] = ($event.target as HTMLInputElement).value"
                      :disabled="entry.written"
                    />
                    <div v-if="taskSearch[entry.key]" class="bw-dropdown">
                      <button
                        v-for="t in loadTaskSearch(entry)"
                        :key="t.id"
                        class="bw-dropdown-item"
                        @click="assignTask(entry, t)"
                      >#{{ t.id }} {{ t.name }}</button>
                      <div v-if="loadTaskSearch(entry).length === 0" class="bw-dropdown-empty">无匹配任务</div>
                    </div>
                  </div>
                </template>
                <template v-else-if="entry.taskId">
                  <span class="bw-task-id">#{{ entry.taskId }}</span>
                  <span class="bw-task-name">{{ entry.taskName }}</span>
                </template>
                <template v-else>
                  <input
                    class="bw-task-input"
                    type="text"
                    placeholder="输入任务名..."
                    :value="entry.taskName"
                    @input="entry.taskName = ($event.target as HTMLInputElement).value"
                    :disabled="entry.written"
                  />
                </template>
              </div>

              <input
                class="bw-hours"
                type="number"
                min="0"
                max="12"
                step="0.5"
                :value="entry.hours"
                @input="entry.hours = parseFloat(($event.target as HTMLInputElement).value) || 0"
                :disabled="entry.written"
              />h
            </div>

            <textarea
              class="bw-content"
              rows="2"
              :value="entry.workContent"
              @input="entry.workContent = ($event.target as HTMLTextAreaElement).value"
              :disabled="entry.written"
              placeholder="工作内容..."
            ></textarea>

            <div v-if="entry.commits.length > 0" class="bw-commits">
              <div v-for="c in entry.commits.slice(0, 4)" :key="c.shortSha" class="bw-commit-line">
                <code>{{ c.shortSha }}</code>
                <span>{{ c.title }}</span>
              </div>
              <div v-if="entry.commits.length > 4" class="bw-commit-more">... 还有 {{ entry.commits.length - 4 }} 条</div>
            </div>

            <div v-if="entry.writeError" class="bw-error-text">{{ entry.writeError }}</div>
          </div>

          <button
            v-if="entry.kind === 'plan-only' && !entry.written"
            class="bw-btn-icon"
            @click="removeEntry(entry.key)"
          >x</button>
        </div>

        <button class="bw-add-btn" :disabled="writing" @click="addManualEntry()">+ 添加手动条目</button>
      </template>
    </main>

    <footer class="bw-footer">
      <div class="bw-footer-left">
        <span v-if="hasTried">{{ writtenCount }}/{{ writeTotal }} 已完成</span>
        <span v-if="writeErrors.length > 0" class="bw-error-count">{{ writeErrors.length }} 条失败</span>
      </div>
      <span class="bw-total">合计 {{ totalHours.toFixed(1) }} / 8 h</span>
      <button class="bw-btn primary" :disabled="writing || loading" @click="writeAll()">
        {{ writing ? `写入中 ${writeProgress}/${writeTotal}...` : '一键写入' }}
      </button>
    </footer>
  </div>
</template>

<style scoped>
.bw-root { display: flex; flex-direction: column; height: 100vh; background: #0b1120; color: rgba(255,255,255,.92); font-family: system-ui, -apple-system, sans-serif; }
.bw-header { display: flex; align-items: center; justify-content: space-between; gap: 16px; padding: 18px 22px 14px; background: rgba(17,24,39,.98); border-bottom: 1px solid rgba(148,163,184,.18); user-select: none; }
.bw-header h1 { margin: 0; font-size: 19px; font-weight: 700; }
.bw-header p { margin: 4px 0 0; font-size: 12px; color: rgba(255,255,255,.46); }
.bw-close { width: 30px; height: 30px; border: none; border-radius: 6px; color: rgba(255,255,255,.6); background: transparent; cursor: pointer; font-size: 18px; }
.bw-close:hover { color: rgba(255,255,255,.95); background: rgba(255,255,255,.08); }
.bw-body { flex: 1; min-height: 0; overflow-y: auto; padding: 16px 22px 24px; display: flex; flex-direction: column; gap: 12px; }
.bw-empty { flex: 1; display: flex; align-items: center; justify-content: center; color: rgba(255,255,255,.55); font-size: 13px; }
.bw-empty.error { color: rgba(252,165,165,.95); }

.bw-card { display: flex; gap: 8px; align-items: flex-start; padding: 12px; border-radius: 10px; background: rgba(255,255,255,.04); border: 1px solid rgba(255,255,255,.06); }
.bw-card.written { opacity: .55; }
.bw-card.plan-only { border-color: rgba(251,191,36,.18); }
.bw-card-body { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 8px; }
.bw-badge { display: inline-block; padding: 1px 8px; border-radius: 3px; font-size: 10px; width: fit-content; }
.bw-badge.warn { background: rgba(251,191,36,.15); color: rgba(251,191,36,.9); }
.bw-badge.ok { background: rgba(74,222,128,.15); color: rgba(74,222,128,.9); }
.bw-card-top { display: flex; align-items: center; gap: 10px; }
.bw-task-label { flex: 1; min-width: 0; display: flex; align-items: center; gap: 6px; }
.bw-task-id { font-size: 11px; color: rgba(96,165,250,.9); font-weight: 600; white-space: nowrap; }
.bw-task-name { font-size: 12px; color: rgba(255,255,255,.85); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.bw-orphan-label { font-size: 11px; color: rgba(251,191,36,.85); white-space: nowrap; }
.bw-task-input { flex: 1; padding: 3px 6px; font-size: 12px; color: rgba(255,255,255,.9); background: rgba(255,255,255,.06); border: 1px solid rgba(255,255,255,.1); border-radius: 4px; }
.bw-hours { width: 48px; padding: 3px 6px; font-size: 12px; color: rgba(255,255,255,.9); background: rgba(255,255,255,.06); border: 1px solid rgba(255,255,255,.1); border-radius: 4px; text-align: center; }
.bw-content { width: 100%; padding: 6px 8px; font-size: 12px; color: rgba(255,255,255,.85); background: rgba(255,255,255,.06); border: 1px solid rgba(255,255,255,.08); border-radius: 5px; resize: vertical; font-family: inherit; }
.bw-content:disabled { opacity: .5; }
.bw-task-search { position: relative; flex: 1; min-width: 0; }
.bw-search-input { width: 100%; padding: 3px 6px; font-size: 11px; color: rgba(255,255,255,.85); background: rgba(255,255,255,.06); border: 1px solid rgba(255,255,255,.1); border-radius: 4px; }
.bw-dropdown { position: absolute; top: 100%; left: 0; right: 0; max-height: 180px; overflow-y: auto; background: rgba(17,24,39,.98); border: 1px solid rgba(255,255,255,.12); border-radius: 4px; z-index: 10; }
.bw-dropdown-item { display: block; width: 100%; padding: 5px 8px; font-size: 11px; color: rgba(255,255,255,.85); background: transparent; border: none; text-align: left; cursor: pointer; }
.bw-dropdown-item:hover { background: rgba(59,130,246,.15); }
.bw-dropdown-empty { padding: 5px 8px; font-size: 11px; color: rgba(255,255,255,.35); }
.bw-commits { display: flex; flex-direction: column; gap: 3px; }
.bw-commit-line { display: flex; gap: 6px; font-size: 11px; color: rgba(255,255,255,.45); }
.bw-commit-line code { font-family: monospace; color: rgba(255,255,255,.3); }
.bw-commit-more { font-size: 10px; color: rgba(255,255,255,.25); }
.bw-error-text { font-size: 11px; color: rgba(252,165,165,.9); background: rgba(252,165,165,.08); padding: 4px 8px; border-radius: 4px; }
.bw-btn-icon { width: 24px; height: 24px; border: none; border-radius: 4px; color: rgba(255,255,255,.4); background: transparent; cursor: pointer; font-size: 14px; flex-shrink: 0; }
.bw-btn-icon:hover { color: rgba(252,165,165,.9); background: rgba(255,255,255,.06); }
.bw-add-btn { padding: 8px; font-size: 12px; color: rgba(255,255,255,.45); background: transparent; border: 1px dashed rgba(255,255,255,.1); border-radius: 8px; cursor: pointer; }
.bw-add-btn:hover { color: rgba(255,255,255,.7); border-color: rgba(255,255,255,.2); }
.bw-footer { display: flex; align-items: center; gap: 12px; padding: 12px 20px; background: rgba(0,0,0,.2); border-top: 1px solid rgba(255,255,255,.06); }
.bw-footer-left { flex: 1; font-size: 11px; color: rgba(255,255,255,.5); }
.bw-error-count { color: rgba(252,165,165,.9); }
.bw-total { font-size: 12px; color: rgba(255,255,255,.7); }
.bw-btn { padding: 10px 24px; font-size: 13px; border-radius: 6px; border: 1px solid transparent; cursor: pointer; font-weight: 600; }
.bw-btn.primary { background: linear-gradient(135deg, rgba(59,130,246,.95), rgba(37,99,235,.95)); color: white; }
.bw-btn:disabled { opacity: .4; cursor: not-allowed; }
</style>
