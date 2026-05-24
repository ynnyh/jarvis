<script setup lang="ts">
import { ref, computed } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useAppStore } from '../stores/app'
import { useConfigStore } from '../stores/config'
import { useTaskCommits } from '../composables/useTaskCommits'
import type { TaskAlert } from '../stores/app'

const props = defineProps<{
  task: TaskAlert
  variant: 'danger' | 'warn' | 'soon' | 'upcoming'
}>()

const store = useAppStore()
const configStore = useConfigStore()
const { markCommitFeedback } = useTaskCommits()

const expanded = ref(false)

/**
 * 闸门 0：试发 0.01h 写回禅道。当前只对 #10257 暴露按钮，跑通后再扩面。
 * 用户原话「写回禅道这个风险比较高，可以前期尝试下，每次加个 0.01」
 */
const GATE0_TASK_ID = '10257'
const showGate0 = computed(() => props.task.id === GATE0_TASK_ID)
const showConfirm = ref(false)
const submitting = ref(false)
const submitResult = ref<'idle' | 'ok' | 'fail'>('idle')
const submitError = ref<string>('')

const commits = computed(() => store.visibleCommitsForTask(props.task.id))
const exactCount = computed(() => commits.value.filter(c => c.matchType === 'exact').length)
const softCount = computed(() => commits.value.filter(c => c.matchType === 'soft').length)

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

function openGate0Confirm(e: Event) {
  e.stopPropagation()
  submitResult.value = 'idle'
  submitError.value = ''
  showConfirm.value = true
}

