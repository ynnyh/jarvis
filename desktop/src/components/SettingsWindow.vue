<script setup lang="ts">
import { computed } from 'vue'
import { useConfigStore } from '../stores/config'

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

const phaseLabel = computed(() => {
  switch (store.phase) {
    case 'working': return '工作中'
    case 'lunch': return '午休'
    case 'before-work': return '尚未上班'
    case 'after-work': return '已下班'
    case 'weekend': return '周末'
    case 'dayoff': return '今天休假'
    case 'overtime': return '加班模式'
    default: return ''
  }
})

function toggleWorkDay(day: number) {
  const days = store.config.workSchedule.workDays
  const i = days.indexOf(day)
  if (i >= 0) days.splice(i, 1)
  else { days.push(day); days.sort() }
}
</script>

<template>
  <Transition name="panel">
    <div v-if="store.showSettingsWindow" class="settings-panel pointer-target">
      <header class="panel-header">
        <div class="panel-title">
          <span class="title-icon">⚙️</span>
          <span class="title-text">设置</span>
        </div>
        <button class="icon-btn" title="关闭" @click="store.showSettingsWindow = false">×</button>
      </header>

      <!-- 当前状态条 -->
      <div class="phase-bar" :class="`phase-${store.phase}`">
        <span class="phase-dot" />
        <span>当前：{{ phaseLabel }}</span>
        <span v-if="store.isQuietHours" class="phase-meta">静默中</span>
      </div>

      <div class="panel-body">
        <!-- 工作日 -->
        <section class="section">
          <h3 class="section-title">工作日</h3>
          <div class="weekday-row">
            <button
              v-for="d in DAYS"
              :key="d.value"
              class="weekday-btn"
              :class="{ active: store.config.workSchedule.workDays.includes(d.value) }"
              @click="toggleWorkDay(d.value)"
            >{{ d.label }}</button>
          </div>
        </section>

        <!-- 工作时段 -->
        <section class="section">
          <h3 class="section-title">工作时段</h3>
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
          <p class="section-hint">改完即时生效，自动保存</p>
        </section>

        <!-- 静默规则 -->
        <section class="section">
          <h3 class="section-title">静默规则</h3>
          <label class="toggle-row">
            <input type="checkbox" v-model="store.config.notifications.quietDuringLunch" />
            <span>午休时段静默（不弹通知）</span>
          </label>
          <label class="toggle-row">
            <input type="checkbox" v-model="store.config.notifications.quietAfterWork" />
            <span>下班后 / 上班前静默</span>
          </label>
          <label class="toggle-row">
            <input type="checkbox" v-model="store.config.notifications.quietOnWeekends" />
            <span>周末整天静默</span>
          </label>
        </section>

        <!-- 仪式感 -->
        <section class="section">
          <h3 class="section-title">早晚仪式</h3>
          <label class="toggle-row">
            <input type="checkbox" v-model="store.config.notifications.morningGreeting" />
            <span>上班时弹早安卡片 + 今日速览</span>
          </label>
          <label class="toggle-row">
            <input type="checkbox" v-model="store.config.notifications.eveningSummary" />
            <span>下班前
              <input
                class="inline-num"
                type="number"
                min="5" max="120" step="5"
                v-model.number="store.config.notifications.eveningSummaryMinutesBefore"
              />
              分钟弹日终总结
            </span>
          </label>
        </section>

        <!-- 今日临时覆盖 -->
        <section class="section">
          <h3 class="section-title">今日临时覆盖</h3>
          <div class="mode-row">
            <button
              class="mode-btn"
              :class="{ active: store.config.override.todayMode === 'normal' }"
              @click="store.setTodayMode('normal')"
            >正常</button>
            <button
              class="mode-btn"
              :class="{ active: store.config.override.todayMode === 'overtime' }"
              @click="store.setTodayMode('overtime')"
            >今晚加班</button>
            <button
              class="mode-btn"
              :class="{ active: store.config.override.todayMode === 'dayoff' }"
              @click="store.setTodayMode('dayoff')"
            >今天休假</button>
          </div>
          <p class="section-hint">仅当天有效，次日自动恢复正常</p>
        </section>
      </div>
    </div>
  </Transition>
</template>

