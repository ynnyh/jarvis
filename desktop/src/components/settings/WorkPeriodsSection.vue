<script setup lang="ts">
import { useConfigStore } from '../../stores/config'

const store = useConfigStore()
</script>

<template>
  <section class="settings-section">
    <h3 class="settings-section-title">工作时段</h3>
    <div class="periods">
      <div
        v-for="(p, i) in store.config.workSchedule.periods"
        :key="i"
        class="period-row"
      >
        <input class="period-label" v-model="p.label" placeholder="名称" />
        <input class="time-input" type="time" v-model="p.start" />
        <span class="dash">—</span>
        <input class="time-input" type="time" v-model="p.end" />
      </div>
    </div>
    <p class="settings-section-hint">改完即时生效，自动保存</p>
  </section>
</template>

<style scoped>
.periods { display: flex; flex-direction: column; gap: 4px; }
.period-row {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 4px 0;
}
.period-label {
  width: 50px;
  padding: 3px 6px;
  font-size: 11px;
  color: rgba(255, 255, 255, 0.85);
  background: rgba(255, 255, 255, 0.06);
  border: 1px solid rgba(255, 255, 255, 0.08);
  border-radius: 4px;
}
.time-input {
  padding: 3px 6px;
  font-size: 11px;
  font-family: inherit;
  color: rgba(255, 255, 255, 0.92);
  background: rgba(255, 255, 255, 0.06);
  border: 1px solid rgba(255, 255, 255, 0.08);
  border-radius: 4px;
}
.dash { color: rgba(255, 255, 255, 0.4); font-size: 11px; }
</style>