async function submitGate0() {
  if (submitting.value) return
  submitting.value = true
  submitError.value = ''
  try {
    const result = await invoke<{ success: boolean; data?: any; error?: string }>('tool_execute', {
      name: 'log-task-effort',
      input: {
        taskId: props.task.id,
        hours: 0.01,
        work: `【${configStore.config.assistantName} 闸门 0 试发】验证写回链路，0.01h 占位记录`,
      },
    })
    if (result.success && result.data?.ok) {
      submitResult.value = 'ok'
      // 弹窗显示成功 2 秒后自动关
      setTimeout(() => {
        showConfirm.value = false
        // 按钮上的 ✓ 标记保留 5 秒，提醒用户去禅道验证
        setTimeout(() => { submitResult.value = 'idle' }, 5000)
      }, 1200)
    } else {
      submitResult.value = 'fail'
      submitError.value = result.error || '禅道返回未知错误'
    }
  } catch (e: any) {
    submitResult.value = 'fail'
    submitError.value = e?.message ?? String(e)
  } finally {
    submitting.value = false
  }
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
    </div>

    <!-- 闸门 0：写回链路练手按钮（仅 #10257） -->
    <div v-if="showGate0" class="gate0-row" @click.stop>
      <button
        class="gate0-btn"
        :class="`gate0-${submitResult}`"
        :title="submitResult === 'ok' ? '已发送，请去禅道确认' : '试发 0.01h 验证写回链路'"
        @click="openGate0Confirm"
      >
        <span v-if="submitResult === 'ok'">✓ 已写入</span>
        <span v-else-if="submitResult === 'fail'">× 失败，重试</span>
        <span v-else>🧪 试发 0.01h（闸门 0）</span>
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

  <!-- 二次确认弹窗（teleport 到 body 避免 panel 容器 z-index 干扰） -->
  <Teleport to="body">
    <Transition name="modal">
      <div v-if="showConfirm" class="modal-overlay pointer-target" @click.self="showConfirm = false">
        <div class="modal-card">
          <h3 class="modal-title">⚠️ 确认向禅道写入工时</h3>
          <div class="modal-body">
            <p class="modal-line"><strong>任务：</strong>#{{ task.id }} {{ task.title }}</p>
            <p class="modal-line"><strong>工时：</strong>0.01 小时</p>
            <p class="modal-line"><strong>工作内容：</strong>【{{ configStore.config.assistantName }} 闸门 0 试发】验证写回链路，0.01h 占位记录</p>
            <p class="modal-hint">
              这是闸门 0 试发，用来验证写回链路。0.01h 不影响实际工时统计。
              提交后请到禅道确认这条记录真进了任务详情。
            </p>
            <p v-if="submitResult === 'fail'" class="modal-error">
              失败：{{ submitError }}
            </p>
            <p v-if="submitResult === 'ok'" class="modal-success">
              ✓ 写入成功，去禅道核对一下吧
            </p>
          </div>
          <div class="modal-actions">
            <button class="modal-btn modal-btn-cancel" @click="showConfirm = false" :disabled="submitting">
              取消
            </button>
            <button
              class="modal-btn modal-btn-confirm"
              :disabled="submitting || submitResult === 'ok'"
              @click="submitGate0"
            >
              <span v-if="submitting">提交中…</span>
              <span v-else-if="submitResult === 'ok'">已提交</span>
              <span v-else>确认提交</span>
            </button>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
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

/* ===== 闸门 0 试发按钮 ===== */
.gate0-row {
  margin-top: 6px;
  display: flex;
  justify-content: flex-start;
}
.gate0-btn {
  font-size: 10.5px;
  padding: 3px 8px;
  border-radius: 4px;
  border: 1px dashed rgba(245, 158, 11, 0.45);
  background: rgba(245, 158, 11, 0.08);
  color: rgba(252, 211, 77, 0.95);
  cursor: pointer;
  transition: background 0.15s, color 0.15s, border-color 0.15s;
}
.gate0-btn:hover { background: rgba(245, 158, 11, 0.18); color: rgba(254, 215, 170, 1); }
.gate0-btn.gate0-ok {
  border-color: rgba(34, 197, 94, 0.5);
  background: rgba(34, 197, 94, 0.15);
  color: rgba(134, 239, 172, 0.95);
}
.gate0-btn.gate0-fail {
  border-color: rgba(239, 68, 68, 0.5);
  background: rgba(239, 68, 68, 0.15);
  color: rgba(252, 165, 165, 0.95);
}

/* ===== 二次确认 Modal ===== */
.modal-overlay {
  position: fixed; inset: 0;
  background: rgba(0, 0, 0, 0.5);
  z-index: 200;
  display: flex; align-items: center; justify-content: center;
  padding: 20px;
  backdrop-filter: blur(2px);
}
.modal-card {
  width: 100%; max-width: 360px;
  background: linear-gradient(135deg, rgba(20, 30, 56, 0.99), rgba(15, 23, 42, 0.99));
  border: 1px solid rgba(245, 158, 11, 0.4);
  border-radius: 12px;
  box-shadow: 0 12px 32px rgba(0, 0, 0, 0.6);
  overflow: hidden;
  color: rgba(255, 255, 255, 0.92);
}
.modal-title {
  margin: 0;
  padding: 12px 16px;
  font-size: 13px;
  font-weight: 600;
  background: rgba(245, 158, 11, 0.12);
  color: rgba(254, 215, 170, 0.95);
  border-bottom: 1px solid rgba(245, 158, 11, 0.2);
}
.modal-body {
  padding: 14px 16px;
  display: flex; flex-direction: column; gap: 6px;
  font-size: 11.5px;
  line-height: 1.55;
}
.modal-line { margin: 0; color: rgba(255, 255, 255, 0.85); word-break: break-word; }
.modal-line strong { color: rgba(147, 197, 253, 0.9); margin-right: 4px; font-weight: 600; }
.modal-hint {
  margin: 6px 0 0;
  padding: 6px 8px;
  font-size: 10.5px;
  color: rgba(255, 255, 255, 0.5);
  background: rgba(255, 255, 255, 0.03);
  border-radius: 4px;
  line-height: 1.55;
}
.modal-error {
  margin: 6px 0 0;
  padding: 6px 8px;
  font-size: 10.5px;
  color: rgba(252, 165, 165, 0.95);
  background: rgba(239, 68, 68, 0.12);
  border-left: 2px solid rgba(239, 68, 68, 0.5);
  border-radius: 4px;
  word-break: break-word;
}
.modal-success {
  margin: 6px 0 0;
  padding: 6px 8px;
  font-size: 10.5px;
  color: rgba(134, 239, 172, 0.95);
  background: rgba(34, 197, 94, 0.12);
  border-left: 2px solid rgba(34, 197, 94, 0.5);
  border-radius: 4px;
}
.modal-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  padding: 10px 16px 14px;
  border-top: 1px solid rgba(255, 255, 255, 0.04);
}
.modal-btn {
  padding: 6px 14px;
  font-size: 11.5px;
  border-radius: 5px;
  cursor: pointer;
  border: 1px solid transparent;
  transition: background 0.15s, color 0.15s, border-color 0.15s;
}
.modal-btn:disabled { cursor: not-allowed; opacity: 0.5; }
.modal-btn-cancel {
  background: transparent;
  color: rgba(255, 255, 255, 0.7);
  border-color: rgba(255, 255, 255, 0.15);
}
.modal-btn-cancel:hover:not(:disabled) { background: rgba(255, 255, 255, 0.06); color: rgba(255, 255, 255, 0.95); }
.modal-btn-confirm {
  background: rgba(245, 158, 11, 0.25);
  color: rgba(254, 215, 170, 0.98);
  border-color: rgba(245, 158, 11, 0.5);
}
.modal-btn-confirm:hover:not(:disabled) { background: rgba(245, 158, 11, 0.4); }

.modal-enter-active, .modal-leave-active { transition: opacity 0.2s; }
.modal-enter-active .modal-card, .modal-leave-active .modal-card { transition: transform 0.2s, opacity 0.2s; }
.modal-enter-from, .modal-leave-to { opacity: 0; }
.modal-enter-from .modal-card, .modal-leave-to .modal-card { transform: translateY(8px) scale(0.97); opacity: 0; }
</style>
