<script setup lang="ts">
// 任务绑定窗：把新任务跟本地代码项目（repoRoot）关联起来。
//
// 来源：
// - 后端 "new-tasks-detected" 事件填入 store.pendingBindTasks 队列
// - 用户点任务卡上的"未绑定"图标手动触发（也是往队列里塞一条然后打开窗口）
//
// 流程：
// 1. 拿队首任务 → 调 recommend_repos_for_task → LLM 给出 repo 评分排序
// 2. 默认勾选 top1（AI 推荐），用户也能切换到其它 repo 或开多选
// 3. 点"确认绑定"→ task_bindings_set 落盘 → 出队
// 4. 点"暂不绑定" / 关闭按钮 → 不写绑定 → 出队
//    （存量任务不会被反复弹，task_snapshot 已记录过这个 id 不再触发事件；
//     用户可通过任务卡上的图标主动重弹）
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

const currentTask = computed(() => store.pendingBindTasks[0] || null)
const queueRest = computed(() => Math.max(0, store.pendingBindTasks.length - 1))

const repoRoots = computed<string[]>(() => configStore.config.repoRoots ?? [])

async function fetchRecommendations() {
  const task = currentTask.value
  if (!task) return
  const roots = repoRoots.value
  if (!roots || roots.length === 0) {
    error.value = '尚未配置代码文件夹（设置 → 代码根目录）。无法绑定项目。'
    recommendations.value = []
    return
  }
  loading.value = true
  error.value = null
  try {
    const res = await invoke<RepoRecommendation[]>('recommend_repos_for_task', {
      taskTitle: task.title,
      taskDescription: '',          // 当前禅道 list 接口不带 desc，后续可以补一次 get-task
      deadline: task.deadline,
      repoRoots: roots,
    })
    recommendations.value = res
    // 默认选中 top1
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

    // 已经在列表里就只是勾上，不重复添加
    const existing = recommendations.value.find(r => r.repoRoot === picked)
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
    // confirmedBy 反映这次绑定的来源，后续可以根据它分析 AI 推荐命中率
    const top = recommendations.value.find(r => r.isTop)
    const isOneClick = repos.length === 1 && top?.repoRoot === repos[0]
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

// 任务切换时重新拉推荐
watch(currentTask, (t) => {
  if (t) fetchRecommendations()
}, { immediate: false })

// 绑定窗打开期间把 avatar 窗口撑大到 640×720，关闭恢复 400×560，
// 不直接看到 avatar 容器的小尺寸把字挤得难以辨认。
useExpandedAvatarWindow(computed(() => store.showBindTaskWindow))

onMounted(() => {
  if (currentTask.value) fetchRecommendations()
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
        <div v-if="!loading && recommendations.length > 0" class="rec-list">
          <div class="rec-header">
            <span class="rec-tip">{{ multiSelectMode ? '可多选，跨仓任务关联多个项目' : '选择该任务归属的项目' }}</span>
          </div>
          <button
            v-for="r in recommendations"
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
              <div class="rec-reason">{{ r.reason || '—' }}</div>
            </div>
            <span v-if="r.reason !== '手动选择'" class="rec-score">{{ r.score }}</span>
          </button>
          <div class="rec-actions">
            <button v-if="!multiSelectMode && recommendations.length > 1" class="action-link" @click="enterMultiMode">
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
  background: linear-gradient(135deg, rgba(20, 30, 56, 0.97), rgba(15, 23, 42, 0.97));
  border: 1px solid rgba(100, 200, 255, 0.18);
  border-radius: 14px;
  box-shadow: 0 12px 32px rgba(0, 0, 0, 0.5);
  overflow: hidden;
  z-index: 58;
  color: rgba(255, 255, 255, 0.92);
}

.panel-header {
  display: flex; align-items: center; justify-content: space-between;
  padding: 8px 10px;
  background: rgba(0, 0, 0, 0.2);
  border-bottom: 1px solid rgba(255, 255, 255, 0.06);
}
.panel-title { display: flex; align-items: center; gap: 6px; font-size: 15px; font-weight: 600; }
.title-icon { font-size: 16px; }
.queue-badge {
  margin-left: 6px;
  padding: 2px 7px;
  font-size: 11.5px;
  font-weight: 500;
  color: rgba(167, 139, 250, 0.95);
  background: rgba(167, 139, 250, 0.12);
  border-radius: 8px;
}

.icon-btn {
  width: 22px; height: 22px;
  display: inline-flex; align-items: center; justify-content: center;
  font-size: 16px; line-height: 1;
  color: rgba(255, 255, 255, 0.55);
  background: transparent; border: none; border-radius: 6px;
  cursor: pointer;
}
.icon-btn:hover:not(:disabled) { color: rgba(255, 255, 255, 0.95); background: rgba(255, 255, 255, 0.08); }
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
  background: rgba(0, 0, 0, 0.22);
  border: 1px solid rgba(255, 255, 255, 0.06);
  border-radius: 10px;
}
.task-title {
  font-size: 15px;
  font-weight: 600;
  line-height: 1.45;
  color: rgba(255, 255, 255, 0.95);
}
.task-meta {
  margin-top: 8px;
  display: flex; flex-wrap: wrap; gap: 5px;
}
.meta-pill {
  padding: 2px 8px;
  font-size: 12px;
  border-radius: 6px;
  background: rgba(255, 255, 255, 0.06);
  color: rgba(255, 255, 255, 0.75);
}
.meta-pill.priority-urgent { color: rgba(248, 113, 113, 0.95); background: rgba(239, 68, 68, 0.12); }
.meta-pill.priority-high { color: rgba(251, 191, 36, 0.95); background: rgba(245, 158, 11, 0.12); }
.meta-pill.priority-low { color: rgba(148, 163, 184, 0.85); background: rgba(148, 163, 184, 0.08); }
.meta-pill.meta-deadline { color: rgba(0, 212, 255, 0.85); background: rgba(0, 212, 255, 0.1); }
.meta-pill.meta-id { font-family: ui-monospace, SFMono-Regular, monospace; color: rgba(255, 255, 255, 0.45); }

.phase-text {
  display: flex; align-items: center; gap: 6px; justify-content: center;
  padding: 10px 12px;
  font-size: 13.5px;
  text-align: center;
  border-radius: 8px;
  background: rgba(0, 0, 0, 0.18);
  color: rgba(255, 255, 255, 0.82);
}
.phase-text.phase-error { color: rgba(248, 113, 113, 0.95); background: rgba(239, 68, 68, 0.1); }

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
  color: rgba(255, 255, 255, 0.55);
  padding: 0 2px;
}

.rec-row {
  display: flex; align-items: center; gap: 10px;
  padding: 10px 12px;
  text-align: left;
  background: rgba(0, 0, 0, 0.18);
  border: 1px solid rgba(255, 255, 255, 0.06);
  border-radius: 8px;
  cursor: pointer;
  transition: all 0.12s;
  color: inherit;
}
.rec-row:hover { background: rgba(0, 0, 0, 0.28); border-color: rgba(100, 200, 255, 0.18); }
.rec-row.selected { background: rgba(0, 212, 255, 0.08); border-color: rgba(0, 212, 255, 0.4); }
.rec-row.top:not(.selected) { border-color: rgba(167, 139, 250, 0.25); }

.rec-check {
  width: 16px;
  font-size: 16px;
  color: rgba(255, 255, 255, 0.55);
  text-align: center;
}
.rec-row.selected .rec-check { color: rgba(0, 212, 255, 0.95); }

.rec-main { flex: 1; min-width: 0; }
.rec-line1 { display: flex; align-items: center; gap: 6px; }
.rec-path {
  font-size: 14px;
  font-weight: 500;
  font-family: ui-monospace, SFMono-Regular, monospace;
  color: rgba(255, 255, 255, 0.95);
  overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
}
.rec-badge {
  padding: 2px 6px;
  font-size: 11px;
  font-weight: 600;
  color: rgba(167, 139, 250, 0.95);
  background: rgba(167, 139, 250, 0.14);
  border-radius: 4px;
  letter-spacing: 0.4px;
  flex-shrink: 0;
}
.rec-badge-manual {
  color: rgba(251, 191, 36, 0.95);
  background: rgba(245, 158, 11, 0.14);
}
.rec-reason {
  margin-top: 3px;
  font-size: 12.5px;
  color: rgba(255, 255, 255, 0.6);
  overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
}

.rec-score {
  width: 38px;
  font-size: 14px;
  font-weight: 600;
  font-family: ui-monospace, SFMono-Regular, monospace;
  text-align: right;
  color: rgba(255, 255, 255, 0.7);
}
.rec-row.selected .rec-score { color: rgba(0, 212, 255, 0.95); }

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
  border-radius: 6px;
  cursor: pointer;
  text-align: center;
  font-family: inherit;
}
.action-link:not(.browse-link) {
  color: rgba(167, 139, 250, 0.85);
  border-color: rgba(167, 139, 250, 0.3);
}
.action-link:not(.browse-link):hover {
  background: rgba(167, 139, 250, 0.08);
  color: rgba(167, 139, 250, 1);
}
.browse-link {
  color: rgba(251, 191, 36, 0.85);
  border-color: rgba(251, 191, 36, 0.3);
}
.browse-link:hover {
  background: rgba(251, 191, 36, 0.08);
  color: rgba(251, 191, 36, 1);
}

