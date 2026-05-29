<script setup lang="ts">
import { watch } from 'vue'
import { enable, disable, isEnabled } from '@tauri-apps/plugin-autostart'
import { useConfigStore } from '../../stores/config'

const store = useConfigStore()

// config 写入时同步到 OS
watch(() => store.config.autoStartOnBoot, async (on) => {
  try {
    if (on) {
      await enable()
    } else {
      await disable()
    }
  } catch (e) {
    console.error('[autostart] 同步失败:', e)
  }
})

// 首次加载时校准：OS 实际状态可能与 config 不一致（用户手动改了系统设置）
isEnabled().then((ok) => {
  if (ok !== store.config.autoStartOnBoot) {
    store.config.autoStartOnBoot = ok
  }
}).catch(() => {})
</script>

<template>
  <section class="settings-section">
    <h3 class="settings-section-title">开机自启</h3>
    <label class="settings-field">
      <span class="settings-field-label">登录时自动启动</span>
      <input type="checkbox" v-model="store.config.autoStartOnBoot" />
    </label>
    <p class="settings-section-hint">开启后系统登录时自动启动 {{ store.config.assistantName }}，可在系统设置中手动管理</p>
  </section>
</template>
