<script setup lang="ts">
import { ref, computed } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useAppStore } from '../stores/app'
import { useTaskCommits } from '../composables/useTaskCommits'
import type { TaskAlert } from '../stores/app'

const props = defineProps<{
  task: TaskAlert
  variant: 'danger' | 'warn' | 'soon' | 'upcoming'
}>()

const store = useAppStore()
const { markCommitFeedback } = useTaskCommits()

const expanded = ref(false)

const commits = computed(() => store.visibleCommitsForTask(props.task.id))
const exactCount = computed(() => commits.value.filter(c => c.matchType === 'exact').length)
const softCount = computed(() => commits.value.filter(c => c.matchType === 'soft').length)

// 绑定状态：未绑定显示灰色 🔗，已绑定显示绿色 ✓ 项目名缩写
const bindingEntry = computed(() => store.taskBindings[props.task.id] ?? null)
const isBound = computed(() => store.isTaskBound(props.task.id))
const boundRepoLabel = computed(() => {
  if (!bindingEntry.value) return ''
  const roots = bindingEntry.value.repoRoots
  if (roots.length === 0) return ''
  // 取最后一层目录名作为短标签；多仓显示 "N 个项目"
  if (roots.length === 1) {
    const parts = roots[0].replace(/\\/g, '/').split('/').filter(Boolean)
    return parts[parts.length - 1] || roots[0]
  }
  return `${roots.length} 个项目`
})

function openBindWindow(e: Event) {
  e.stopPropagation()
  // 把当前任务推到队列首位（unshift 等价：清掉再 push），然后打开绑定窗。
  // 用 enqueue + 立即打开的方式，确保即使队列里有其它任务，用户手动触发的也会被优先处理。
  // 实现上：直接重写 pendingBindTasks，避免引入额外 store 方法。
  const t = {
    id: props.task.id,
    title: props.task.title,
    priority: props.task.priority,
    deadline: props.task.deadline,
  }
  // 去掉队列里同 id 的旧条目，把当前任务塞到队首
  const rest = store.pendingBindTasks.filter(x => x.id !== t.id)
  store.pendingBindTasks.splice(0, store.pendingBindTasks.length, t, ...rest)
  store.showBindTaskWindow = true
}

const dueLabel = computed(() => {
  const d = props.task.daysUntilDue
  if (d < 0) return `逾期 ${-d} 天`
  if (d === 0) return '今天到期'
  if (d === 1) return '明天到期'
  return `${d} 天后到期`
})

async function openTask() {
  try {
    await invoke('open_zentao_task', { id: props.task.id })
  } catch (e) {
    console.error('打开禅道任务失败:', e)
  }
}

function toggleExpand(e: Event) {
  e.stopPropagation()
  expanded.value = !expanded.value
}

function formatTime(iso: string): string {
  // 2026-05-21T17:41:06+08:00 → 05-21 17:41
  const m = iso.match(/(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})/)
  if (!m) return iso
  return `${m[2]}-${m[3]} ${m[4]}:${m[5]}`
}

function onAccept(sha: string, e: Event) {
  e.stopPropagation()
  markCommitFeedback(props.task.id, sha, 'accepted')
}

function onReject(sha: string, e: Event) {
  e.stopPropagation()
  markCommitFeedback(props.task.id, sha, 'rejected')
}

function feedbackOf(sha: string): 'accepted' | 'rejected' | undefined {
  return store.commitFeedback[`${props.task.id}|${sha}`]
}
</script>

