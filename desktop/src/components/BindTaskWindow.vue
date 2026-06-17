<script setup lang="ts">
// 任务绑定窗：把新任务跟本地代码项目（repoRoot）关联起来。
//
// 来源：
// - 后端 "new-tasks-detected" 事件填入 store.pendingBindTasks 队列
// - 用户点任务卡上的"未绑定"图标手动触发（也是往队列里塞一条然后打开窗口）
//
// 流程：
// 1. 默认展示项目列表（不调 LLM），用户手动选择归属项目
// 2. 用户可点击"AI 推荐"按钮主动触发 LLM 分析，获取评分排序
// 3. 点"确认绑定"→ task_bindings_set 落盘 → 出队
// 4. 点"暂不绑定" / 关闭按钮 → 不写绑定 → 出队
// 5. 出队后若 queue 还有任务，继续处理；空则关窗

import { ref, computed, watch, onMounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useAppStore } from '../stores/app'
import { useConfigStore } from '../stores/config'
import { useExpandedAvatarWindow } from '../composables/useExpandedAvatarWindow'

interface RepoRecommendation {
  repoRoot: string
  score: number
  reason: string
  isTop: boolean
}

const store = useAppStore()
const configStore = useConfigStore()

const recommendations = ref<RepoRecommendation[]>([])
const loading = ref(false)
const error = ref<string | null>(null)
const selectedRepos = ref<Set<string>>(new Set())
const multiSelectMode = ref(false)
const saving = ref(false)
const aiRequested = ref(false)

const currentTask = computed(() => store.pendingBindTasks[0] || null)
const queueRest = computed(() => Math.max(0, store.pendingBindTasks.length - 1))

const repoRoots = computed<string[]>(() => configStore.config.repoRoots ?? [])

// 默认项目列表（不调 LLM）：直接用 repoRoots 生成，无评分
const plainRecommendations = computed<RepoRecommendation[]>(() =>
  repoRoots.value.map(r => ({ repoRoot: r, score: 0, reason: '', isTop: false })),
)

// 当前展示的列表：AI 已请求则用 AI 结果，否则用 repoRoots + 手动选择 + 已绑定的目录
const displayRecommendations = computed(() => {
  const base = aiRequested.value ? [...recommendations.value] : [...plainRecommendations.value]
  const existingPaths = new Set(base.map(r => r.repoRoot))
  // 补上手动选择的目录（通过「浏览选择其它目录」添加的）
  if (!aiRequested.value) {
    for (const entry of recommendations.value) {
      if (entry.reason === '手动选择' && !existingPaths.has(entry.repoRoot)) {
        base.push(entry)
        existingPaths.add(entry.repoRoot)
      }
    }
  }
  // 补上已选中但不在当前列表里的目录（比如之前通过浏览绑定过的非 repoRoots 路径）
  for (const root of selectedRepos.value) {
    if (!existingPaths.has(root)) {
      base.push({ repoRoot: root, score: 0, reason: '已绑定', isTop: false })
      existingPaths.add(root)
    }
  }
  return base
})

async function requestAiRecommendation() {
  const task = currentTask.value
  if (!task) return
  aiRequested.value = true
  loading.value = true
  error.value = null
  try {
    const res = await invoke<RepoRecommendation[]>('recommend_repos_for_task', {
      taskTitle: task.title,
      taskDescription: '',
      deadline: task.deadline,
      repoRoots: repoRoots.value,
    })
    recommendations.value = res
    // AI 推荐出来后默认选中 top1
    const top = res.find(r => r.isTop) || res[0]
    selectedRepos.value = new Set(top ? [top.repoRoot] : [])
    multiSelectMode.value = false
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e)
    recommendations.value = []
  } finally {
    loading.value = false
  }
}

function toggleRepo(repoRoot: string) {
  if (multiSelectMode.value) {
    const s = new Set(selectedRepos.value)
    if (s.has(repoRoot)) s.delete(repoRoot)
    else s.add(repoRoot)
    selectedRepos.value = s
  } else {
    // 单选模式：直接替换
    selectedRepos.value = new Set([repoRoot])
  }
}

