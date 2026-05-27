<script setup lang="ts">
// 手动写工时窗口：从禅道拉取任务列表，按分类选择，填写工时后写入。
// 流程：选分类 → 选任务 → 填内容和工时 → 确认写入。

import { onMounted, onUnmounted, ref, computed } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { emit } from '@tauri-apps/api/event'

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
}

type Category = 'ops' | 'daily' | 'feature'

const CATEGORY_META: Record<Category, { label: string; icon: string }> = {
  ops: { label: '运维', icon: '🔧' },
  daily: { label: '日常事务', icon: '📋' },
  feature: { label: '新增功能', icon: '✨' },
}

// 状态
const classifiedTasks = ref<ClassifiedTasks | null>(null)
const loading = ref(true)
const loadError = ref('')

const selectedCategory = ref<Category | null>(null)
const selectedTask = ref<ZenTaoTaskBrief | null>(null)
const hours = ref('')
const content = ref('')
const submitting = ref(false)
const error = ref('')
const result = ref<'idle' | 'ok' | 'fail'>('idle')

const currentTasks = computed(() => {
  if (!classifiedTasks.value || !selectedCategory.value) return []
  return classifiedTasks.value[selectedCategory.value]
})

// 加载任务
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

function selectCategory(cat: Category) {
  selectedCategory.value = cat
  selectedTask.value = null
}

function selectTask(t: ZenTaoTaskBrief) {
  selectedTask.value = t
  error.value = ''
}

function goBack() {
  if (selectedTask.value) {
    selectedTask.value = null
  } else if (selectedCategory.value) {
    selectedCategory.value = null
  }
}

// 提交
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
    if (selectedTask.value || selectedCategory.value) {
      goBack()
    } else {
      closeWindow()
    }
  }
}

onMounted(() => {
  loadTasks()
  window.addEventListener('keydown', onKeydown)
})
onUnmounted(() => {
  window.removeEventListener('keydown', onKeydown)
})
</script>

<template>
  <div class="mh-root">
    <header class="mh-header">
      <button v-if="selectedCategory" class="mh-back" @click="goBack" title="返回">←</button>
      <h1 class="mh-title">
        {{ selectedTask ? `✍️ 写工时 · #${selectedTask.id}` : selectedCategory ? `${CATEGORY_META[selectedCategory].icon} ${CATEGORY_META[selectedCategory].label}任务` : '📝 手动写工时' }}
      </h1>
    </header>

    <div class="mh-body">
      <!-- 加载中 -->
      <div v-if="loading" class="mh-center">
        <span class="mh-spinner">⟳</span>
        <p>正在从禅道拉取任务列表…</p>
      </div>

      <!-- 加载失败 -->
      <div v-else-if="loadError" class="mh-center">
        <p class="mh-error-text">{{ loadError }}</p>
        <button class="mh-retry" @click="loadTasks">重试</button>
      </div>

      <!-- 第一步：选分类 -->
      <div v-else-if="!selectedCategory" class="mh-categories">
        <p class="mh-hint">选择要填写工时的任务类型</p>
        <button
          v-for="cat in (['ops', 'daily', 'feature'] as const)"
          :key="cat"
          class="mh-cat-btn"
          @click="selectCategory(cat)"
        >
          <span class="mh-cat-icon">{{ CATEGORY_META[cat].icon }}</span>
          <span class="mh-cat-label">{{ CATEGORY_META[cat].label }}</span>
          <span class="mh-cat-count">{{ classifiedTasks?.[cat]?.length ?? 0 }} 个任务</span>
          <span class="mh-cat-arrow">›</span>
        </button>
      </div>

      <!-- 第二步：选任务 -->
      <div v-else-if="!selectedTask" class="mh-task-list">
        <p class="mh-hint">点击选择要写工时的任务</p>
        <div v-if="currentTasks.length === 0" class="mh-center-sm">该分类下没有待处理的任务。</div>
        <button
          v-for="t in currentTasks"
          :key="t.id"
          class="mh-task-btn"
          @click="selectTask(t)"
        >
          <span class="mh-task-id">#{{ t.id }}</span>
          <span class="mh-task-name">{{ t.name }}</span>
          <span v-if="t.pri >= 2" class="mh-pri" :class="`pri-${t.pri}`">P{{ t.pri }}</span>
          <span v-if="t.deadline" class="mh-deadline">{{ t.deadline }}</span>
        </button>
      </div>

      <!-- 第三步：填工时 -->
      <div v-else class="mh-form">
        <div class="mh-selected-task">
          <span class="mh-task-id">#{{ selectedTask.id }}</span>
          <span class="mh-task-name">{{ selectedTask.name }}</span>
        </div>

        <div class="form-row">
          <label class="form-label">工时（小时）</label>
          <input
            v-model="hours"
            class="form-input"
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
        <p v-if="result === 'ok'" class="form-success">
          ✓ 已写入禅道。
        </p>
      </div>
    </div>

    <footer v-if="selectedTask" class="mh-footer">
      <button class="btn btn-cancel" :disabled="submitting" @click="goBack">返回</button>
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
    <footer v-else class="mh-footer">
      <button class="btn btn-cancel" @click="closeWindow">关闭</button>
    </footer>
  </div>
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
  gap: 8px;
  padding: 14px 20px;
  background: rgba(0, 0, 0, 0.25);
  border-bottom: 1px solid rgba(255, 255, 255, 0.06);
}
.mh-back {
  background: transparent;
  border: none;
  color: rgba(255, 255, 255, 0.6);
  font-size: 18px;
  cursor: pointer;
  padding: 2px 6px;
  border-radius: 4px;
}
.mh-back:hover { background: rgba(255, 255, 255, 0.08); color: rgba(255, 255, 255, 0.95); }
.mh-title {
  margin: 0;
  font-size: 15px;
  font-weight: 600;
  color: rgba(255, 255, 255, 0.95);
}

