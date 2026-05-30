<script setup lang="ts">
import { ref, onErrorCaptured } from 'vue'

const hasError = ref(false)
const errorMsg = ref('')

onErrorCaptured((err) => {
  hasError.value = true
  errorMsg.value = err instanceof Error ? err.message : String(err)
  console.error('[ErrorBoundary]', err)
  return false // 不再向上传播
})

function retry() {
  hasError.value = false
  errorMsg.value = ''
}
</script>

<template>
  <template v-if="hasError">
    <div class="error-fallback">
      <span class="error-fallback__icon">⚠️</span>
      <span class="error-fallback__text">界面出了点问题</span>
      <button class="error-fallback__retry" @click="retry">重试</button>
    </div>
  </template>
  <slot v-else />
</template>

<style scoped>
.error-fallback {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 10px 16px;
  background: rgba(15, 23, 42, 0.96);
  border: 1px solid rgba(239, 68, 68, 0.3);
  border-radius: 10px;
  color: rgba(255, 255, 255, 0.85);
  font-size: 12px;
}
.error-fallback__icon { font-size: 14px; }
.error-fallback__text { flex: 1; }
.error-fallback__retry {
  padding: 3px 10px;
  font-size: 11px;
  color: rgba(255, 255, 255, 0.85);
  background: rgba(255, 255, 255, 0.08);
  border: 1px solid rgba(255, 255, 255, 0.12);
  border-radius: 6px;
  cursor: pointer;
}
.error-fallback__retry:hover {
  background: rgba(0, 212, 255, 0.16);
  border-color: rgba(0, 212, 255, 0.32);
}
</style>
