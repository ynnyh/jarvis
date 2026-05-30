<script setup lang="ts">
// 写工时独立窗口：avatar/复盘窗触发 invoke('write_hours_open', payload) 时，
// Rust 把 payload 存进 state、show 这个窗、然后立刻 eval("location.reload()")
// 强制 webview 重载 → Vue 实例销毁重建 → 本组件 onMounted 必跑 loadPayload，
// 从 state 拿到本次的最新 payload。这样彻底绕开"hide/show 不触发 onMounted"
// 和"Tauri 事件在预注册窗口上派发不稳"两个坑。
// 写入成功后 emit "write-hours-done" 让复盘窗把任务标灰，然后 write_hours_close
// 隐藏自己并 show avatar。

import { onMounted, onUnmounted, ref, nextTick } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { emit } from '@tauri-apps/api/event'
import ErrorBoundary from './components/ErrorBoundary.vue'

interface WriteHoursPayload {
  taskId: string
  taskName: string
  suggestedHours?: number
  content: string
  kind: 'task' | 'orphan'
}

const payload = ref<WriteHoursPayload | null>(null)
const taskIdInput = ref('')
const hours = ref('')
const content = ref('')
const submitting = ref(false)
const error = ref('')
const result = ref<'idle' | 'ok' | 'fail'>('idle')
const taskIdEl = ref<HTMLInputElement | null>(null)
const hoursEl = ref<HTMLInputElement | null>(null)

async function closeWindow() {
  if (submitting.value) return
  try {
    await invoke('write_hours_close')
  } catch (e) {
    console.error('write_hours_close 失败:', e)
    // 兜底拽回 avatar，避免用户陷入"小人消失只能重启 app"
    try { await invoke('avatar_show_fallback') } catch {}
  }
}

async function submit() {
  if (submitting.value) return
  const tid = taskIdInput.value.trim().replace(/^#/, '')
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
      // 通知复盘窗把这个任务标灰
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
  // Ctrl/Cmd+Enter 提交：textarea 里普通 Enter 仍换行，只有带修饰键才提交
  if (ev.key === 'Enter' && (ev.ctrlKey || ev.metaKey)) {
    ev.preventDefault()
    submit()
  }
}

onMounted(async () => {
  // Rust 端 write_hours_open 在写入 state 之后立刻 show + reload，所以这里
  // mount 时 state 一定已经有当次 payload，直接拉就行。
  window.addEventListener('keydown', onKeydown)
  await loadPayload()
  await nextTick()
  // 聚焦到第一个需要填的框：孤儿没 taskId 聚焦 taskId，否则聚焦工时
  if (!taskIdInput.value) taskIdEl.value?.focus()
  else hoursEl.value?.focus()
})

onUnmounted(() => {
  window.removeEventListener('keydown', onKeydown)
})
</script>

<template>
  <ErrorBoundary>
  <div class="wh-root">
    <header class="wh-header" data-tauri-drag-region>
      <h1 class="wh-title" data-tauri-drag-region>
        {{ payload?.kind === 'orphan' ? '✍️ 写到任务（手动填任务 ID）' : '✍️ 写入工时到禅道' }}
      </h1>
      <button class="wh-header-close" :disabled="submitting" @click="closeWindow" title="关闭">×</button>
    </header>

    <div class="wh-body">
      <div class="form-row">
        <label class="form-label">任务 ID</label>
        <input
          v-model="taskIdInput"
          ref="taskIdEl"
          class="form-input"
          type="text"
          inputmode="numeric"
          :disabled="submitting || result === 'ok'"
          placeholder="如 10238（不用带 # 号）"
        />
        <p v-if="payload?.kind === 'task'" class="form-hint">
          来自任务「{{ payload.taskName }}」，需要改写到其它任务可直接覆盖
        </p>
        <p v-else class="form-hint">
          这批 commit 没自动关联到任何任务，填一个真实任务 ID 把工时写过去
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
  background: linear-gradient(135deg, #1a2238, #0f172a);
  color: rgba(255, 255, 255, 0.92);
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", "PingFang SC", "Microsoft YaHei", sans-serif;
}

.wh-header {
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
.wh-title {
  margin: 0;
  font-size: 14px;
  font-weight: 600;
  color: rgba(255, 255, 255, 0.95);
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
  color: rgba(255, 255, 255, 0.6);
  font-size: 18px;
  cursor: pointer;
  padding: 0;
  line-height: 1;
}
.wh-header-close:hover:not(:disabled) {
  background: rgba(239, 68, 68, 0.25);
  color: rgba(255, 255, 255, 0.98);
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
  min-height: 140px;
  resize: vertical;
  line-height: 1.55;
  font-family: ui-monospace, SFMono-Regular, monospace;
  font-size: 12.5px;
}
.form-hint {
  margin: 0;
  font-size: 11.5px;
  color: rgba(255, 255, 255, 0.45);
  line-height: 1.5;
}

.form-error {
  margin: 0;
  padding: 8px 10px;
  font-size: 12px;
  color: rgba(252, 165, 165, 0.95);
  background: rgba(239, 68, 68, 0.12);
  border-left: 3px solid rgba(239, 68, 68, 0.5);
  border-radius: 4px;
  word-break: break-word;
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

.wh-footer {
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
