<script setup lang="ts">
import { useConfigStore } from '../../stores/config'
import ToggleSwitch from '../ui/ToggleSwitch.vue'

const store = useConfigStore()
</script>

<template>
  <section class="settings-section">
    <h3 class="settings-section-title">静默规则</h3>
    <ToggleSwitch v-model="store.config.notifications.quietDuringLunch" label="午休时段静默（不弹通知）" />
    <ToggleSwitch v-model="store.config.notifications.quietAfterWork" label="下班后 / 上班前静默" />
    <ToggleSwitch v-model="store.config.notifications.quietOnWeekends" label="周末整天静默" />
  </section>
</template>
