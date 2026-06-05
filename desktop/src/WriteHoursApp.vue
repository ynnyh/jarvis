<script setup lang="ts">
// 写工时独立窗口：avatar/复盘窗触发 invoke('write_hours_open', payload) 时，
// Rust 把 payload 存进 state、show 这个窗、然后立刻 eval("location.reload()")
// 强制 webview 重载 → Vue 实例销毁重建 → 本组件 onMounted 必跑 loadPayload，
// 从 state 拿到本次的最新 payload。这样彻底绕开"hide/show 不触发 onMounted"
// 和"Tauri 事件在预注册窗口上派发不稳"两个坑。
// 写入成功后 emit "write-hours-done" 让复盘窗把任务标灰，然后 write_hours_close
// 隐藏自己并 show avatar。

import { onMounted, onUnmounted, ref, computed, nextTick } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { emit } from '@tauri-apps/api/event'
import ErrorBoundary from './components/ErrorBoundary.vue'
import MatrixRain from './components/MatrixRain.vue'
import CyberParticles from './components/CyberParticles.vue'
import { useConfigStore } from './stores/config'
import { useTheme } from './composables/useTheme'
const configStore = useConfigStore()
useTheme()

interface TaskInfo {
  id: string
  name: string
}

interface WriteHoursPayload {
  taskId: string
  taskName: string
  suggestedHours?: number
  content: string
  kind: 'task' | 'orphan'
  tasks?: TaskInfo[]
}

const payload = ref<WriteHoursPayload | null>(null)
const taskSearch = ref('')
const taskIdInput = ref('')
const selectedTaskName = ref('')
const hours = ref('')
const content = ref('')
const submitting = ref(false)
const error = ref('')
const result = ref<'idle' | 'ok' | 'fail'>('idle')
const hoursEl = ref<HTMLInputElement | null>(null)
const searchWrapper = ref<HTMLElement | null>(null)
const showDropdown = ref(false)