.panel-footer {
  display: flex;
  gap: 6px;
  padding: 8px 10px;
  background: rgba(0, 0, 0, 0.2);
  border-top: 1px solid rgba(255, 255, 255, 0.06);
}
.primary-btn {
  flex: 1;
  padding: 10px 14px;
  font-size: 13.5px;
  font-weight: 500;
  color: white;
  background: linear-gradient(135deg, rgba(0, 212, 255, 0.9), rgba(59, 130, 246, 0.9));
  border: none;
  border-radius: 6px;
  cursor: pointer;
  transition: all 0.15s;
}
.primary-btn:hover:not(:disabled) {
  box-shadow: 0 4px 12px rgba(0, 212, 255, 0.3);
  transform: translateY(-1px);
}
.primary-btn:disabled {
  background: rgba(255, 255, 255, 0.08);
  color: rgba(255, 255, 255, 0.4);
  cursor: not-allowed;
}
.secondary-btn {
  padding: 10px 18px;
  font-size: 13.5px;
  color: rgba(255, 255, 255, 0.7);
  background: rgba(255, 255, 255, 0.05);
  border: 1px solid rgba(255, 255, 255, 0.08);
  border-radius: 6px;
  cursor: pointer;
}
.secondary-btn:hover:not(:disabled) {
  color: rgba(255, 255, 255, 0.95);
  background: rgba(255, 255, 255, 0.1);
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
