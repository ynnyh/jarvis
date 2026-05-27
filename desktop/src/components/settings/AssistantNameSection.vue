<script setup lang="ts">
import { useConfigStore } from '../../stores/config'

const store = useConfigStore()
</script>

<template>
  <section class="settings-section">
    <h3 class="settings-section-title">称呼</h3>
    <label class="settings-field">
      <span class="settings-field-label">助手名字</span>
      <input class="settings-input" type="text" maxlength="16" placeholder="Jarvis"
        v-model="store.config.assistantName" />
    </label>
    <label class="settings-field">
      <span class="settings-field-label">怎么称呼你</span>
      <input class="settings-input" type="text" maxlength="16" placeholder="主人"
        v-model="store.config.userTitle" />
    </label>
    <p class="settings-section-hint">影响菜单、问候、通知标题等所有显示文案，重启或切换面板即时生效</p>
  </section>
</template>