/** 当前写的任务 ID（去掉 # 前缀，纯数字校验） */
const currentTaskId = computed(() => taskIdInput.value.trim().replace(/^#/, ''))

/** 已选中某个任务 */
const hasSelectedTask = computed(() => currentTaskId.value.length > 0)

/** 按搜索关键词过滤任务列表 */
const filteredTasks = computed(() => {
  const tasks = payload.value?.tasks
  if (!tasks || tasks.length === 0) return []
  const q = taskSearch.value.trim().toLowerCase()
  if (!q) return []
  return tasks
    .filter(t => {
      if (t.id.startsWith(q)) return true
      if (t.name.toLowerCase().includes(q)) return true
      return false
    })
    .slice(0, 30) // 最多展示 30 条
})

function selectTask(t: TaskInfo) {
  taskIdInput.value = t.id
  taskSearch.value = `#${t.id} ${t.name}`
  selectedTaskName.value = t.name
  showDropdown.value = false
  error.value = ''
  // 聚焦到工时输入
  nextTick(() => hoursEl.value?.focus())
}

function clearTask() {
  taskIdInput.value = ''
  taskSearch.value = ''
  selectedTaskName.value = ''
  showDropdown.value = false
}

function onSearchInput() {
  // 用户手动改搜索框时清除已选 taskId
  if (taskSearch.value.trim() === '') {
    taskIdInput.value = ''
    selectedTaskName.value = ''
  }
  showDropdown.value = true
}

function onSearchBlur() {
  // 延迟隐藏让 click 事件先触发
  setTimeout(() => { showDropdown.value = false }, 180)
}

function onSearchFocus() {
  if (filteredTasks.value.length > 0 || taskSearch.value.trim()) {
    showDropdown.value = true
  }
}

async function closeWindow() {
  if (submitting.value) return
  try {
    await invoke('write_hours_close')
  } catch (e) {
    console.error('write_hours_close 失败:', e)
    try { await invoke('avatar_show_fallback') } catch {}
  }
}

async function submit() {
  if (submitting.value) return
  const tid = currentTaskId.value
  if (!/^\d+$/.test(tid)) {
    error.value = '任务 ID 必须是纯数字（如 10238）'
    return
  }
  const hoursNum = parseFloat(hours.value)
  if (!Number.isFinite(hoursNum) || hoursNum <= 0) {
    error.value = '工时必须是正数（小数也行，比如 0.5）'
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
      input: { taskId: tid, hours: hoursNum, work: content.value },
    })
    if (r.success && r.data?.ok) {
      result.value = 'ok'
      await emit('write-hours-done', { taskId: tid })
      setTimeout(() => { closeWindow() }, 1000)
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

function applyPayload(p: WriteHoursPayload) {
  payload.value = p
  taskIdInput.value = p.taskId || ''
  selectedTaskName.value = p.taskName || ''
  taskSearch.value = p.taskId
    ? `#${p.taskId} ${p.taskName}`
    : p.taskName || ''
  hours.value = p.suggestedHours ? String(p.suggestedHours) : ''
  content.value = p.content || ''
  error.value = ''
  result.value = 'idle'
  submitting.value = false
}

async function loadPayload() {
  try {
    const p = await invoke<WriteHoursPayload | null>('write_hours_take_payload')
    if (!p) return
    applyPayload(p)
  } catch (e: any) {
    error.value = `加载数据失败：${e?.message ?? String(e)}`
  }
}

function onKeydown(ev: KeyboardEvent) {
  if (ev.key === 'Escape') { closeWindow(); return }
  if (ev.key === 'Enter' && (ev.ctrlKey || ev.metaKey)) {
    ev.preventDefault()
    submit()
  }
}

onMounted(async () => {
  await configStore.load()
  window.addEventListener('keydown', onKeydown)
  await loadPayload()
  await nextTick()
  // 聚焦到搜索框（无预填任务时）或工时输入
  if (!taskIdInput.value) {
    // 有预填任务名时自动搜索
    if (selectedTaskName.value && payload.value?.tasks?.length) {
      showDropdown.value = true
    }
  } else {
    hoursEl.value?.focus()
  }
})

onUnmounted(() => {
  window.removeEventListener('keydown', onKeydown)
})
</script>

<template>
  <ErrorBoundary>
  <div class="wh-root theme-bg">
    <MatrixRain />
    <CyberParticles />
    <header class="wh-header" data-tauri-drag-region>
      <h1 class="wh-title" data-tauri-drag-region>
        {{ payload?.kind === 'orphan' ? '✍️ 写到任务' : '✍️ 写入工时到禅道' }}
      </h1>
      <button class="wh-header-close" :disabled="submitting" @click="closeWindow" title="关闭">×</button>
    </header>

    <div class="wh-body">
      <div class="form-row">
        <label class="form-label">禅道任务</label>
        <div class="task-search-wrapper" ref="searchWrapper">
          <input
            v-model="taskSearch"
            class="form-input"
            type="text"
            :disabled="submitting || result === 'ok'"
            placeholder="输入任务名称或 ID 搜索…"
            @input="onSearchInput"
            @focus="onSearchFocus"
            @blur="onSearchBlur"
          />
          <ul v-if="showDropdown && filteredTasks.length > 0" class="task-search-dropdown">
            <li
              v-for="t in filteredTasks"
              :key="t.id"
              class="task-search-option"
              @mousedown.prevent="selectTask(t)"
            >
              <span class="tso-id">#{{ t.id }}</span>
              <span class="tso-name">{{ t.name }}</span>
            </li>
          </ul>
          <p v-if="!hasSelectedTask && !showDropdown && taskSearch && filteredTasks.length === 0 && payload?.tasks?.length" class="form-hint">
            没有匹配的任务，可以继续输入任务 ID
          </p>
        </div>
        <p v-if="hasSelectedTask" class="form-hint">
          当前选中：<strong>#{{ currentTaskId }} {{ selectedTaskName }}</strong>
          <button class="clear-task-btn" @click="clearTask" :disabled="submitting || result === 'ok'">换一个</button>
        </p>
        <p v-else-if="payload?.kind === 'task' && payload.taskId" class="form-hint">
          来自任务「{{ payload.taskName }}」，可搜索切换或直接填任务 ID
        </p>
        <p v-else class="form-hint">
          这批 commit 没自动关联到任务。输入关键词搜索后点选，或直接填任务 ID
        </p>
      </div>

      <div class="form-row">
        <label class="form-label">工时（小时）</label>
        <input
          v-model="hours"
          ref="hoursEl"
          class="form-input"
          type="text"
          inputmode="decimal"
          :disabled="submitting || result === 'ok'"
          placeholder="如 0.5、1、1.5"
        />
        <p class="form-hint">AI 按 commit 量估算 {{ payload?.suggestedHours || '—' }}h，按实际填写即可</p>
      </div>

      <div class="form-row form-row-grow">
        <label class="form-label">工作内容（同来源多 commit 已合并去重）</label>
        <textarea
          v-model="content"
          class="form-textarea"
          :disabled="submitting || result === 'ok'"
          placeholder="给禅道看的工作记录文本"
        />
      </div>

      <p v-if="error" class="form-error">{{ error }}</p>
      <p v-if="result === 'ok'" class="form-success">
        ✓ 已写入禅道。需要修改去禅道工作日志里编辑或删除即可。
      </p>
    </div>

    <footer class="wh-footer">
      <button
        class="btn btn-cancel"
        :disabled="submitting"
        @click="closeWindow"
      >
        取消
      </button>
      <button
        class="btn btn-confirm"
        :disabled="submitting || result === 'ok'"
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
.wh-root {
  display: flex;
  flex-direction: column;
  height: 100vh;
  background: var(--bg);
  color: var(--text);
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", "PingFang SC", "Microsoft YaHei", sans-serif;
}

.wh-header {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 12px 16px;
  background: var(--panel-bg);
  border-bottom: 1px solid var(--divider);
  user-select: none;
}
.wh-title {
  margin: 0;
  font-size: 14px;
  font-weight: 600;
  color: var(--text);
  flex: 1;
}
.wh-header-close {
  flex-shrink: 0;
  width: 26px;
  height: 26px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: transparent;
  border: none;
  border-radius: 4px;
  color: var(--text-ghost);
  font-size: 18px;
  cursor: pointer;
  padding: 0;
  line-height: 1;
}
.wh-header-close:hover:not(:disabled) {
  background: var(--red-bg-strong);
  color: var(--red-text);
}
.wh-header-close:disabled { opacity: 0.4; cursor: not-allowed; }

.wh-body {
  flex: 1;
  overflow-y: auto;
  padding: 16px 20px;
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.form-row { display: flex; flex-direction: column; gap: 5px; }
.form-row-grow { flex: 1; min-height: 0; }
.form-label {
  font-size: 12.5px;
  color: var(--text-dim);
  font-weight: 500;
}
.form-input,
.form-textarea {
  padding: 8px 10px;
  font-size: 13px;
  color: var(--text);
  background: var(--input-bg);
  border: 1px solid var(--input-border);
  border-radius: 6px;
  outline: none;
  font-family: inherit;
  transition: border-color 0.15s;
}
.form-input:focus,
.form-textarea:focus { border-color: color-mix(in srgb, var(--accent) 60%, transparent); }
.form-input:disabled,
.form-textarea:disabled { opacity: 0.55; cursor: not-allowed; }
.form-textarea {
  flex: 1;
  min-height: 140px;
  resize: vertical;
  line-height: 1.55;
  font-family: ui-monospace, SFMono-Regular, monospace;
  font-size: 12.5px;
}
.form-hint {
  margin: 0;
  font-size: 11.5px;
  color: var(--text-muted);
  line-height: 1.5;
}
.form-hint strong { color: color-mix(in srgb, var(--accent-text) 80%, transparent); }

.clear-task-btn {
  background: transparent;
  border: none;
  color: var(--accent-text);
  font-size: 11px;
  cursor: pointer;
  padding: 0 4px;
  margin-left: 6px;
  text-decoration: underline;
  text-underline-offset: 2px;
}
.clear-task-btn:hover:not(:disabled) { color: var(--accent-text); }
.clear-task-btn:disabled { opacity: 0.4; cursor: not-allowed; }

.form-error {
  margin: 0;
  padding: 8px 10px;
  font-size: 12px;
  color: var(--red-text);
  background: var(--red-bg);
  border-left: 3px solid var(--red-border);
  border-radius: 4px;
  word-break: break-word;
}
.form-success {
  margin: 0;
  padding: 8px 10px;
  font-size: 12px;
  color: var(--green-text);
  background: var(--green-bg);
  border-left: 3px solid var(--green-border);
  border-radius: 4px;
}

/* ===== 任务搜索下拉 ===== */
.task-search-wrapper { position: relative; }
.task-search-dropdown {
  position: absolute;
  top: 100%;
  left: 0;
  right: 0;
  z-index: 100;
  list-style: none;
  margin: 2px 0 0;
  padding: 4px 0;
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: 6px;
  max-height: 220px;
  overflow-y: auto;
  box-shadow: 0 8px 24px var(--overlay);
}
.task-search-dropdown::-webkit-scrollbar { width: 4px; }
.task-search-dropdown::-webkit-scrollbar-track { background: transparent; }
.task-search-dropdown::-webkit-scrollbar-thumb { background: var(--text-ghost); border-radius: 2px; }
.task-search-option {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 6px 10px;
  cursor: pointer;
  font-size: 12px;
  color: var(--text);
  transition: background 0.1s;
}
.task-search-option:hover {
  background: color-mix(in srgb, var(--accent) 20%, transparent);
}
.tso-id {
  font-family: ui-monospace, monospace;
  font-size: 11px;
  color: var(--accent-text);
  flex-shrink: 0;
}
.tso-name {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.wh-footer {
  flex-shrink: 0;
  display: flex;
  justify-content: flex-end;
  gap: 10px;
  padding: 12px 20px;
  background: var(--panel-bg);
  border-top: 1px solid var(--divider);
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
  color: var(--text-dim);
  border-color: var(--border);
}
.btn-cancel:hover:not(:disabled) {
  background: var(--surface-item-hover);
  color: var(--text);
}
.btn-confirm {
  background: linear-gradient(135deg, color-mix(in srgb, var(--accent) 90%, transparent), color-mix(in srgb, var(--accent) 70%, transparent));
  color: white;
  border-color: transparent;
  font-weight: 500;
}
.btn-confirm:hover:not(:disabled) {
  box-shadow: 0 4px 12px color-mix(in srgb, var(--accent) 35%, transparent);
}
</style>
