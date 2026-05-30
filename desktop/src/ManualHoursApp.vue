<script setup lang="ts">
// 手动写工时窗口：单页布局。顶部 Tab 切类别（运维/事务/新增功能/其他），
// 下方任务搜索下拉（模糊检索）+ 工时 + 内容。运维 Tab 多一层项目筛选。

import { onMounted, onUnmounted, ref, computed, nextTick } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { emit } from '@tauri-apps/api/event'
import ErrorBoundary from './components/ErrorBoundary.vue'

interface ZenTaoTaskBrief {
  id: string
  name: string
  status: string
  pri: number
  deadline: string
}

interface ClassifiedTasks {
  ops: ZenTaoTaskBrief[]
  daily: ZenTaoTaskBrief[]
  feature: ZenTaoTaskBrief[]
  other: ZenTaoTaskBrief[]
}

type Category = 'ops' | 'daily' | 'feature' | 'other'

const CATEGORY_META: Record<Category, { label: string; icon: string }> = {
  ops: { label: '运维', icon: '🔧' },
  daily: { label: '事务', icon: '📋' },
  feature: { label: '新增功能', icon: '✨' },
  other: { label: '其他', icon: '📦' },
}
const CATEGORY_ORDER: Category[] = ['ops', 'daily', 'feature', 'other']

const PROJECT_ALL = '__all__'
const PROJECT_UNKNOWN = '（未归项目）'

// 任务名前缀作为项目名：取第一个「（」或「(」之前的部分。
// 如「26年计量管理系统（数据核对）常规运维」→「26年计量管理系统」。
function extractProject(name: string): string {
  let idx = name.length
  const a = name.indexOf('（')
  const b = name.indexOf('(')
  if (a >= 0) idx = Math.min(idx, a)
  if (b >= 0) idx = Math.min(idx, b)
  const head = name.slice(0, idx).trim()
  return head || PROJECT_UNKNOWN
}

// 状态
const classifiedTasks = ref<ClassifiedTasks | null>(null)
const loading = ref(true)
const loadError = ref('')

const activeCategory = ref<Category>('ops')
const selectedTask = ref<ZenTaoTaskBrief | null>(null)
const taskQuery = ref('')
const taskComboOpen = ref(false)
const projectFilter = ref<string>(PROJECT_ALL)
const hours = ref('')
const content = ref('')
const submitting = ref(false)
const error = ref('')
const result = ref<'idle' | 'ok' | 'fail'>('idle')
const searchEl = ref<HTMLInputElement | null>(null)

const currentTasks = computed(() => {
  if (!classifiedTasks.value) return []
  return classifiedTasks.value[activeCategory.value]
})

const opsProjects = computed(() => {
  const set = new Set<string>()
  for (const t of classifiedTasks.value?.ops ?? []) set.add(extractProject(t.name))
  return Array.from(set).sort((a, b) => a.localeCompare(b, 'zh-CN'))
})

const visibleTasks = computed(() => {
  let list = currentTasks.value
  if (activeCategory.value === 'ops' && projectFilter.value !== PROJECT_ALL) {
    list = list.filter(t => extractProject(t.name) === projectFilter.value)
  }
  const q = taskQuery.value.trim().toLowerCase()
  if (q && !selectedTask.value) {
    list = list.filter(t => t.id.toLowerCase().includes(q) || t.name.toLowerCase().includes(q))
  }
  return list
})

async function loadTasks() {
  loading.value = true
  loadError.value = ''
  try {
    const r = await invoke<{ success: boolean; data?: ClassifiedTasks; error?: string }>('tool_execute', {
      name: 'get_classified_tasks',
      input: {},
    })
    if (!r.success || !r.data) {
      loadError.value = r.error || '拉取任务失败'
      return
    }
    classifiedTasks.value = r.data
  } catch (e) {
    loadError.value = String(e)
  } finally {
    loading.value = false
  }
}

function setCategory(cat: Category) {
  if (activeCategory.value === cat) return
  activeCategory.value = cat
  selectedTask.value = null
  taskQuery.value = ''
  projectFilter.value = PROJECT_ALL
  taskComboOpen.value = false
  error.value = ''
}

function pickTask(t: ZenTaoTaskBrief) {
  selectedTask.value = t
  taskQuery.value = ''
  taskComboOpen.value = false
  error.value = ''
}

function clearTask() {
  selectedTask.value = null
  taskQuery.value = ''
  taskComboOpen.value = false
}

