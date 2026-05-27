<script setup lang="ts">
import { ref, computed, watch, onMounted, onUnmounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { useAppStore } from '../stores/app'
import { useConfigStore } from '../stores/config'
import { useDailyReview, type ReviewRange } from '../composables/useDailyReview'
import { cleanCommitTitle } from '../composables/cleanCommitTitle'

const store = useAppStore()
const configStore = useConfigStore()
const { fetchReview, copyPlainText } = useDailyReview()

const refreshing = ref(false)
const copyState = ref<'idle' | 'ok' | 'fail'>('idle')
const showRaw = ref(false)
const range = ref<ReviewRange>('today')

const RANGE_LABELS: Record<ReviewRange, string> = {
  today: '今天',
  yesterday: '昨天',
  thisWeek: '本周',
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

/** 复制单个任务的工作内容：去重后的 commit 标题列表（去掉 emoji/feat: 前缀），每行一个 */
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

/** 每个孤儿业务线分组的复制状态。和 taskCopyState 同一套设计，但 key 是
 *  businessLine 而非 taskId —— 孤儿 commit 按业务线分组。 */
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

// 切 range：直接显式调 fetchReview，不依赖 watch（之前发现 watch 偶发
// 不触发，且即使触发也会被 reviewLoading 早返回吞掉），显式调更稳。
async function switchRange(r: ReviewRange) {
  if (range.value === r) {
    // 同一档再点也强制刷一遍，相当于"再查一次"
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

// ===== 一键写入禅道工时 =====
//
// 闸门 0 试发完成后扩面（用户已确认机制 OK 且可手动删除），用户认可
// suggestedHours 方案 B：弹窗给出 AI 估算工时 + 自动拼接的工作内容，
// 允许二次编辑后提交。同项目多 commit 合并去重一段。
//
// taskId 也设为可编辑：
// - 已关联到任务的 commit：默认填入该 taskId，用户可改（少见但偶尔需要）
// - 孤儿 commit（repo 没绑定任务）：taskId 空，让用户手动填一个真实任务号
//   也能写工时 —— 应对"这次提交对应的任务还没建/还没绑定但工时得记"的场景
//
// 写工时编辑器是独立的 Tauri 窗口（writeHours，见 WriteHoursApp.vue），
// 这里只负责构造 payload 触发 write_hours_open，并监听 write-hours-done
// 事件把任务标灰防重复点。本会话写入过的 taskId 用 writtenTasks 兜底，
// app 重启清空 —— 由禅道工时记录本身做最终判重。

/** 本会话写入过的任务集合（taskId）。刷新窗口不丢，重启 app 会清空。 */
const writtenTasks = ref<Set<string>>(new Set())

let unlistenWriteDone: UnlistenFn | null = null
onMounted(async () => {
  unlistenWriteDone = await listen<{ taskId: string }>('write-hours-done', (e) => {
    const tid = e.payload?.taskId
    if (tid) writtenTasks.value = new Set([...writtenTasks.value, tid])
  })
})
onUnmounted(() => { unlistenWriteDone?.() })

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
  if (writtenTasks.value.has(t.taskId)) return
  const content = buildWorkContent(t.commits)
  console.log('[review] openWriteModalForTask payload:', {
    taskId: t.taskId,
    commitsCount: t.commits?.length ?? 0,
    contentPreview: content.slice(0, 80),
  })
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
    console.error('write_hours_open 失败:', e)
  }
}

/** 从"未关联任务的提交"分组点开：taskId 空，让用户填 */
async function openWriteModalForOrphan(g: {
  businessLine: string
  suggestedHours?: number
  commits: Array<{ title: string }>
}) {
  const content = buildWorkContent(g.commits)
  console.log('[review] openWriteModalForOrphan payload:', {
    businessLine: g.businessLine,
    commitsCount: g.commits?.length ?? 0,
    firstTitle: g.commits?.[0]?.title ?? '(no commits)',
    contentLen: content.length,
    contentPreview: content.slice(0, 80),
  })
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
    console.error('write_hours_open 失败:', e)
  }
}

function isTaskWritten(taskId: string): boolean {
  return writtenTasks.value.has(taskId)
}
</script>

<template>
  <Transition name="panel">
    <div v-if="store.showReviewWindow" class="review-panel pointer-target">
      <header class="panel-header">
        <div class="panel-title">
          <span class="title-icon">📋</span>
          <span class="title-text">今日复盘</span>
        </div>
        <div class="panel-actions">
          <div class="range-switch">
            <button class="range-btn" :class="{ active: range === 'today' }" :disabled="store.reviewLoading" @click="switchRange('today')">今天</button>
            <button class="range-btn" :class="{ active: range === 'yesterday' }" :disabled="store.reviewLoading" @click="switchRange('yesterday')">昨天</button>
            <button class="range-btn" :class="{ active: range === 'thisWeek' }" :disabled="store.reviewLoading" @click="switchRange('thisWeek')">本周</button>
          </div>
          <button class="icon-btn" :class="{ spinning: refreshing }" title="刷新" @click="handleRefresh">↻</button>
          <button class="icon-btn" title="关闭" @click="store.showReviewWindow = false">×</button>
        </div>
      </header>

      <!-- 切 range / 手动刷新时的细条 loading：reviewData 已经在，不能用大空状态覆盖 -->
      <div v-if="store.reviewLoading && store.reviewData" class="refresh-strip">
        <span class="refresh-spinner">⟳</span>
        <span>正在拉取「{{ RANGE_LABELS[range] }}」的复盘数据…</span>
      </div>

      <!-- 加载中（无数据时的大空状态） -->
      <div v-if="store.reviewLoading && !store.reviewData" class="empty">
        <span class="empty-icon loading">⟳</span>
        <p class="empty-hint">正在扫描本地仓库和禅道任务…</p>
      </div>

      <!-- 错误 -->
      <div v-else-if="store.reviewLastError && !store.reviewData" class="empty">
        <span class="empty-icon error">!</span>
        <p class="empty-text">生成复盘失败</p>
        <p class="empty-hint">{{ store.reviewLastError }}</p>
        <button class="retry-btn" @click="handleRefresh">重试</button>
      </div>

      <div v-else-if="store.reviewData" class="panel-body">
        <!-- 招呼语：用 userTitle 称呼用户，区分今天/本周 -->
        <p class="greeting">
          {{ configStore.config.userTitle }}，这是「{{ RANGE_LABELS[range] }}」的复盘
        </p>
        <!-- 概况 -->
        <section class="section">
          <div class="summary-grid">
            <div class="summary-item">
              <div class="summary-num">{{ store.reviewData.summary.totalCommits }}</div>
              <div class="summary-label">commit</div>
            </div>
            <div class="summary-item">
              <div class="summary-num">{{ store.reviewData.summary.businessLineCount }}</div>
              <div class="summary-label">业务线</div>
            </div>
            <div class="summary-item">
              <div class="summary-num">{{ store.reviewData.summary.tasksAdvancedCount }}</div>
              <div class="summary-label">推进任务</div>
            </div>
            <div class="summary-item" v-if="store.reviewData.needsStatusUpdate.length > 0">
              <div class="summary-num warn">{{ store.reviewData.needsStatusUpdate.length }}</div>
              <div class="summary-label">待更新</div>
            </div>
          </div>
          <p class="section-hint">范围：{{ store.reviewData.range.label }}</p>
        </section>

        <!-- 空状态 -->
        <div v-if="store.reviewData.summary.totalCommits === 0" class="empty-small">
          这段时间没有本地提交。
        </div>

        <!-- 待状态更新的任务（top） -->
        <section v-if="store.reviewData.needsStatusUpdate.length > 0" class="section needs-update">
          <h3 class="section-title warn-title">⚠️ 需要在禅道更新状态</h3>
          <ul class="needs-list">
            <li
              v-for="t in store.reviewData.needsStatusUpdate"
              :key="t.taskId"
              class="needs-item"
              @click="openTask(t.taskId)"
              title="点击打开禅道"
            >
              <div class="needs-title">#{{ t.taskId }} {{ t.taskName }}</div>
              <div class="needs-reason">{{ t.reason }}</div>
            </li>
          </ul>
        </section>

        <!-- 按任务（用于禅道填报） -->
        <section v-if="store.reviewData.advancedTasks.length > 0" class="section">
          <h3 class="section-title">
            <span>📝 按任务（用于禅道填报）</span>
            <span class="section-count">{{ store.reviewData.advancedTasks.length }} 个任务</span>
          </h3>
          <p class="section-hint">点 ✍️ 写工时到禅道（弹窗内可编辑工时和内容）；点 📋 仅复制工作内容</p>
          <ul class="task-fill-list">
            <li v-for="t in store.reviewData.advancedTasks" :key="t.taskId" class="task-fill-item">
              <div class="task-fill-row">
                <span class="task-fill-id" @click="openTask(t.taskId)" title="打开禅道任务">#{{ t.taskId }}</span>
                <span class="task-fill-name">{{ t.taskName }}</span>
                <span v-if="t.suggestedHours" class="hours-pill">~{{ t.suggestedHours }}h</span>
                <button
                  class="write-mini"
                  :class="{ written: isTaskWritten(t.taskId) }"
                  :disabled="isTaskWritten(t.taskId)"
                  @click="openWriteModalForTask(t)"
                  :title="isTaskWritten(t.taskId) ? '本会话已写入（去禅道可继续修改）' : `写入工时到禅道 #${t.taskId}`"
                >
                  {{ isTaskWritten(t.taskId) ? '✓ 已写入' : '✍️ 写工时' }}
                </button>
                <button
                  class="copy-mini"
                  :class="`mini-${taskCopyState[t.taskId] || 'idle'}`"
                  @click="copyTaskCommits(t)"
                  :title="`复制 ${t.commitCount} 条工作内容`"
                >
                  {{ taskCopyState[t.taskId] === 'ok' ? '✓' : taskCopyState[t.taskId] === 'fail' ? '×' : '📋' }}
                </button>
              </div>
              <div class="task-fill-meta">
                <span>{{ t.businessLine }}</span>
                <span class="muted">·</span>
                <span>{{ t.commitCount }} 个 commit</span>
                <span v-if="t.status === 'wait'" class="status-mark">未开始</span>
              </div>
            </li>
          </ul>
        </section>

        <!-- 业务线分组：展示工时建议 -->
        <section v-for="g in store.reviewData.byBusinessLine" :key="g.businessLine" class="section">
          <h3 class="section-title">
            <span>{{ g.businessLine }}</span>
            <span class="section-count">
              {{ g.commits.length }} 个 commit · {{ g.tasks.length }} 个任务
              <span v-if="g.suggestedHours" class="hours-pill">建议 ~{{ g.suggestedHours }}h</span>
            </span>
          </h3>

          <details class="commits-block" open>
            <summary class="commits-summary">📦 commit 列表</summary>
            <ul class="commit-list">
              <li v-for="c in g.commits" :key="c.sha + c.repoPath" class="commit-item">
                <div class="commit-title">{{ cleanCommitTitle(c.title) }}</div>
                <div class="commit-meta">
                  <span class="commit-repo">{{ c.repoName }}</span>
                  <span class="commit-sha">{{ c.shortSha }}</span>
                  <span class="commit-time">{{ formatTime(c.authoredDate) }}</span>
                </div>
              </li>
            </ul>
          </details>

          <details v-if="g.tasks.length > 0" class="commits-block">
            <summary class="commits-summary">🎯 涉及的任务（{{ g.tasks.length }}）</summary>
            <ul class="task-mini-list">
              <li
                v-for="t in g.tasks"
                :key="t.taskId"
                class="task-mini-item"
                @click="openTask(t.taskId)"
                title="点击打开禅道"
              >
                #{{ t.taskId }} {{ t.taskName }}
              </li>
            </ul>
          </details>
        </section>

        <!-- 未关联任务的提交：用户实际写了代码但没匹配到禅道任务，也是日报内容 -->
        <section v-if="store.reviewData.orphanCommits.some(o => o.commits.length > 0)" class="section">
          <h3 class="section-title">
            <span>🧩 未关联禅道任务的提交</span>
            <span class="section-count">{{ store.reviewData.summary.orphanCommitCount }} 个 commit</span>
          </h3>
          <p class="section-hint">这些 commit 没匹配到任务号。点 ✍️ 手动填一个任务 ID 写工时；点 📋 仅复制内容。</p>
          <ul class="task-fill-list">
            <li v-for="g in store.reviewData.orphanCommits.filter(o => o.commits.length > 0)"
              :key="g.businessLine" class="task-fill-item orphan-item">
              <div class="task-fill-row">
                <span class="task-fill-name">{{ g.businessLine }}</span>
                <span v-if="g.suggestedHours" class="hours-pill">~{{ g.suggestedHours }}h</span>
                <button
                  class="write-mini"
                  @click="openWriteModalForOrphan(g)"
                  title="手动填任务 ID 后写入工时"
                >
                  ✍️ 写到任务…
                </button>
                <button
                  class="copy-mini"
                  :class="`mini-${orphanCopyState[g.businessLine] || 'idle'}`"
                  @click="copyOrphanGroup(g)"
                  :title="`复制 ${g.commits.length} 条 commit`"
                >
                  {{ orphanCopyState[g.businessLine] === 'ok' ? '✓' : orphanCopyState[g.businessLine] === 'fail' ? '×' : '📋' }}
                </button>
              </div>
              <div class="task-fill-meta">
                <span>{{ g.commits.length }} 个 commit</span>
              </div>
              <details class="orphan-commits-block">
                <summary class="commits-summary">📦 commit 列表</summary>
                <ul class="commit-list">
                  <li v-for="c in g.commits" :key="c.sha + c.repoPath" class="commit-item">
                    <div class="commit-title">{{ cleanCommitTitle(c.title) }}</div>
                    <div class="commit-meta">
                      <span class="commit-repo">{{ c.repoName }}</span>
                      <span class="commit-sha">{{ c.shortSha }}</span>
                      <span class="commit-time">{{ formatTime(c.authoredDate) }}</span>
                    </div>
                  </li>
                </ul>
              </details>
            </li>
          </ul>
        </section>

        <!-- 原始日报全文预览 -->
        <section class="section">
          <button class="toggle-raw" @click="showRaw = !showRaw">
            {{ showRaw ? '收起' : '展开' }} 日报全文
            <span class="toggle-arrow">{{ showRaw ? '▾' : '▸' }}</span>
          </button>
          <pre v-if="showRaw" class="markdown-pre">{{ store.reviewData.plainText }}</pre>
        </section>
      </div>

      <!-- 底部操作 -->
      <footer v-if="store.reviewData" class="panel-footer">
        <button class="copy-btn" :class="`state-${copyState}`" @click="handleCopy">{{ copyLabel }}</button>
      </footer>
    </div>
  </Transition>
</template>

<style scoped>
.review-panel {
  position: fixed;
  inset: var(--panel-top, 8px) var(--panel-right, 8px) var(--panel-bottom, 90px) var(--panel-left, 8px);
  display: flex;
  flex-direction: column;
  background: linear-gradient(135deg, rgba(20, 30, 56, 0.97), rgba(15, 23, 42, 0.97));
  border: 1px solid rgba(59, 130, 246, 0.3);
  border-radius: 14px;
  box-shadow: 0 12px 32px rgba(0, 0, 0, 0.5);
  overflow: hidden;
  z-index: 55;
  color: rgba(255, 255, 255, 0.92);
}

.panel-header {
  display: flex; align-items: center; justify-content: space-between;
  padding: 8px 10px;
  background: rgba(0, 0, 0, 0.2);
  border-bottom: 1px solid rgba(255, 255, 255, 0.06);
}
.panel-title { display: flex; align-items: center; gap: 6px; font-size: 13px; font-weight: 600; }
.title-icon { font-size: 14px; }
.panel-actions { display: flex; align-items: center; gap: 4px; }

.range-switch {
  display: flex; gap: 2px;
  background: rgba(255, 255, 255, 0.04);
  border-radius: 5px;
  padding: 2px;
  margin-right: 4px;
}
.range-btn {
  background: transparent; border: none; color: rgba(255, 255, 255, 0.6);
  padding: 2px 8px; font-size: 10px;
  border-radius: 3px; cursor: pointer;
}
.range-btn.active { background: rgba(59, 130, 246, 0.3); color: rgba(255, 255, 255, 0.95); }
.range-btn:hover:not(.active):not(:disabled) { color: rgba(255, 255, 255, 0.85); }
.range-btn:disabled { opacity: 0.5; cursor: wait; }

/* 切 range / 手动刷新时的细条 loading */
.refresh-strip {
  display: flex; align-items: center; gap: 6px; justify-content: center;
  padding: 5px 10px;
  font-size: 11px;
  color: rgba(147, 197, 253, 0.95);
  background: rgba(59, 130, 246, 0.12);
  border-bottom: 1px solid rgba(59, 130, 246, 0.25);
}
.refresh-spinner {
  display: inline-block;
  animation: spin 0.9s linear infinite;
}
@keyframes spin { from { transform: rotate(0); } to { transform: rotate(360deg); } }

.icon-btn {
  width: 22px; height: 22px;
  display: inline-flex; align-items: center; justify-content: center;
  font-size: 16px; line-height: 1;
  color: rgba(255, 255, 255, 0.55);
  background: transparent; border: none; border-radius: 6px;
  cursor: pointer;
}
.icon-btn:hover { color: rgba(255, 255, 255, 0.95); background: rgba(255, 255, 255, 0.08); }
.icon-btn.spinning { animation: spin 0.6s linear infinite; }
@keyframes spin { from { transform: rotate(0); } to { transform: rotate(360deg); } }

.panel-body {
  flex: 1; overflow-y: auto; padding: 10px;
  display: flex; flex-direction: column; gap: 14px;
}

.greeting {
  margin: 0;
  font-size: 12.5px;
  color: rgba(255, 255, 255, 0.65);
  line-height: 1.5;
}

.section { display: flex; flex-direction: column; gap: 6px; }
.section-title {
  margin: 0;
  font-size: 11px;
  font-weight: 600;
  color: rgba(0, 212, 255, 0.85);
  letter-spacing: 0.5px;
  display: flex;
  align-items: center;
  justify-content: space-between;
}
.section-title.warn-title { color: rgba(250, 204, 21, 0.95); }
.section-count { font-size: 9.5px; color: rgba(255, 255, 255, 0.45); font-weight: normal; }
.hours-pill {
  display: inline-block;
  margin-left: 6px;
  padding: 1px 6px;
  background: rgba(34, 197, 94, 0.18);
  color: rgba(134, 239, 172, 0.95);
  border-radius: 3px;
  font-size: 9.5px;
  font-weight: 600;
}
.section-hint { margin: 0; font-size: 9.5px; color: rgba(255, 255, 255, 0.35); }

/* 概况 */
.summary-grid {
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  gap: 6px;
  padding: 8px 0;
}
.summary-item {
  display: flex;
  flex-direction: column;
  align-items: center;
  padding: 8px 4px;
  background: rgba(255, 255, 255, 0.03);
  border-radius: 6px;
}
.summary-num { font-size: 20px; font-weight: 700; color: rgba(255, 255, 255, 0.95); line-height: 1; }
.summary-num.warn { color: rgba(250, 204, 21, 0.95); }
.summary-label { font-size: 9.5px; color: rgba(255, 255, 255, 0.5); margin-top: 4px; }

/* 待更新 */
.needs-update {
  padding: 6px 8px;
  background: rgba(250, 204, 21, 0.06);
  border-left: 2px solid rgba(250, 204, 21, 0.5);
  border-radius: 4px;
}
.needs-list { list-style: none; margin: 0; padding: 0; }
.needs-item {
  padding: 6px 0;
  border-top: 1px dashed rgba(255, 255, 255, 0.06);
  cursor: pointer;
}
.needs-item:first-child { border-top: none; }
.needs-item:hover { background: rgba(255, 255, 255, 0.03); }
.needs-title { font-size: 11.5px; color: rgba(255, 255, 255, 0.92); line-height: 1.4; }
.needs-reason { font-size: 10px; color: rgba(250, 204, 21, 0.7); margin-top: 2px; }

/* 按任务填报视图 */
.task-fill-list { list-style: none; margin: 0; padding: 0; }
.task-fill-item {
  padding: 6px 8px;
  margin: 4px 0;
  background: rgba(255, 255, 255, 0.03);
  border-radius: 5px;
  border-left: 2px solid rgba(59, 130, 246, 0.4);
}
.task-fill-item.orphan-item {
  border-left-color: rgba(168, 85, 247, 0.5);
  background: rgba(168, 85, 247, 0.04);
}
.orphan-commits-block {
  margin-top: 4px;
  background: rgba(0, 0, 0, 0.15);
  border-radius: 4px;
}
.orphan-commits-block .commits-summary {
  padding: 4px 8px;
  font-size: 10px;
}
.task-fill-row {
  display: flex;
  align-items: center;
  gap: 6px;
}
.task-fill-id {
  font-family: ui-monospace, monospace;
  font-size: 10.5px;
  color: rgba(147, 197, 253, 0.8);
  flex-shrink: 0;
  cursor: pointer;
}
.task-fill-id:hover { color: rgba(147, 197, 253, 1); }
.task-fill-name {
  flex: 1;
  font-size: 11.5px;
  color: rgba(255, 255, 255, 0.92);
  line-height: 1.4;
  word-break: break-word;
}
.task-fill-meta {
  display: flex;
  gap: 4px;
  align-items: center;
  margin-top: 3px;
  font-size: 9.5px;
  color: rgba(255, 255, 255, 0.45);
  padding-left: 0;
}
.muted { color: rgba(255, 255, 255, 0.3); }
.status-mark {
  margin-left: 6px;
  padding: 1px 5px;
  background: rgba(250, 204, 21, 0.18);
  color: rgba(253, 224, 71, 0.95);
  border-radius: 3px;
  font-size: 9px;
}

.copy-mini {
  width: 24px; height: 24px;
  background: rgba(59, 130, 246, 0.18);
  border: 1px solid rgba(59, 130, 246, 0.3);
  border-radius: 4px;
  color: rgba(255, 255, 255, 0.9);
  font-size: 12px;
  cursor: pointer;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
  transition: all 0.15s;
}
.copy-mini:hover { background: rgba(59, 130, 246, 0.3); }
.copy-mini.mini-ok {
  background: rgba(34, 197, 94, 0.25);
  border-color: rgba(34, 197, 94, 0.5);
  color: rgba(134, 239, 172, 0.95);
}
.copy-mini.mini-fail {
  background: rgba(239, 68, 68, 0.25);
  border-color: rgba(239, 68, 68, 0.5);
  color: rgba(252, 165, 165, 0.95);
}

/* ===== 一键写工时按钮 ===== */
.write-mini {
  height: 24px;
  padding: 0 8px;
  background: rgba(167, 139, 250, 0.18);
  border: 1px solid rgba(167, 139, 250, 0.4);
  border-radius: 4px;
  color: rgba(196, 181, 253, 0.95);
  font-size: 11px;
  font-weight: 500;
  cursor: pointer;
  display: inline-flex;
  align-items: center;
  flex-shrink: 0;
  white-space: nowrap;
  transition: all 0.15s;
}
.write-mini:hover:not(:disabled) {
  background: rgba(167, 139, 250, 0.32);
  border-color: rgba(167, 139, 250, 0.7);
}
.write-mini:disabled,
.write-mini.written {
  background: rgba(34, 197, 94, 0.18);
  border-color: rgba(34, 197, 94, 0.4);
  color: rgba(134, 239, 172, 0.95);
  cursor: not-allowed;
}

/* 业务线分组 */
.commits-block {
  background: rgba(255, 255, 255, 0.02);
  border-radius: 6px;
  padding: 0;
  margin-top: 4px;
}
.commits-summary {
  padding: 6px 8px;
  font-size: 10.5px;
  color: rgba(255, 255, 255, 0.65);
  cursor: pointer;
  user-select: none;
  list-style: none;
}
.commits-summary::-webkit-details-marker { display: none; }
.commits-summary::before {
  content: '▸';
  display: inline-block;
  margin-right: 4px;
  transition: transform 0.15s;
}
details[open] > .commits-summary::before { transform: rotate(90deg); }

.commit-list, .task-mini-list { list-style: none; margin: 0; padding: 0 8px 6px; }
.commit-item {
  padding: 4px 0;
  border-top: 1px dashed rgba(255, 255, 255, 0.04);
}
.commit-item:first-child { border-top: none; }
.commit-title { font-size: 11px; color: rgba(255, 255, 255, 0.85); line-height: 1.4; }
.commit-meta {
  display: flex; gap: 6px; align-items: center;
  margin-top: 2px;
  font-size: 9.5px;
  color: rgba(255, 255, 255, 0.4);
}
.commit-repo { color: rgba(147, 197, 253, 0.7); font-family: ui-monospace, monospace; }
.commit-sha { font-family: ui-monospace, monospace; }
.commit-time { color: rgba(255, 255, 255, 0.3); }

.task-mini-item {
  padding: 3px 0;
  font-size: 11px;
  color: rgba(255, 255, 255, 0.75);
  cursor: pointer;
  line-height: 1.4;
}
.task-mini-item:hover { color: rgba(147, 197, 253, 0.95); }

/* 原始 markdown */
.toggle-raw {
  background: transparent;
  border: none;
  color: rgba(255, 255, 255, 0.55);
  font-size: 11px;
  cursor: pointer;
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 0;
}
.toggle-raw:hover { color: rgba(255, 255, 255, 0.85); }
.markdown-pre {
  margin-top: 6px;
  padding: 8px;
  background: rgba(0, 0, 0, 0.3);
  border-radius: 4px;
  font-size: 10.5px;
  line-height: 1.5;
  color: rgba(255, 255, 255, 0.85);
  white-space: pre-wrap;
  word-break: break-word;
  max-height: 300px;
  overflow-y: auto;
  font-family: ui-monospace, monospace;
}

/* 底部 */
.panel-footer {
  padding: 8px 10px;
  background: rgba(0, 0, 0, 0.2);
  border-top: 1px solid rgba(255, 255, 255, 0.06);
}
.copy-btn {
  width: 100%;
  padding: 8px 12px;
  background: rgba(59, 130, 246, 0.2);
  color: rgba(255, 255, 255, 0.95);
  border: 1px solid rgba(59, 130, 246, 0.4);
  border-radius: 6px;
  font-size: 12px;
  cursor: pointer;
  transition: all 0.15s;
}
.copy-btn:hover { background: rgba(59, 130, 246, 0.3); }
.copy-btn.state-ok { background: rgba(34, 197, 94, 0.25); border-color: rgba(34, 197, 94, 0.5); }
.copy-btn.state-fail { background: rgba(239, 68, 68, 0.25); border-color: rgba(239, 68, 68, 0.5); }

/* 空状态 */
.empty {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 10px;
  padding: 20px;
}
.empty-icon { font-size: 36px; color: rgba(255, 255, 255, 0.4); }
.empty-icon.loading { animation: spin 1.2s linear infinite; }
.empty-icon.error { color: rgba(239, 68, 68, 0.6); }
.empty-text { font-size: 13px; color: rgba(255, 255, 255, 0.7); margin: 0; }
.empty-hint { font-size: 11px; color: rgba(255, 255, 255, 0.4); margin: 0; text-align: center; }
.empty-small { font-size: 12px; color: rgba(255, 255, 255, 0.5); text-align: center; padding: 8px 0; }
.retry-btn {
  margin-top: 4px;
  padding: 6px 14px;
  background: rgba(59, 130, 246, 0.25);
  color: rgba(255, 255, 255, 0.95);
  border: 1px solid rgba(59, 130, 246, 0.5);
  border-radius: 6px;
  font-size: 11px;
  cursor: pointer;
}

/* transitions */
.panel-enter-active, .panel-leave-active { transition: opacity 0.2s, transform 0.2s; }
.panel-enter-from, .panel-leave-to { opacity: 0; transform: translateY(6px); }
</style>