function enterMultiMode() {
  multiSelectMode.value = true
}

/**
 * 浏览选择任意目录。覆盖三种场景：
 * - 自动扫到的 git 仓都不对（项目还没初始化 git）
 * - 想绑定到非 git 项目（纯文档 / 配置仓）
 * - 临时项目放在 repoRoots 之外
 *
 * 选中后插入推荐列表（手动条目无 score/AI 推荐角标），并自动勾上。
 */
async function browseAndPickRepo() {
  try {
    const picked = await invoke<string | null>('pick_directory', {
      title: '选择要绑定的项目目录',
    })
    if (!picked) return  // 用户取消

    // 已经在 repoRoots 里就只是勾上，不重复添加
    const existing = repoRoots.value.find(r => r === picked)
    if (existing) {
      toggleRepo(picked)
      return
    }

    const manualEntry: RepoRecommendation = {
      repoRoot: picked,
      score: 0,            // 0 分但标"手动选择"，渲染层会跳过分数
      reason: '手动选择',
      isTop: false,
    }
    recommendations.value = [...recommendations.value, manualEntry]

    // 自动勾上手动选择的项；单选模式下顶替原选择
    if (multiSelectMode.value) {
      const s = new Set(selectedRepos.value)
      s.add(picked)
      selectedRepos.value = s
    } else {
      selectedRepos.value = new Set([picked])
    }
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e)
  }
}

async function handleConfirm() {
  const task = currentTask.value
  if (!task || saving.value) return
  const repos = Array.from(selectedRepos.value)
  if (repos.length === 0) {
    error.value = '至少选一个项目，或点"暂不绑定"跳过这条任务'
    return
  }
  saving.value = true
  try {
    // confirmedBy 反映这次绑定的来源
    const top = aiRequested.value ? recommendations.value.find(r => r.isTop) : null
    const isOneClick = aiRequested.value && repos.length === 1 && top?.repoRoot === repos[0]
    const confirmedBy = isOneClick
      ? 'llm-1click'
      : (repos.length > 1 ? 'manual-multi' : 'manual')
    await invoke('task_bindings_set', {
      taskId: task.id,
      repoRoots: repos,
      lastConfirmedBy: confirmedBy,
    })
    // 立即刷新绑定表，让任务卡上的"未绑定"图标实时变绿
    await store.refreshTaskBindings()
    advanceQueue()
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e)
  } finally {
    saving.value = false
  }
}

function handleSkip() {
  if (saving.value) return
  advanceQueue()
}

function advanceQueue() {
  store.dequeueBindTask()
  if (store.pendingBindTasks.length === 0) {
    store.showBindTaskWindow = false
  }
  // 否则 watch(currentTask) 会重新拉推荐
}

// 绑定窗打开或切换任务时重置选中状态，优先预填已有绑定。
// 必须同时 watch currentTask 和 showBindTaskWindow：前者是任务切换（队列前进），
// 后者是窗口打开（用户点击任务卡上的绑定按钮时 currentTask 可能不变）。
watch([currentTask, () => store.showBindTaskWindow], ([t, showing]) => {
  if (!showing || !t) {
    selectedRepos.value = new Set()
    return
  }
  aiRequested.value = false
  recommendations.value = []
  error.value = null
  // 已有绑定则预填
  const existing = store.taskBindings[t.id]
  const roots = existing?.repoRoots?.filter(r => r) ?? []
  if (roots.length > 0) {
    selectedRepos.value = new Set(roots)
  } else if (repoRoots.value.length > 0) {
    selectedRepos.value = new Set([repoRoots.value[0]])
  } else {
    selectedRepos.value = new Set()
  }
}, { immediate: false })

// 绑定窗打开期间把 avatar 窗口撑大到 640×720，关闭恢复 400×560，
// 不直接看到 avatar 容器的小尺寸把字挤得难以辨认。
useExpandedAvatarWindow(computed(() => store.showBindTaskWindow))

onMounted(() => {
  const t = currentTask.value
  if (t) {
    const existing = store.taskBindings[t.id]
    const roots = existing?.repoRoots?.filter(r => r) ?? []
    if (roots.length > 0) {
      selectedRepos.value = new Set(roots)
    } else if (repoRoots.value.length > 0) {
      selectedRepos.value = new Set([repoRoots.value[0]])
    }
  }
})