<template>
  <li class="task-item" :class="variant" @click="openTask" title="点击打开禅道">
    <div class="task-row1">
      <span class="task-title">{{ task.title }}</span>
      <span class="task-badge" :class="`badge-${variant}`">{{ dueLabel }}</span>
    </div>
    <div class="task-row2">
      <span>📅 {{ task.deadline }}</span>
      <span class="muted">· {{ task.status === 'doing' ? '进行中' : '未开始' }}</span>
      <span v-if="task.isTeam" class="team-tag" title="团队任务，工时为你个人">👥</span>
      <span v-if="task.estimatedHours > 0" class="muted">
        · {{ task.consumedHours }}/{{ task.estimatedHours }}h
      </span>
      <button
        class="bind-chip"
        :class="{ bound: isBound, unbound: !isBound }"
        :title="isBound ? `已绑定：${boundRepoLabel}（点击修改）` : '未绑定项目，点击绑定'"
        @click="openBindWindow"
      >
        <span class="bind-chip-icon">{{ isBound ? '✓' : '🔗' }}</span>
        <span class="bind-chip-text">{{ isBound ? boundRepoLabel : '未绑定' }}</span>
      </button>
    </div>

    <!-- 📌 相关提交 折叠区 -->
    <div v-if="commits.length > 0" class="commits-section" :class="{ expanded }">
      <button class="commits-toggle" @click="toggleExpand">
        <span class="commits-icon">📌</span>
        <span>{{ commits.length }} 条相关提交</span>
        <span v-if="exactCount > 0" class="commit-tag tag-exact">{{ exactCount }} 精确</span>
        <span v-if="softCount > 0" class="commit-tag tag-soft">{{ softCount }} 软关联</span>
        <span class="commits-arrow">{{ expanded ? '▾' : '▸' }}</span>
      </button>
      <ul v-if="expanded" class="commit-list" @click.stop>
        <li
          v-for="c in commits"
          :key="c.sha"
          class="commit-item"
          :class="[`commit-${c.matchType}`, feedbackOf(c.sha) ? `fb-${feedbackOf(c.sha)}` : '']"
        >
          <div class="commit-row1">
            <span class="commit-repo">[{{ c.businessLine }}/{{ c.repoName }}]</span>
            <span class="commit-title">{{ c.title }}</span>
          </div>
          <div class="commit-row2">
            <span class="commit-sha">{{ c.shortSha }}</span>
            <span class="commit-date">{{ formatTime(c.authoredDate) }}</span>
            <span v-if="c.matchType === 'exact'" class="commit-tag tag-exact">✓ 精确</span>
            <span v-else-if="c.matchedKeywords?.length" class="commit-tag tag-soft" :title="c.matchedKeywords.join(', ')">
              关键词: {{ c.matchedKeywords.join('/') }}
            </span>
            <span v-if="c.matchType === 'soft' && !feedbackOf(c.sha)" class="commit-actions">
              <button class="fb-btn fb-yes" @click="onAccept(c.sha, $event)" title="确认是这个任务">✓</button>
              <button class="fb-btn fb-no" @click="onReject(c.sha, $event)" title="不是这个任务">✗</button>
            </span>
            <span v-else-if="feedbackOf(c.sha) === 'accepted'" class="fb-mark fb-mark-yes">已确认</span>
          </div>
        </li>
      </ul>
    </div>
  </li>
</template>