function onTaskInput() {
  if (selectedTask.value) selectedTask.value = null
  taskComboOpen.value = true
}

function onComboBlur() {
  // 延迟关闭：让下拉项的 mousedown 先触发
  setTimeout(() => { taskComboOpen.value = false }, 150)
}

async function submit() {
  if (submitting.value || !selectedTask.value) return
  const hoursNum = parseFloat(hours.value)
  if (!Number.isFinite(hoursNum) || hoursNum <= 0) {
    error.value = '工时必须是正数（如 0.5、1）'
    return
  }
  if (!content.value.trim()) {
    error.value = '工作内容不能为空'
    return
  }
  submitting.value = true
  error.value = ''
  try {
    const r = await invoke<{ success: boolean; data?: any; error?: string }>('tool_execute', {
      name: 'log-task-effort',
      input: { taskId: selectedTask.value.id, hours: hoursNum, work: content.value },
    })
    if (r.success && r.data?.ok) {
      result.value = 'ok'
      await emit('write-hours-done', { taskId: selectedTask.value.id })
      setTimeout(() => { closeWindow() }, 1200)
    } else {
      result.value = 'fail'
      error.value = r.error || '禅道返回未知错误'
    }
  } catch (e: any) {
    result.value = 'fail'
    error.value = e?.message ?? String(e)
  } finally {
    submitting.value = false
  }
}

async function closeWindow() {
  if (submitting.value) return
  try {
    await invoke('manual_hours_close')
  } catch (e) {
    console.error('manual_hours_close 失败:', e)
    try { await invoke('avatar_show_fallback') } catch {}
  }
}

function onKeydown(ev: KeyboardEvent) {
  if (ev.key === 'Escape') {
    if (taskComboOpen.value) {
      taskComboOpen.value = false
    } else {
      closeWindow()
    }
    return
  }
  // Ctrl/Cmd+Enter 提交（submit 内部已守卫未选任务）
  if (ev.key === 'Enter' && (ev.ctrlKey || ev.metaKey)) {
    ev.preventDefault()
    submit()
  }
}

onMounted(async () => {
  window.addEventListener('keydown', onKeydown)
  await loadTasks()
  await nextTick()
  // 加载完默认聚焦任务搜索框，省一次点击
  searchEl.value?.focus()
})
onUnmounted(() => {
  window.removeEventListener('keydown', onKeydown)
})
</script>

