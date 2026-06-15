<script setup lang="ts">
import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'

const exporting = ref(false)
const message = ref('')
const messageType = ref<'success' | 'error'>('success')

async function exportLogs() {
  if (exporting.value) return
  exporting.value = true
  message.value = ''
  try {
    const savedPath = await invoke<string>('export_diagnostic_logs')
    message.value = `已导出到：${savedPath}`
    messageType.value = 'success'
  } catch (e) {
    const err = String(e)
    // 用户取消保存框不算错误
    if (err.includes('取消')) {
      message.value = ''
    } else {
      message.value = `导出失败：${err}`
      messageType.value = 'error'
    }
  } finally {
    exporting.value = false
  }
}
</script>

<template>
  <section class="settings-section">
    <h3 class="settings-section-title">诊断日志</h3>
    <p class="settings-section-hint">
      遇到问题时导出最近 3 天的日志和环境摘要，方便排查。导出内容已脱敏，不含密码或 API Key。
    </p>
    <button
      class="diag-export-btn"
      :disabled="exporting"
      @click="exportLogs"
    >
      {{ exporting ? '正在导出…' : '导出诊断日志' }}
    </button>
    <p v-if="message" class="diag-message" :class="messageType">{{ message }}</p>
  </section>
</template>

<style scoped>
.diag-export-btn {
  margin-top: 8px;
  padding: 8px 16px;
  font-size: 13px;
  font-weight: 500;
  color: var(--accent-text);
  background: var(--surface);
  border: 1px solid var(--divider-color, var(--divider));
  border-radius: 6px;
  cursor: pointer;
  transition: background 0.15s;
}
.diag-export-btn:hover:not(:disabled) {
  background: var(--surface-hover, var(--surface));
}
.diag-export-btn:disabled {
  opacity: 0.6;
  cursor: not-allowed;
}
.diag-message {
  margin-top: 8px;
  font-size: 12.5px;
  word-break: break-all;
}
.diag-message.success {
  color: var(--text-ghost);
}
.diag-message.error {
  color: var(--danger, #e5484d);
}
</style>
