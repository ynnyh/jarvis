<script setup lang="ts">
import { useConfigStore } from '../../stores/config'
import ToggleSwitch from '../ui/ToggleSwitch.vue'

const store = useConfigStore()
</script>

<template>
  <section class="settings-section">
    <h3 class="settings-section-title">主动提醒</h3>
    <ToggleSwitch v-model="store.config.notifications.morningGreeting" label="上班时打个招呼" />
    <div class="ritual-row">
      <ToggleSwitch v-model="store.config.notifications.todayPlanPromptEnabled" label="今日计划提醒" />
      <input
        class="settings-inline-time"
        type="time"
        v-model="store.config.notifications.todayPlanPromptTime"
        :disabled="!store.config.notifications.todayPlanPromptEnabled"
      />
    </div>
    <div class="ritual-row">
      <ToggleSwitch v-model="store.config.notifications.eveningSummary" label="下班前晚总结" />
      <input
        class="settings-inline-num"
        type="number"
        min="5"
        max="120"
        step="5"
        v-model.number="store.config.notifications.eveningSummaryMinutesBefore"
        :disabled="!store.config.notifications.eveningSummary"
      />
      <span class="ritual-inline-label">分钟提醒我做收尾</span>
    </div>
    <ToggleSwitch v-model="store.config.notifications.eveningSummaryChannelNotify" label="同时推送到手机" :disabled="!store.config.notifications.eveningSummary" />
  </section>
</template>

<style scoped>
.ritual-row {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}
.ritual-inline-label {
  font-size: 12px;
  color: var(--text-dim);
}
</style>