function shortPath(p: string): string {
  // 显示用：路径太长截两端，保留最后两层目录
  if (p.length <= 36) return p
  const parts = p.replace(/\\/g, '/').split('/').filter(Boolean)
  if (parts.length <= 2) return p
  return '…/' + parts.slice(-2).join('/')
}

function priorityLabel(p: string): string {
  return p === 'urgent' ? '紧急' : p === 'high' ? '高' : p === 'low' ? '低' : '普通'
}
</script>

<template>
  <Transition name="panel">
    <div v-if="store.showBindTaskWindow && currentTask" class="bind-panel pointer-target">
      <header class="panel-header">
        <div class="panel-title">
          <span class="title-icon">🔗</span>
          <span class="title-text">关联任务到项目</span>
          <span v-if="queueRest > 0" class="queue-badge">还有 {{ queueRest }} 条待处理</span>
        </div>
        <button class="icon-btn" :disabled="saving" title="暂不绑定" @click="handleSkip">×</button>
      </header>

      <div class="panel-body">
        <!-- 任务卡 -->
        <div class="task-card">
          <div class="task-title">{{ currentTask.title }}</div>
          <div class="task-meta">
            <span class="meta-pill" :class="'priority-' + currentTask.priority">{{ priorityLabel(currentTask.priority) }}</span>
            <span v-if="currentTask.deadline" class="meta-pill meta-deadline">⏰ {{ currentTask.deadline }}</span>
            <span class="meta-pill meta-id">#{{ currentTask.id }}</span>
          </div>
        </div>

        <!-- 状态 / 错误 -->
        <div v-if="loading" class="phase-text">
          <span class="spinner" /> AI 正在分析任务和项目的相关度…
        </div>
        <div v-if="error" class="phase-text phase-error">{{ error }}</div>

        <!-- 推荐列表 -->
        <div v-if="!loading && displayRecommendations.length > 0" class="rec-list">
          <div class="rec-header">
            <span class="rec-tip">{{ multiSelectMode ? '可多选，跨仓任务关联多个项目' : '选择该任务归属的项目' }}</span>
            <button
              v-if="!aiRequested"
              class="ai-btn"
              :disabled="loading"
              @click="requestAiRecommendation"
            >
              AI 推荐
            </button>
          </div>
          <button
            v-for="r in displayRecommendations"
            :key="r.repoRoot"
            class="rec-row"
            :class="{ selected: selectedRepos.has(r.repoRoot), top: r.isTop }"
            @click="toggleRepo(r.repoRoot)"
          >
            <span class="rec-check">{{ selectedRepos.has(r.repoRoot) ? '●' : '○' }}</span>
            <div class="rec-main">
              <div class="rec-line1">
                <span class="rec-path" :title="r.repoRoot">{{ shortPath(r.repoRoot) }}</span>
                <span v-if="r.isTop" class="rec-badge">AI 推荐</span>
                <span v-if="r.reason === '手动选择'" class="rec-badge rec-badge-manual">手动</span>
              </div>
              <div v-if="r.reason" class="rec-reason">{{ r.reason }}</div>
            </div>
            <span v-if="aiRequested && r.reason !== '手动选择'" class="rec-score">{{ r.score }}</span>
          </button>
          <div class="rec-actions">
            <button v-if="!multiSelectMode && displayRecommendations.length > 1" class="action-link" @click="enterMultiMode">
              + 再关联一个项目（跨仓任务）
            </button>
            <button class="action-link browse-link" @click="browseAndPickRepo" title="选择任意目录（非 git 项目也可）">
              📁 浏览选择其它目录
            </button>
          </div>
        </div>
      </div>

      <footer class="panel-footer">
        <button
          class="primary-btn"
          :disabled="saving || loading || selectedRepos.size === 0"
          @click="handleConfirm"
        >
          {{ saving ? '保存中…' : '确认绑定' }}
        </button>
        <button class="secondary-btn" :disabled="saving" @click="handleSkip">暂不绑定</button>
      </footer>
    </div>
  </Transition>
