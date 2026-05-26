<script setup lang="ts">
// 今日临时覆盖：仅当天有效，次日自动恢复正常。setTodayMode 由 store 维护（写
// override.todayMode + todayModeSetOn 用日期判断当天）。

import { useConfigStore } from '../../stores/config'

const store = useConfigStore()
</script>

<template>
  <section class="settings-section">
    <h3 class="settings-section-title">今日临时覆盖</h3>
    <div class="mode-row">
      <button
        class="mode-btn"
        :class="{ active: store.config.override.todayMode === 'normal' }"
        @click="store.setTodayMode('normal')"
      >正常</button>
      <button
        class="mode-btn"
        :class="{ active: store.config.override.todayMode === 'overtime' }"
        @click="store.setTodayMode('overtime')"
      >今晚加班</button>
      <button
        class="mode-btn"
        :class="{ active: store.config.override.todayMode === 'dayoff' }"
        @click="store.setTodayMode('dayoff')"
      >今天休假</button>
    </div>
    <p class="settings-section-hint">仅当天有效，次日自动恢复正常</p>
  </section>
</template>

<style scoped>
.mode-row { display: flex; gap: 4px; }
.mode-btn {
  flex: 1;
  padding: 6px 4px;
  font-size: 11px;
  color: rgba(255, 255, 255, 0.65);
  background: rgba(255, 255, 255, 0.04);
  border: 1px solid rgba(255, 255, 255, 0.08);
  border-radius: 6px;
  cursor: pointer;
  transition: all 0.15s;
}
.mode-btn.active {
  color: rgba(245, 158, 11, 0.98);
  background: rgba(245, 158, 11, 0.15);
  border-color: rgba(245, 158, 11, 0.4);
}
</style>