<style scoped>
.settings-panel {
  position: fixed;
  inset: 8px 8px 90px 8px;
  display: flex;
  flex-direction: column;
  background: linear-gradient(135deg, rgba(20, 30, 56, 0.97), rgba(15, 23, 42, 0.97));
  border: 1px solid rgba(100, 200, 255, 0.16);
  border-radius: 14px;
  box-shadow: 0 12px 32px rgba(0, 0, 0, 0.5);
  overflow: hidden;
  z-index: 60;
  color: rgba(255, 255, 255, 0.92);
}

.panel-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 8px 10px;
  background: rgba(0, 0, 0, 0.2);
  border-bottom: 1px solid rgba(255, 255, 255, 0.06);
}
.panel-title {
  display: flex; align-items: center; gap: 6px;
  font-size: 13px; font-weight: 600;
}
.title-icon { font-size: 14px; }
.icon-btn {
  width: 22px; height: 22px;
  display: inline-flex; align-items: center; justify-content: center;
  font-size: 16px; line-height: 1;
  color: rgba(255, 255, 255, 0.55);
  background: transparent; border: none; border-radius: 6px;
  cursor: pointer;
}
.icon-btn:hover { color: rgba(255, 255, 255, 0.95); background: rgba(255, 255, 255, 0.08); }

.phase-bar {
  display: flex; align-items: center; gap: 6px;
  padding: 4px 10px;
  font-size: 10px;
  background: rgba(0, 0, 0, 0.15);
  border-bottom: 1px solid rgba(255, 255, 255, 0.04);
  color: rgba(255, 255, 255, 0.65);
}
.phase-dot { width: 6px; height: 6px; border-radius: 50%; background: rgba(16, 185, 129, 0.9); }
.phase-working .phase-dot { background: rgba(16, 185, 129, 0.95); }
.phase-lunch .phase-dot { background: rgba(167, 139, 250, 0.95); }
.phase-after-work .phase-dot,
.phase-before-work .phase-dot { background: rgba(148, 163, 184, 0.7); }
.phase-weekend .phase-dot,
.phase-dayoff .phase-dot { background: rgba(245, 158, 11, 0.9); }
.phase-meta { margin-left: auto; color: rgba(245, 158, 11, 0.85); }

.panel-body {
  flex: 1;
  overflow-y: auto;
  padding: 10px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.section { display: flex; flex-direction: column; gap: 6px; }
.section-title {
  margin: 0;
  font-size: 11px;
  font-weight: 600;
  color: rgba(0, 212, 255, 0.85);
  letter-spacing: 0.5px;
}
.section-hint {
  margin: 2px 0 0;
  font-size: 9.5px;
  color: rgba(255, 255, 255, 0.35);
}

/* 工作日按钮 */
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

/* 工作时段 */
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

/* 切换开关 */
.toggle-row {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 11.5px;
  color: rgba(255, 255, 255, 0.85);
  cursor: pointer;
  padding: 2px 0;
}
.toggle-row input[type=checkbox] {
  width: 14px;
  height: 14px;
  accent-color: rgba(0, 212, 255, 0.9);
}
.inline-num {
  width: 42px;
  margin: 0 2px;
  padding: 2px 4px;
  font-size: 11px;
  text-align: center;
  color: rgba(255, 255, 255, 0.92);
  background: rgba(255, 255, 255, 0.06);
  border: 1px solid rgba(255, 255, 255, 0.08);
  border-radius: 4px;
}

/* 模式按钮 */
.mode-row { display: flex; gap: 4px; }
.mode-btn {
  flex: 1;
  padding: 6px 4px;
  font-size: 11px;
  color: rgba(255, 255, 255, 0.65);
  background: rgba(255, 255, 255, 0.04);
  border: 1px solid rgba(255, 255, 255, 0.08);
  border-radius: 6px;
  cursor: pointer;
  transition: all 0.15s;
}
.mode-btn.active {
  color: rgba(245, 158, 11, 0.98);
  background: rgba(245, 158, 11, 0.15);
  border-color: rgba(245, 158, 11, 0.4);
}

.panel-enter-active,
.panel-leave-active {
  transition: opacity 0.2s ease, transform 0.2s ease;
}
.panel-enter-from,
.panel-leave-to {
  opacity: 0;
  transform: translateY(6px) scale(0.98);
}
</style>