<template>
  <ErrorBoundary>
  <div class="mh-root">
    <header class="mh-header" data-tauri-drag-region>
      <h1 class="mh-title" data-tauri-drag-region>📝 手动写工时</h1>
      <button class="mh-header-close" :disabled="submitting" @click="closeWindow" title="关闭">×</button>
    </header>

    <div class="mh-body">
      <div v-if="loading" class="mh-center">
        <span class="mh-spinner">⟳</span>
        <p>正在从禅道拉取任务列表…</p>
      </div>

      <div v-else-if="loadError" class="mh-center">
        <p class="mh-error-text">{{ loadError }}</p>
        <button class="mh-retry" @click="loadTasks">重试</button>
      </div>

      <div v-else class="mh-main">
        <div class="mh-tabs">
          <button
            v-for="cat in CATEGORY_ORDER"
            :key="cat"
            class="mh-tab"
            :class="{ active: activeCategory === cat }"
            @click="setCategory(cat)"
          >
            <span class="mh-tab-icon">{{ CATEGORY_META[cat].icon }}</span>
            <span class="mh-tab-label">{{ CATEGORY_META[cat].label }}</span>
            <span class="mh-tab-count">{{ classifiedTasks?.[cat]?.length ?? 0 }}</span>
          </button>
        </div>

        <div v-if="activeCategory === 'ops' && opsProjects.length > 1" class="form-row">
          <label class="form-label">项目</label>
          <select v-model="projectFilter" class="form-input form-select">
            <option :value="PROJECT_ALL">全部项目（{{ classifiedTasks?.ops?.length ?? 0 }}）</option>
            <option v-for="p in opsProjects" :key="p" :value="p">{{ p }}</option>
          </select>
        </div>

        <div class="form-row">
          <label class="form-label">任务</label>
          <div v-if="selectedTask" class="selected-task-card">
            <span class="selected-task-id">#{{ selectedTask.id }}</span>
            <span class="selected-task-name">{{ selectedTask.name }}</span>
            <span v-if="selectedTask.pri >= 2" class="mh-pri" :class="`pri-${selectedTask.pri}`">P{{ selectedTask.pri }}</span>
            <button
              class="selected-task-clear"
              :disabled="submitting || result === 'ok'"
              title="重新选择"
              @click="clearTask"
            >×</button>
          </div>
          <div v-else class="task-combo">
            <input
              v-model="taskQuery"
              ref="searchEl"
              class="form-input"
              :placeholder="`从 ${currentTasks.length} 个任务中搜索…`"
              :disabled="submitting || result === 'ok'"
              @focus="taskComboOpen = true"
              @input="onTaskInput"
              @blur="onComboBlur"
            />
            <div v-if="taskComboOpen" class="task-combo-dropdown">
              <template v-if="visibleTasks.length > 0">
                <button
                  v-for="t in visibleTasks.slice(0, 50)"
                  :key="t.id"
                  class="task-combo-item"
                  @mousedown.prevent="pickTask(t)"
                >
                  <span class="mh-task-id">#{{ t.id }}</span>
                  <span class="mh-task-name">{{ t.name }}</span>
                  <span v-if="t.pri >= 2" class="mh-pri" :class="`pri-${t.pri}`">P{{ t.pri }}</span>
                </button>
                <div v-if="visibleTasks.length > 50" class="task-combo-more">
                  还有 {{ visibleTasks.length - 50 }} 个，继续输入以筛选…
                </div>
              </template>
              <div v-else class="task-combo-empty">该筛选下没有匹配的任务</div>
            </div>
          </div>
        </div>

        <div class="form-row form-row-inline">
          <label class="form-label">工时（小时）</label>
          <input
            v-model="hours"
            class="form-input form-input-hours"
            type="text"
            inputmode="decimal"
            :disabled="submitting || result === 'ok'"
            placeholder="如 0.5、1、1.5"
          />
        </div>

        <div class="form-row form-row-grow">
          <label class="form-label">工作内容</label>
          <textarea
            v-model="content"
            class="form-textarea"
            :disabled="submitting || result === 'ok'"
            placeholder="给禅道看的工作记录文本"
          />
        </div>

        <p v-if="error" class="form-error">{{ error }}</p>
        <p v-if="result === 'ok'" class="form-success">✓ 已写入禅道。</p>
      </div>
    </div>

    <footer class="mh-footer">
      <button class="btn btn-cancel" :disabled="submitting" @click="closeWindow">关闭</button>
      <button
        class="btn btn-confirm"
        :disabled="submitting || result === 'ok' || !selectedTask || loading"
        @click="submit"
      >
        <span v-if="submitting">提交中…</span>
        <span v-else-if="result === 'ok'">已写入</span>
        <span v-else>确认写入</span>
      </button>
    </footer>
  </div>
  </ErrorBoundary>
</template>

