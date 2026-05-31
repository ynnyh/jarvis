<script setup lang="ts">
import { useConfigStore } from '../../stores/config'

const store = useConfigStore()
</script>

<template>
  <section class="settings-section">
    <h3 class="settings-section-title">主动提醒</h3>
    <label class="settings-toggle">
      <input type="checkbox" v-model="store.config.notifications.morningGreeting" />
      <span>上班时打个招呼</span>
    </label>
    <label class="settings-toggle">
      <input type="checkbox" v-model="store.config.notifications.todayPlanPromptEnabled" />
      <span>
        在
        <input
          class="settings-inline-time"
          type="time"
          v-model="store.config.notifications.todayPlanPromptTime"
        />
        提醒我确认今日计划
      </span>
    </label>
    <label class="settings-toggle">
      <input type="checkbox" v-model="store.config.notifications.eveningSummary" />
      <span>
        下班前
        <input
          class="settings-inline-num"
          type="number"
          min="5"
          max="120"
          step="5"
          v-model.number="store.config.notifications.eveningSummaryMinutesBefore"
        />
        分钟提醒我做收尾
      </span>
    </label>
  </section>
</template>
