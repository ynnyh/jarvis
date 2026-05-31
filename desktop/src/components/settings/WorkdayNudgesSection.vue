<script setup lang="ts">
import { useConfigStore } from '../../stores/config'

const store = useConfigStore()
</script>

<template>
  <section class="settings-section">
    <h3 class="settings-section-title">工作时段小提示</h3>
    <label class="settings-toggle">
      <input type="checkbox" v-model="store.config.notifications.workdayNudges" />
      <span>在工作时段里定时弹出喝水、起身、提肛、午饭、下班提醒</span>
    </label>
    <label class="settings-toggle">
      <span>
        喝水、起身、提肛每
        <input
          class="settings-inline-num"
          type="number"
          min="30"
          max="240"
          step="15"
          v-model.number="store.config.notifications.nudgeIntervalMinutes"
          :disabled="!store.config.notifications.workdayNudges"
        />
        分钟轮一次
      </span>
    </label>
    <p class="settings-section-hint">
      午饭前 10 分钟、下班前 10 分钟为固定时间锚点，自动只提醒一次。所有提示在静默时段都不会弹。
    </p>
  </section>
</template>
