<script setup lang="ts">
import { ref, computed, watch } from 'vue'
import { useConfigStore } from '../../stores/config'
import ToggleSwitch from '../ui/ToggleSwitch.vue'

const store = useConfigStore()

type Direction = 'before' | 'exact' | 'after'

const direction = ref<Direction>('before')
const absMinutes = ref<number>(30)

// 从磁盘加载的配置同步到本地双控件
watch(
  () => store.config.notifications.eveningSummaryMinutesBefore,
  (raw) => {
    const v = Number(raw ?? 30)
    if (v > 0) {
      direction.value = 'before'
      absMinutes.value = v
    } else if (v === 0) {
      direction.value = 'exact'
      // exact 不使用分钟数，保留上次选的值以免用户来回切换丢数据
    } else {
      direction.value = 'after'
      absMinutes.value = Math.max(1, Math.min(240, -v))
    }
  },
  { immediate: true }
)

// 本地双控件变更写回配置（兼容旧的正负存储格式）
watch([direction, absMinutes], ([dir, mins]) => {
  const m = Math.max(1, Math.min(240, Number(mins) || 0))
  let val: number
  if (dir === 'before') val = m
  else if (dir === 'after') val = -m
  else val = 0
  store.config.notifications.eveningSummaryMinutesBefore = val
})

const lastWorkEnd = computed(() => {
  const ends = store.config.workSchedule.periods
    .map(p => p.end)
    .filter(Boolean)
    .sort()
  return ends[ends.length - 1] || '18:00'
})

// 预计提醒时间预览
const reminderTimePreview = computed(() => {
  const [h, m] = lastWorkEnd.value.split(':').map(Number)
  const base = (Number.isFinite(h) ? h : 18) * 60 + (Number.isFinite(m) ? m : 0)
  const before = Number(store.config.notifications.eveningSummaryMinutesBefore ?? 30)
  const total = base - before
  const hh = ((Math.floor(total / 60) % 24) + 24) % 24
  const mm = ((total % 60) + 60) % 60
  return `${String(hh).padStart(2, '0')}:${String(mm).padStart(2, '0')}`
})

const disabled = computed(() => !store.config.notifications.eveningSummary)
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

    <ToggleSwitch v-model="store.config.notifications.eveningSummary" label="晚总结提醒" />
    <div class="evening-summary-row">
      <select
        class="settings-input settings-select"
        v-model="direction"
        :disabled="disabled"
      >
        <option value="before">下班前</option>
        <option value="exact">准点下班</option>
        <option value="after">下班后</option>
      </select>
      <input
        v-if="direction !== 'exact'"
        class="settings-input settings-num"
        type="number"
        min="1"
        max="240"
        step="1"
        v-model.number="absMinutes"
        :disabled="disabled"
      />
      <span v-if="direction !== 'exact'" class="ritual-inline-label">分钟</span>
      <span v-else class="ritual-inline-label">&nbsp;</span>
      <span class="ritual-preview">预计 {{ reminderTimePreview }} 提醒</span>
    </div>
    <ToggleSwitch
      v-model="store.config.notifications.eveningSummaryChannelNotify"
      label="同时推送到手机"
      :disabled="disabled"
    />
  </section>
</template>

<style scoped>
.ritual-row {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}
.evening-summary-row {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
  padding: 4px 0 4px 28px;
}
.settings-select {
  appearance: none;
  -webkit-appearance: none;
  padding-right: 28px;
  background-image: url("data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' width='10' height='6' viewBox='0 0 10 6'><path fill='%23888' d='M0 0l5 6 5-6z'/></svg>");
  background-repeat: no-repeat;
  background-position: right 10px center;
  cursor: pointer;
}
.settings-select:disabled {
  cursor: not-allowed;
  opacity: 0.6;
}
.settings-num {
  width: 90px;
}
.ritual-inline-label {
  font-size: 12px;
  color: var(--text-dim);
}
.ritual-preview {
  font-size: 12px;
  color: var(--text-dim);
  padding: 2px 8px;
  background: var(--surface);
  border: var(--divider);
  border-radius: 4px;
}
</style>
