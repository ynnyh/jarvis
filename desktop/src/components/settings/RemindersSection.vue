<script setup lang="ts">
import { onMounted } from 'vue'
import { useConfigStore } from '../../stores/config'
import type { ScheduledReminder } from '../../stores/config'

const store = useConfigStore()

onMounted(() => { store.refreshReminders() })

function removeReminder(index: number) {
  store.config.reminders.splice(index, 1)
}

function toggleReminder(r: ScheduledReminder) {
  r.enabled = !r.enabled
}

function cronHuman(cron: string): string {
  const parts = cron.split(/\s+/)
  if (parts.length !== 5) return cron
  const [min, hour, day, month, dow] = parts
  if (day === '*' && month === '*' && dow === '*') {
    return `每天 ${hour}:${min.padStart(2, '0')}`
  }
  if (day === '*' && month === '*') {
    return `${cron}`
  }
  return cron
}
</script>

<template>
  <section class="settings-section">
    <h3 class="settings-section-title">定时提醒</h3>
    <p class="settings-section-hint">
      通过 Telegram/QQ 机器人发送「定时 17:30 写日报」来添加提醒，到时间会同时通过机器人和桌面气泡提醒。
    </p>
    <div v-if="store.config.reminders.length === 0" class="reminder-empty">
      暂无定时提醒
    </div>
    <div v-else class="reminder-list">
      <div v-for="(r, i) in store.config.reminders" :key="r.id" class="reminder-item">
        <label class="reminder-toggle">
          <input type="checkbox" :checked="r.enabled" @change="toggleReminder(r)" />
          <span class="reminder-cron">{{ cronHuman(r.cron) }}</span>
        </label>
        <span class="reminder-message">{{ r.message }}</span>
        <button class="reminder-delete" @click="removeReminder(i)" title="删除">×</button>
      </div>
    </div>
    <p class="settings-section-hint">
      机器人命令：「定时 HH:MM 内容」「定时列表」「删除定时 N」
    </p>
  </section>
</template>

<style scoped>
.reminder-empty {
  padding: 12px 0;
  color: var(--text-faint);
  font-size: 12.5px;
}
.reminder-list {
  display: flex;
  flex-direction: column;
  gap: 6px;
  margin: 8px 0;
}
.reminder-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 10px;
  background: var(--surface);
  border: var(--divider);
  border-radius: 6px;
  font-size: 12.5px;
}
.reminder-toggle {
  display: flex;
  align-items: center;
  gap: 6px;
  cursor: pointer;
  flex-shrink: 0;
}
.reminder-toggle input[type="checkbox"] {
  -webkit-appearance: none;
  appearance: none;
  position: relative;
  width: 36px;
  height: 20px;
  margin: 0;
  background: var(--surface-2);
  border: 1px solid var(--border);
  border-radius: 10px;
  outline: none;
  cursor: pointer;
  transition: background 0.2s ease, border-color 0.2s ease;
  flex-shrink: 0;
}
.reminder-toggle input[type="checkbox"]::after {
  content: '';
  position: absolute;
  top: 2px;
  left: 2px;
  width: 14px;
  height: 14px;
  background: #fff;
  border-radius: 50%;
  transition: transform 0.2s ease;
  pointer-events: none;
}
.reminder-toggle input[type="checkbox"]:checked {
  background: var(--accent);
  border-color: var(--accent);
}
.reminder-toggle input[type="checkbox"]:checked::after {
  transform: translateX(16px);
}
.reminder-cron {
  font-family: ui-monospace, monospace;
  font-size: 11px;
  color: var(--accent-text);
  white-space: nowrap;
}
.reminder-message {
  flex: 1;
  color: var(--text);
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.reminder-delete {
  flex-shrink: 0;
  width: 20px;
  height: 20px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: none;
  border: none;
  color: var(--text-faint);
  cursor: pointer;
  border-radius: 4px;
  font-size: 14px;
}
.reminder-delete:hover {
  color: var(--red-text);
  background: var(--red-bg);
}
</style>
