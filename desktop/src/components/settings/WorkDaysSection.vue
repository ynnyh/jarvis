<script setup lang="ts">
import { useConfigStore } from '../../stores/config'

const store = useConfigStore()

const DAYS = [
  { value: 1, label: '一' },
  { value: 2, label: '二' },
  { value: 3, label: '三' },
  { value: 4, label: '四' },
  { value: 5, label: '五' },
  { value: 6, label: '六' },
  { value: 0, label: '日' },
]

function toggle(day: number) {
  const days = store.config.workSchedule.workDays
  const i = days.indexOf(day)
  if (i >= 0) days.splice(i, 1)
  else { days.push(day); days.sort() }
}
</script>

<template>
  <section class="settings-section">
    <h3 class="settings-section-title">工作日</h3>
    <div class="weekday-row">
      <button
        v-for="d in DAYS"
        :key="d.value"
        class="weekday-btn"
        :class="{ active: store.config.workSchedule.workDays.includes(d.value) }"
        @click="toggle(d.value)"
      >{{ d.label }}</button>
    </div>
  </section>
</template>

<style scoped>
.weekday-row { display: flex; gap: 4px; }
.weekday-btn {
  flex: 1;
  height: 26px;
  font-size: 11px;
  color: rgba(255, 255, 255, 0.55);
  background: rgba(255, 255, 255, 0.04);
  border: 1px solid rgba(255, 255, 255, 0.06);
  border-radius: 6px;
  cursor: pointer;
  transition: all 0.15s;
}
.weekday-btn.active {
  color: rgba(0, 212, 255, 0.95);
  background: rgba(0, 212, 255, 0.12);
  border-color: rgba(0, 212, 255, 0.4);
}
</style>
