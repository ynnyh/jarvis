<script setup lang="ts">
import { useConfigStore } from '../../stores/config'

const store = useConfigStore()
</script>

<template>
  <section class="settings-section">
    <h3 class="settings-section-title">静默规则</h3>
    <label class="settings-toggle">
      <input type="checkbox" v-model="store.config.notifications.quietDuringLunch" />
      <span>午休时段静默（不弹通知）</span>
    </label>
    <label class="settings-toggle">
      <input type="checkbox" v-model="store.config.notifications.quietAfterWork" />
      <span>下班后 / 上班前静默</span>
    </label>
    <label class="settings-toggle">
      <input type="checkbox" v-model="store.config.notifications.quietOnWeekends" />
      <span>周末整天静默</span>
    </label>
  </section>
</template>