<style scoped>
.task-item {
  padding: 8px 10px;
  margin-bottom: 6px;
  background: rgba(255, 255, 255, 0.04);
  border-left: 3px solid;
  border-radius: 6px;
  cursor: pointer;
  transition: background 0.15s;
}
.task-item:hover { background: rgba(255, 255, 255, 0.08); }
.task-item.danger { border-left-color: #ef4444; }
.task-item.warn   { border-left-color: #f59e0b; }
.task-item.soon   { border-left-color: #3b82f6; }
.task-item.upcoming { border-left-color: #6b7280; }

.task-row1 { display: flex; justify-content: space-between; gap: 8px; align-items: center; }
.task-title { font-size: 13px; color: #e5e7eb; line-height: 1.4; }
.task-badge {
  font-size: 10px;
  padding: 2px 6px;
  border-radius: 4px;
  white-space: nowrap;
  flex-shrink: 0;
}
.badge-danger { background: rgba(239, 68, 68, 0.2); color: #fca5a5; }
.badge-warn   { background: rgba(245, 158, 11, 0.2); color: #fcd34d; }
.badge-soon   { background: rgba(59, 130, 246, 0.2); color: #93c5fd; }
.badge-upcoming { background: rgba(107, 114, 128, 0.2); color: #9ca3af; }

.task-row2 {
  font-size: 11px;
  color: #9ca3af;
  margin-top: 4px;
  display: flex;
  gap: 4px;
  align-items: center;
}
.muted { color: #6b7280; }
.team-tag { font-size: 10px; }

/* ===== 绑定状态角标 ===== */
.bind-chip {
  margin-left: auto;
  display: inline-flex;
  align-items: center;
  gap: 3px;
  padding: 1px 6px;
  font-size: 10px;
  border-radius: 8px;
  border: 1px solid;
  cursor: pointer;
  transition: background 0.12s, border-color 0.12s, color 0.12s;
  font-family: inherit;
}
.bind-chip.unbound {
  background: rgba(148, 163, 184, 0.08);
  border-color: rgba(148, 163, 184, 0.25);
  color: rgba(148, 163, 184, 0.85);
}
.bind-chip.unbound:hover {
  background: rgba(167, 139, 250, 0.12);
  border-color: rgba(167, 139, 250, 0.45);
  color: rgba(167, 139, 250, 0.95);
}
.bind-chip.bound {
  background: rgba(34, 197, 94, 0.1);
  border-color: rgba(34, 197, 94, 0.3);
  color: rgba(134, 239, 172, 0.95);
}
.bind-chip.bound:hover {
  background: rgba(34, 197, 94, 0.18);
  border-color: rgba(34, 197, 94, 0.55);
}
.bind-chip-icon { font-size: 9.5px; line-height: 1; }
.bind-chip-text {
  max-width: 110px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* ----- 相关提交折叠区 ----- */
.commits-section {
  margin-top: 6px;
  border-top: 1px dashed rgba(255, 255, 255, 0.08);
  padding-top: 6px;
}
.commits-toggle {
  display: flex;
  align-items: center;
  gap: 6px;
  width: 100%;
  background: transparent;
  border: none;
  color: #93c5fd;
  font-size: 11px;
  padding: 2px 0;
  cursor: pointer;
  text-align: left;
}
.commits-toggle:hover { color: #bfdbfe; }
.commits-icon { font-size: 12px; }
.commits-arrow { margin-left: auto; font-size: 10px; opacity: 0.6; }

.commit-tag {
  font-size: 9px;
  padding: 1px 4px;
  border-radius: 3px;
}
.tag-exact { background: rgba(34, 197, 94, 0.2); color: #86efac; }
.tag-soft  { background: rgba(234, 179, 8, 0.18); color: #fde68a; }

.commit-list {
  list-style: none;
  margin: 6px 0 0;
  padding: 0;
}
.commit-item {
  padding: 6px 8px;
  margin: 4px 0;
  border-radius: 4px;
  border-left: 2px solid;
  background: rgba(255, 255, 255, 0.03);
}
.commit-exact { border-left-color: #22c55e; }
.commit-soft  { border-left-color: #eab308; }
.commit-item.fb-rejected { opacity: 0.4; }

.commit-row1 {
  display: flex;
  gap: 6px;
  align-items: baseline;
  font-size: 11px;
}
.commit-repo {
  color: #94a3b8;
  font-family: ui-monospace, monospace;
  font-size: 10px;
  flex-shrink: 0;
}
.commit-title { color: #e5e7eb; line-height: 1.4; word-break: break-all; }

.commit-row2 {
  display: flex;
  gap: 6px;
  align-items: center;
  margin-top: 3px;
  font-size: 10px;
  color: #94a3b8;
}
.commit-sha { font-family: ui-monospace, monospace; color: #64748b; }
.commit-date { color: #6b7280; }

.commit-actions { display: inline-flex; gap: 4px; margin-left: auto; }
.fb-btn {
  width: 18px; height: 18px;
  border: 1px solid rgba(255, 255, 255, 0.15);
  background: rgba(255, 255, 255, 0.04);
  color: #cbd5e1;
  border-radius: 3px;
  cursor: pointer;
  font-size: 11px;
  padding: 0;
  display: inline-flex;
  align-items: center;
  justify-content: center;
}
.fb-yes:hover { background: rgba(34, 197, 94, 0.25); color: #86efac; border-color: #22c55e; }
.fb-no:hover  { background: rgba(239, 68, 68, 0.25); color: #fca5a5; border-color: #ef4444; }

.fb-mark { margin-left: auto; font-size: 9px; padding: 1px 5px; border-radius: 3px; }
.fb-mark-yes { background: rgba(34, 197, 94, 0.2); color: #86efac; }
</style>