<style scoped>
.mh-root {
  display: flex;
  flex-direction: column;
  height: 100vh;
  background: linear-gradient(135deg, #1a2238, #0f172a);
  color: rgba(255, 255, 255, 0.92);
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", "PingFang SC", "Microsoft YaHei", sans-serif;
}

.mh-header {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 12px 16px;
  background: rgba(0, 0, 0, 0.25);
  border-bottom: 1px solid rgba(255, 255, 255, 0.06);
  user-select: none;
}
.mh-title {
  margin: 0;
  font-size: 14px;
  font-weight: 600;
  color: rgba(255, 255, 255, 0.95);
  flex: 1;
}
.mh-header-close {
  flex-shrink: 0;
  width: 26px;
  height: 26px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: transparent;
  border: none;
  border-radius: 4px;
  color: rgba(255, 255, 255, 0.6);
  font-size: 18px;
  cursor: pointer;
  padding: 0;
  line-height: 1;
}
.mh-header-close:hover:not(:disabled) {
  background: rgba(239, 68, 68, 0.25);
  color: rgba(255, 255, 255, 0.98);
}
.mh-header-close:disabled { opacity: 0.4; cursor: not-allowed; }

.mh-body {
  flex: 1;
  overflow-y: auto;
  padding: 16px 20px;
  min-height: 0;
}

.mh-center {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 12px;
  padding: 40px 0;
  color: rgba(255, 255, 255, 0.6);
  font-size: 13px;
}
.mh-spinner {
  font-size: 28px;
  animation: spin 1s linear infinite;
}
@keyframes spin { from { transform: rotate(0); } to { transform: rotate(360deg); } }
.mh-error-text { color: rgba(239, 68, 68, 0.85); }
.mh-retry {
  padding: 6px 14px;
  background: rgba(59, 130, 246, 0.25);
  color: rgba(255, 255, 255, 0.95);
  border: 1px solid rgba(59, 130, 246, 0.5);
  border-radius: 6px;
  font-size: 11px;
  cursor: pointer;
}

/* 主区域 */
.mh-main {
  display: flex;
  flex-direction: column;
  gap: 14px;
  height: 100%;
}

/* Tab */
.mh-tabs {
  display: flex;
  gap: 4px;
  padding: 4px;
  background: rgba(0, 0, 0, 0.25);
  border: 1px solid rgba(255, 255, 255, 0.06);
  border-radius: 8px;
}
.mh-tab {
  flex: 1;
  min-width: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 4px;
  padding: 8px 4px;
  background: transparent;
  border: 1px solid transparent;
  border-radius: 6px;
  color: rgba(255, 255, 255, 0.65);
  font-size: 12px;
  cursor: pointer;
  font-family: inherit;
  transition: all 0.15s;
  white-space: nowrap;
  overflow: hidden;
}
.mh-tab:hover {
  background: rgba(255, 255, 255, 0.04);
  color: rgba(255, 255, 255, 0.92);
}
.mh-tab.active {
  background: rgba(59, 130, 246, 0.18);
  border-color: rgba(59, 130, 246, 0.35);
  color: rgba(255, 255, 255, 0.98);
}
.mh-tab-icon { font-size: 13px; flex-shrink: 0; }
.mh-tab-label { font-weight: 500; white-space: nowrap; }
.mh-tab-count {
  flex-shrink: 0;
  padding: 0 5px;
  background: rgba(255, 255, 255, 0.08);
  border-radius: 8px;
  font-size: 10px;
  line-height: 14px;
  color: rgba(255, 255, 255, 0.55);
}
.mh-tab.active .mh-tab-count {
  background: rgba(59, 130, 246, 0.35);
  color: rgba(255, 255, 255, 0.95);
}

/* 表单行 */
.form-row { display: flex; flex-direction: column; gap: 5px; }
.form-row-inline { flex-direction: row; align-items: center; gap: 10px; }
.form-row-inline .form-label { flex-shrink: 0; }
.form-row-grow { flex: 1; min-height: 120px; }
.form-label {
  font-size: 12.5px;
  color: rgba(255, 255, 255, 0.7);
  font-weight: 500;
}
.form-input,
.form-textarea {
  padding: 8px 10px;
  font-size: 13px;
  color: rgba(255, 255, 255, 0.95);
  background: rgba(0, 0, 0, 0.3);
  border: 1px solid rgba(255, 255, 255, 0.1);
  border-radius: 6px;
  outline: none;
  font-family: inherit;
  transition: border-color 0.15s;
  width: 100%;
  box-sizing: border-box;
}
.form-input:focus,
.form-textarea:focus { border-color: rgba(59, 130, 246, 0.6); }
.form-input:disabled,
.form-textarea:disabled { opacity: 0.55; cursor: not-allowed; }
.form-select {
  appearance: none;
  background-image: linear-gradient(45deg, transparent 50%, rgba(255,255,255,0.6) 50%),
                    linear-gradient(135deg, rgba(255,255,255,0.6) 50%, transparent 50%);
  background-position: calc(100% - 14px) 50%, calc(100% - 9px) 50%;
  background-size: 5px 5px, 5px 5px;
  background-repeat: no-repeat;
  padding-right: 26px;
}
.form-select option { background: #1a2238; color: rgba(255,255,255,0.95); }
.form-input-hours { max-width: 140px; }
.form-textarea {
  flex: 1;
  min-height: 100px;
  resize: vertical;
  line-height: 1.55;
  font-family: ui-monospace, SFMono-Regular, monospace;
  font-size: 12.5px;
}
.form-error {
  margin: 0;
  padding: 8px 10px;
  font-size: 12px;
  color: rgba(252, 165, 165, 0.95);
  background: rgba(239, 68, 68, 0.12);
  border-left: 3px solid rgba(239, 68, 68, 0.5);
  border-radius: 4px;
}
.form-success {
  margin: 0;
  padding: 8px 10px;
  font-size: 12px;
  color: rgba(134, 239, 172, 0.95);
  background: rgba(34, 197, 94, 0.12);
  border-left: 3px solid rgba(34, 197, 94, 0.5);
  border-radius: 4px;
}

/* 任务搜索下拉 */
.task-combo {
  position: relative;
}

/* 已选任务卡片：完整展示 id + name，可换行 */
.selected-task-card {
  display: flex;
  align-items: flex-start;
  gap: 8px;
  padding: 10px 12px;
  background: rgba(59, 130, 246, 0.12);
  border: 1px solid rgba(59, 130, 246, 0.3);
  border-radius: 6px;
}
.selected-task-id {
  font-family: ui-monospace, monospace;
  font-size: 12px;
  color: rgba(147, 197, 253, 0.95);
  flex-shrink: 0;
  padding-top: 2px;
}
.selected-task-name {
  flex: 1;
  font-size: 13px;
  line-height: 1.5;
  color: rgba(255, 255, 255, 0.95);
  word-break: break-word;
  white-space: normal;
}
.selected-task-clear {
  flex-shrink: 0;
  width: 22px;
  height: 22px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: rgba(255, 255, 255, 0.08);
  border: none;
  border-radius: 50%;
  color: rgba(255, 255, 255, 0.7);
  font-size: 14px;
  cursor: pointer;
  padding: 0;
  line-height: 1;
}
.selected-task-clear:hover:not(:disabled) {
  background: rgba(255, 255, 255, 0.18);
  color: white;
}
.selected-task-clear:disabled { opacity: 0.4; cursor: not-allowed; }

.task-combo-dropdown {
  position: absolute;
  top: calc(100% + 4px);
  left: 0;
  right: 0;
  max-height: 280px;
  overflow-y: auto;
  background: #1c2540;
  border: 1px solid rgba(255, 255, 255, 0.12);
  border-radius: 6px;
  box-shadow: 0 8px 24px rgba(0, 0, 0, 0.45);
  z-index: 10;
  padding: 4px;
}
.task-combo-item {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  padding: 8px 10px;
  background: transparent;
  border: none;
  border-radius: 4px;
  color: rgba(255, 255, 255, 0.92);
  font-size: 12.5px;
  cursor: pointer;
  text-align: left;
  font-family: inherit;
}
.task-combo-item:hover { background: rgba(59, 130, 246, 0.18); }
.task-combo-more,
.task-combo-empty {
  padding: 8px 10px;
  font-size: 11.5px;
  color: rgba(255, 255, 255, 0.45);
  text-align: center;
}

.mh-task-id {
  font-family: ui-monospace, monospace;
  font-size: 11px;
  color: rgba(147, 197, 253, 0.8);
  flex-shrink: 0;
}
.mh-task-name {
  flex: 1;
  font-size: 12.5px;
  line-height: 1.4;
  word-break: break-word;
}
.mh-pri {
  padding: 1px 5px;
  border-radius: 3px;
  font-size: 9px;
  font-weight: 700;
  flex-shrink: 0;
}
.pri-2 { background: rgba(250, 204, 21, 0.2); color: rgba(253, 224, 71, 0.95); }
.pri-3 { background: rgba(239, 68, 68, 0.2); color: rgba(252, 165, 165, 0.95); }
.pri-4 { background: rgba(168, 85, 247, 0.25); color: rgba(216, 180, 254, 0.95); }

/* 底部 */
.mh-footer {
  flex-shrink: 0;
  display: flex;
  justify-content: flex-end;
  gap: 10px;
  padding: 12px 20px;
  background: rgba(0, 0, 0, 0.2);
  border-top: 1px solid rgba(255, 255, 255, 0.06);
}
.btn {
  padding: 8px 18px;
  font-size: 13px;
  border-radius: 6px;
  cursor: pointer;
  border: 1px solid transparent;
  transition: background 0.15s, color 0.15s, border-color 0.15s;
  font-family: inherit;
}
.btn:disabled { cursor: not-allowed; opacity: 0.5; }
.btn-cancel {
  background: transparent;
  color: rgba(255, 255, 255, 0.7);
  border-color: rgba(255, 255, 255, 0.18);
}
.btn-cancel:hover:not(:disabled) {
  background: rgba(255, 255, 255, 0.06);
  color: rgba(255, 255, 255, 0.95);
}
.btn-confirm {
  background: linear-gradient(135deg, rgba(167, 139, 250, 0.95), rgba(139, 92, 246, 0.95));
  color: white;
  border-color: transparent;
  font-weight: 500;
}
.btn-confirm:hover:not(:disabled) {
  box-shadow: 0 4px 12px rgba(167, 139, 250, 0.35);
}
</style>