</template>

<style scoped>
.bind-panel {
  position: fixed;
  inset: var(--panel-top, 8px) var(--panel-right, 8px) var(--panel-bottom, 90px) var(--panel-left, 8px);
  display: flex;
  flex-direction: column;
  background: var(--popup-bg);
  backdrop-filter: blur(20px);
  -webkit-backdrop-filter: blur(20px);
  border: var(--panel-border);
  border-radius: var(--radius-lg);
  box-shadow: var(--panel-shadow);
  overflow: hidden;
  z-index: 58;
  color: var(--text);
}

.panel-header {
  display: flex; align-items: center; justify-content: space-between;
  padding: 8px 10px;
  background: var(--panel-header-bg);
  border-bottom: var(--panel-header-border);
}
.panel-title { display: flex; align-items: center; gap: 6px; font-size: 15px; font-weight: 600; }
.title-icon { font-size: 16px; }
.queue-badge {
  margin-left: 6px;
  padding: 2px 7px;
  font-size: 11.5px;
  font-weight: 500;
  color: var(--purple-text);
  background: var(--purple-bg);
  border-radius: var(--radius-sm);
}

.icon-btn {
  width: 22px; height: 22px;
  display: inline-flex; align-items: center; justify-content: center;
  font-size: 16px; line-height: 1;
  color: var(--text-dim);
  background: transparent; border: none; border-radius: var(--radius-control);
  cursor: pointer;
}
.icon-btn:hover:not(:disabled) { color: var(--text); background: var(--surface-item-hover); }
.icon-btn:disabled { cursor: not-allowed; opacity: 0.35; }