.mh-body {
  flex: 1;
  overflow-y: auto;
  padding: 16px 20px;
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
.mh-center-sm {
  text-align: center;
  padding: 20px 0;
  color: rgba(255, 255, 255, 0.45);
  font-size: 12px;
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

.mh-hint {
  margin: 0 0 12px;
  font-size: 12px;
  color: rgba(255, 255, 255, 0.45);
}

/* 分类按钮 */
.mh-categories {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.mh-cat-btn {
  display: flex;
  align-items: center;
  gap: 10px;
  width: 100%;
  padding: 14px 16px;
  background: rgba(255, 255, 255, 0.03);
  border: 1px solid rgba(255, 255, 255, 0.08);
  border-radius: 10px;
  color: rgba(255, 255, 255, 0.92);
  font-size: 14px;
  cursor: pointer;
  text-align: left;
  transition: all 0.15s;
}
.mh-cat-btn:hover {
  background: rgba(255, 255, 255, 0.06);
  border-color: rgba(255, 255, 255, 0.15);
}
.mh-cat-icon { font-size: 22px; }
.mh-cat-label { flex: 1; font-weight: 500; }
.mh-cat-count {
  font-size: 11px;
  color: rgba(255, 255, 255, 0.4);
}
.mh-cat-arrow {
  font-size: 20px;
  color: rgba(255, 255, 255, 0.3);
  font-weight: 300;
}

/* 任务列表 */
.mh-task-list {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.mh-task-btn {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  padding: 10px 12px;
  background: rgba(255, 255, 255, 0.03);
  border: 1px solid rgba(255, 255, 255, 0.06);
  border-radius: 6px;
  color: rgba(255, 255, 255, 0.92);
  font-size: 13px;
  cursor: pointer;
  text-align: left;
  transition: all 0.15s;
}
.mh-task-btn:hover {
  background: rgba(59, 130, 246, 0.1);
  border-color: rgba(59, 130, 246, 0.3);
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
.mh-deadline {
  font-size: 10px;
  color: rgba(255, 255, 255, 0.35);
  flex-shrink: 0;
}

/* 表单 */
.mh-form {
  display: flex;
  flex-direction: column;
  gap: 14px;
}
.mh-selected-task {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 12px;
  background: rgba(59, 130, 246, 0.08);
  border: 1px solid rgba(59, 130, 246, 0.2);
  border-radius: 6px;
}
.form-row { display: flex; flex-direction: column; gap: 5px; }
.form-row-grow { flex: 1; min-height: 0; }
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
}
.form-input:focus,
.form-textarea:focus { border-color: rgba(59, 130, 246, 0.6); }
.form-input:disabled,
.form-textarea:disabled { opacity: 0.55; cursor: not-allowed; }
.form-textarea {
  flex: 1;
  min-height: 120px;
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
