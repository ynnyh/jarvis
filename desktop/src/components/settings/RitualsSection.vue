<script setup lang="ts">
import { useConfigStore } from '../../stores/config'

const store = useConfigStore()
</script>

<template>
  <section class="settings-section">
    <h3 class="settings-section-title">早晚仪式</h3>
    <label class="settings-toggle">
      <input type="checkbox" v-model="store.config.notifications.morningGreeting" />
      <span>上班时弹早安卡片 + 今日速览</span>
    </label>
    <label class="settings-toggle">
      <input type="checkbox" v-model="store.config.notifications.eveningSummary" />
      <span>下班前
        <input
          class="settings-inline-num"
          type="number"
          min="5" max="120" step="5"
          v-model.number="store.config.notifications.eveningSummaryMinutesBefore"
        />
        分钟弹日终总结
      </span>
    </label>
  </section>
</template>