.panel-body {
  flex: 1;
  overflow-y: auto;
  padding: 12px;
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.task-card {
  padding: 12px 14px;
  background: var(--panel-header-bg);
  border: var(--divider);
  border-radius: var(--radius-md);
}
.task-title {
  font-size: 15px;
  font-weight: 600;
  line-height: 1.45;
  color: var(--text);
}
.task-meta {
  margin-top: 8px;
  display: flex; flex-wrap: wrap; gap: 5px;
}
.meta-pill {
  padding: 2px 8px;
  font-size: 12px;
  border-radius: var(--radius-sm);
  background: var(--input-bg);
  color: var(--text-ghost);
}
.meta-pill.priority-urgent { color: var(--red-text); background: var(--red-bg); }
.meta-pill.priority-high { color: var(--yellow-text); background: var(--yellow-bg); }
.meta-pill.priority-low { color: var(--text-dim); background: var(--surface-2); }
.meta-pill.meta-deadline { color: var(--accent-text); background: var(--accent-glow); }
.meta-pill.meta-id { font-family: ui-monospace, SFMono-Regular, monospace; color: var(--text-muted); }

.phase-text {
  display: flex; align-items: center; gap: 6px; justify-content: center;
  padding: 10px 12px;
  font-size: 13.5px;
  text-align: center;
  border-radius: var(--radius-md);
  background: var(--panel-header-bg);
  color: var(--text-ghost);
}
.phase-text.phase-error { color: var(--red-text); background: var(--red-bg); }

.spinner {
  width: 10px; height: 10px;
  border: 1.5px solid rgba(255, 255, 255, 0.2);
  border-top-color: rgba(100, 200, 255, 0.9);
  border-radius: 50%;
  animation: spin 0.7s linear infinite;
  flex-shrink: 0;
}
@keyframes spin { from { transform: rotate(0); } to { transform: rotate(360deg); } }

.rec-list {
  display: flex; flex-direction: column; gap: 6px;
}
.rec-header {
  display: flex; justify-content: space-between; align-items: center;
  font-size: 12.5px;
  color: var(--text-dim);
  padding: 0 2px;
}
.ai-btn {
  padding: 3px 10px;
  font-size: 11.5px;
  font-weight: 500;
  color: var(--purple-text);
  background: var(--purple-bg);
  border: 1px solid var(--purple-border);
  border-radius: var(--radius-control);
  cursor: pointer;
  transition: all 0.15s;
}
.ai-btn:hover:not(:disabled) {
  background: var(--purple-bg-strong);
  border-color: var(--purple-border);
}
.ai-btn:disabled { opacity: 0.4; cursor: not-allowed; }

.rec-row {
  display: flex; align-items: center; gap: 10px;
  padding: 10px 12px;
  text-align: left;
  background: var(--panel-header-bg);
  border: var(--divider);
  border-radius: var(--radius-md);
  cursor: pointer;
  transition: all 0.12s;
  color: inherit;
}
.rec-row:hover { background: var(--surface-2); border-color: var(--accent-border); }
.rec-row.selected { background: var(--accent-glow); border-color: var(--accent-border); box-shadow: var(--shadow-1); }
.rec-row.top:not(.selected) { border-color: var(--purple-border); }

.rec-check {
  width: 16px;
  font-size: 16px;
  color: var(--text-dim);
  text-align: center;
}
.rec-row.selected .rec-check { color: var(--accent-text); }

.rec-main { flex: 1; min-width: 0; }
.rec-line1 { display: flex; align-items: center; gap: 6px; }
.rec-path {
  font-size: 14px;
  font-weight: 500;
  font-family: ui-monospace, SFMono-Regular, monospace;
  color: var(--text);
  overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
}
.rec-badge {
  padding: 2px 6px;
  font-size: 11px;
  font-weight: 600;
  color: var(--purple-text);
  background: var(--purple-bg);
  border-radius: var(--radius-sm);
  letter-spacing: 0.4px;
  flex-shrink: 0;
}
.rec-badge-manual {
  color: var(--yellow-text);
  background: var(--yellow-bg);
}
.rec-reason {
  margin-top: 3px;
  font-size: 12.5px;
  color: var(--text-dim);
  overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
}

.rec-score {
  width: 38px;
  font-size: 14px;
  font-weight: 600;
  font-family: ui-monospace, SFMono-Regular, monospace;
  text-align: right;
  color: var(--text-ghost);
}
.rec-row.selected .rec-score { color: var(--accent-text); }

.rec-actions {
  display: flex;
  flex-direction: column;
  gap: 4px;
  margin-top: 2px;
}
.action-link {
  padding: 7px 10px;
  font-size: 12.5px;
  background: transparent;
  border: 1px dashed;
  border-radius: var(--radius-control);
  cursor: pointer;
  text-align: center;
  font-family: inherit;
}
.action-link:not(.browse-link) {
  color: var(--purple-text);
  border-color: var(--purple-border);
}
.action-link:not(.browse-link):hover {
  background: var(--purple-bg);
  color: var(--purple-text);
}
.browse-link {
  color: var(--yellow-text);
  border-color: var(--yellow-border);
}
.browse-link:hover {
  background: var(--yellow-bg);
  color: var(--yellow-text);
}

.panel-footer {
  display: flex;
  gap: 6px;
  padding: 8px 10px;
  background: var(--panel-header-bg);
  border-top: var(--divider);
}
.primary-btn {
  flex: 1;
  padding: 10px 14px;
  font-size: 13.5px;
  font-weight: 500;
  color: white;
  background: var(--btn-primary-bg);
  border: none;
  border-radius: var(--radius-control);
  cursor: pointer;
  transition: all 0.15s;
}
.primary-btn:hover:not(:disabled) {
  box-shadow: 0 4px 12px rgba(0, 212, 255, 0.3);
  transform: translateY(-1px);
}
.primary-btn:disabled {
  background: var(--surface-item-hover);
  color: var(--text-muted);
  cursor: not-allowed;
}
.secondary-btn {
  padding: 10px 18px;
  font-size: 13.5px;
  color: var(--text-ghost);
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: var(--radius-control);
  cursor: pointer;
}
.secondary-btn:hover:not(:disabled) {
  color: var(--text);
  background: var(--surface-item-active);
}
.secondary-btn:disabled { opacity: 0.35; cursor: not-allowed; }

.panel-enter-active,
.panel-leave-active {
  transition: opacity 0.2s ease, transform 0.2s ease;
}
.panel-enter-from,
.panel-leave-to {
  opacity: 0;
  transform: translateY(6px) scale(0.98);
}
</style>
