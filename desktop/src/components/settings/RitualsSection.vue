<script setup lang="ts">
import { useConfigStore } from '../../stores/config'
const store = useConfigStore()
</script>
<template>
  <section class="settings-section">
    <h3 class="settings-section-title">????</h3>
    <label class="settings-toggle">
      <input type="checkbox" v-model="store.config.notifications.morningGreeting" />
      <span>?????????????</span>
    </label>
    <label class="settings-toggle">
      <input type="checkbox" v-model="store.config.notifications.todayPlanPromptEnabled" />
      <span>??????????
        <input
          class="settings-inline-time"
          type="time"
          v-model="store.config.notifications.todayPlanPromptTime"
        />
      </span>
    </label>
    <label class="settings-toggle">
      <input type="checkbox" v-model="store.config.notifications.eveningSummary" />
      <span>???
        <input
          class="settings-inline-num"
          type="number"
          min="5" max="120" step="5"
          v-model.number="store.config.notifications.eveningSummaryMinutesBefore"
        />
        ???????
      </span>
    </label>
  </section>
</template>
