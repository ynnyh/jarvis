<script setup lang="ts">
import { computed } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useAppStore } from '../stores/app'
import { useConfigStore } from '../stores/config'

const store = useAppStore()
const configStore = useConfigStore()

// 未来 7 天累计可用工时（排除休息日 + 尊重今日临时覆盖）
const availableHours = computed(() => configStore.availableHoursIn7Days)
const workingDays = computed(() => configStore.workingDaysIn7)

const overloadRatio = computed(() => {
  if (availableHours.value <= 0) return 0
  return store.myRemainingHours / availableHours.value
})

const top5 = computed(() => store.urgencyScored.slice(0, 5))

function scoreColor(score: number): string {
  if (score >= 140) return 'danger'
  if (score >= 90) return 'warn'
  if (score >= 50) return 'soon'
  return 'mild'
}

// 智能建议（规则化生成）
const suggestions = computed<string[]>(() => {
  const out: string[] = []
  const u = store.urgencyScored
  if (u.length === 0) {
    return ['📭 当前 7 天内无紧迫任务，可以喘口气，或主动推进长期任务。']
  }
  const overdue = u.filter(e => e.alert.daysUntilDue < 0)
  if (overdue.length > 0) {
    const worst = overdue[0]
    out.push(`🔥 先处理「${worst.alert.title.slice(0, 24)}」，已逾期 ${-worst.alert.daysUntilDue} 天，再拖延会影响考核。`)
  }
  if (availableHours.value > 0) {
    if (overloadRatio.value > 1.5) {
      out.push(`⚠️ 未来 7 天你有 ${workingDays.value} 个工作日 / ${availableHours.value}h 可用，但身上还剩 ${store.myRemainingHours.toFixed(1)}h，超载 ${((overloadRatio.value - 1) * 100).toFixed(0)}%。建议主攻 Top 3、其余跟对应负责人沟通延期或转交。`)
    } else if (overloadRatio.value > 1) {
      out.push(`📊 未来 7 天 ${workingDays.value} 个工作日（${availableHours.value}h）排得很满（${(overloadRatio.value * 100).toFixed(0)}%），节奏要紧。`)
    } else if (overloadRatio.value > 0.8) {
      out.push(`✓ 未来 7 天工时排得比较饱（${(overloadRatio.value * 100).toFixed(0)}%），按当前优先级推进即可。`)
    }
  }
  for (const s of store.stackedDays) {
    const total = s.tasks.reduce((sum, t) => sum + t.leftHours, 0)
    out.push(`📅 ${s.date}（${s.daysFromToday === 0 ? '今天' : `${s.daysFromToday} 天后`}）有 ${s.count} 个任务同日截止，合计 ${total.toFixed(1)}h，建议提前 1-2 天分散处理。`)
  }
  if (out.length === 0) {
    out.push('✓ 节奏正常，按当前优先级推进即可。')
  }
  return out
})

async function openTask(id: string) {
  try {
    await invoke('open_zentao_task', { id })
  } catch (e) {
    console.error('打开禅道任务失败:', e)
  }
}
</script>

<template>
  <Transition name="panel">
    <div v-if="store.showRiskWindow" class="risk-panel pointer-target">
      <header class="panel-header">
        <div class="panel-title">
          <span class="title-icon">⚠️</span>
          <span class="title-text">风险分析</span>
        </div>
        <button class="icon-btn" title="关闭" @click="store.showRiskWindow = false">×</button>
      </header>

      <div class="panel-body">
        <!-- 工时预算 -->
        <section class="section">
          <h3 class="section-title">📊 工时预算（未来 7 天）</h3>
          <div class="budget-row">
            <div class="budget-item">
              <div class="budget-num">{{ availableHours.toFixed(0) }}h</div>
              <div class="budget-label">可用（{{ workingDays }} 个工作日）</div>
            </div>
            <div class="budget-divider">/</div>
            <div class="budget-item">
              <div class="budget-num">{{ store.myRemainingHours.toFixed(1) }}h</div>
              <div class="budget-label">我剩余</div>
            </div>
            <div class="budget-divider">=</div>
            <div class="budget-item" :class="`ratio-${overloadRatio > 1.2 ? 'high' : overloadRatio > 0.8 ? 'mid' : 'low'}`">
              <div class="budget-num">{{ (overloadRatio * 100).toFixed(0) }}%</div>
              <div class="budget-label">饱和度</div>
            </div>
          </div>
          <div class="budget-bar">
            <div
              class="budget-fill"
              :class="`fill-${overloadRatio > 1.2 ? 'high' : overloadRatio > 0.8 ? 'mid' : 'low'}`"
              :style="{ width: Math.min(200, overloadRatio * 100) + '%' }"
            />
            <div class="budget-line" />
          </div>
          <p class="section-hint">100% 线代表"工作量等于未来 7 天可用工时"。已排除休息日，按设置里的工作日和时段计算。</p>
        </section>

        <!-- 紧迫度 Top 5 -->
        <section v-if="top5.length > 0" class="section">
          <h3 class="section-title">🚨 紧迫度 Top {{ top5.length }}</h3>
          <ul class="urgency-list">
            <li
              v-for="(e, i) in top5"
              :key="e.alert.id"
              class="urgency-item"
              :class="scoreColor(e.score)"
              @click="openTask(e.alert.id)"
              title="点击打开禅道"
            >
              <div class="urgency-rank">{{ i + 1 }}</div>
              <div class="urgency-main">
                <div class="urgency-title">
                  <span>{{ e.alert.title }}</span>
                  <span v-if="e.alert.isTeam" class="urgency-team">👥</span>
                </div>
                <div class="urgency-reasons">{{ e.reasons.join(' · ') }}</div>
              </div>
              <div class="urgency-score" :class="scoreColor(e.score)">{{ e.score }}</div>
            </li>
          </ul>
        </section>

        <!-- 高压日 -->
        <section v-if="store.hoursByDay.some(d => d.hours > 0)" class="section">
          <h3 class="section-title">📆 各日压力分布</h3>
          <ul class="day-list">
            <li
              v-for="d in store.hoursByDay"
              :key="d.date"
              class="day-row"
              :class="{ stacked: d.count >= 2 }"
            >
              <div class="day-date">
                <span>{{ d.date.slice(5) }}</span>
                <span class="day-rel">{{ d.daysFromToday < 0 ? `逾期${-d.daysFromToday}天` : d.daysFromToday === 0 ? '今天' : `+${d.daysFromToday}天` }}</span>
              </div>
              <div class="day-bar-container">
                <div
                  class="day-bar"
                  :style="{ width: Math.min(100, d.hours / Math.max(configStore.hoursPerWorkDay, 1) * 100) + '%' }"
                />
              </div>
              <div class="day-meta">
                <span>{{ d.count }}个</span>
                <span class="day-hours">{{ d.hours.toFixed(1) }}h</span>
              </div>
            </li>
          </ul>
        </section>

        <!-- 智能建议 -->
        <section class="section">
          <h3 class="section-title">💡 给你的建议</h3>
          <ul class="advice-list">
            <li v-for="(s, i) in suggestions" :key="i" class="advice-item">{{ s }}</li>
          </ul>
        </section>
      </div>
    </div>
  </Transition>
</template>

<style scoped>
.risk-panel {
  position: fixed;
  inset: var(--panel-top, 8px) var(--panel-right, 8px) var(--panel-bottom, 90px) var(--panel-left, 8px);
  display: flex;
  flex-direction: column;
  background: linear-gradient(135deg, rgba(20, 30, 56, 0.97), rgba(15, 23, 42, 0.97));
  border: 1px solid rgba(245, 158, 11, 0.25);
  border-radius: 14px;
  box-shadow: 0 12px 32px rgba(0, 0, 0, 0.5);
  overflow: hidden;
  z-index: 55;
  color: rgba(255, 255, 255, 0.92);
}

.panel-header {
  display: flex; align-items: center; justify-content: space-between;
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

.panel-body {
  flex: 1; overflow-y: auto; padding: 10px;
  display: flex; flex-direction: column; gap: 14px;
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
  margin: 4px 0 0;
  font-size: 9.5px;
  color: rgba(255, 255, 255, 0.35);
}

/* 工时预算 */
.budget-row {
  display: flex; align-items: flex-end; gap: 8px;
  padding: 6px 0;
}
.budget-item { display: flex; flex-direction: column; align-items: center; }
.budget-num { font-size: 18px; font-weight: 700; line-height: 1; color: rgba(255, 255, 255, 0.95); }
.budget-label { font-size: 9.5px; color: rgba(255, 255, 255, 0.45); margin-top: 2px; }
.budget-divider { font-size: 14px; color: rgba(255, 255, 255, 0.35); padding-bottom: 4px; }
.ratio-high .budget-num { color: rgba(248, 113, 113, 0.95); }
.ratio-mid .budget-num { color: rgba(250, 204, 21, 0.95); }
.ratio-low .budget-num { color: rgba(16, 185, 129, 0.95); }

.budget-bar {
  position: relative;
  height: 6px;
  background: rgba(255, 255, 255, 0.06);
  border-radius: 3px;
  overflow: hidden;
}
.budget-fill { height: 100%; border-radius: 3px; transition: width 0.4s; }
.fill-low { background: linear-gradient(90deg, rgba(16, 185, 129, 0.8), rgba(16, 185, 129, 0.5)); }
.fill-mid { background: linear-gradient(90deg, rgba(250, 204, 21, 0.8), rgba(245, 158, 11, 0.6)); }
.fill-high { background: linear-gradient(90deg, rgba(248, 113, 113, 0.85), rgba(239, 68, 68, 0.7)); }
.budget-line {
  position: absolute; left: 50%; top: 0; bottom: 0;
  width: 1px; background: rgba(255, 255, 255, 0.3);
}

/* 紧迫度 Top */
.urgency-list { list-style: none; padding: 0; margin: 0; display: flex; flex-direction: column; gap: 4px; }
.urgency-item {
  display: flex; align-items: center; gap: 8px;
  padding: 7px 9px;
  background: rgba(255, 255, 255, 0.03);
  border: 1px solid rgba(255, 255, 255, 0.06);
  border-radius: 8px;
  cursor: pointer;
  transition: background 0.15s, border-color 0.15s, transform 0.1s;
}
.urgency-item:active { transform: scale(0.985); }
.urgency-item.danger { background: rgba(239, 68, 68, 0.08); border-color: rgba(239, 68, 68, 0.2); }
.urgency-item.warn { background: rgba(245, 158, 11, 0.08); border-color: rgba(245, 158, 11, 0.2); }
.urgency-item.soon { background: rgba(59, 130, 246, 0.06); border-color: rgba(59, 130, 246, 0.18); }
.urgency-item.mild { background: rgba(139, 92, 246, 0.05); border-color: rgba(139, 92, 246, 0.14); }

.urgency-rank {
  font-size: 14px;
  font-weight: 700;
  color: rgba(255, 255, 255, 0.4);
  min-width: 18px;
  text-align: center;
}
.urgency-main { flex: 1; min-width: 0; }
.urgency-title {
  font-size: 11.5px;
  color: rgba(255, 255, 255, 0.92);
  overflow: hidden;
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  word-break: break-word;
  line-height: 1.35;
}
.urgency-team {
  display: inline-flex; padding: 0 3px;
  font-size: 9.5px;
  color: rgba(167, 139, 250, 0.95);
  background: rgba(167, 139, 250, 0.12);
  border-radius: 3px;
  margin-left: 3px;
  vertical-align: middle;
}
.urgency-reasons {
  font-size: 9.5px;
  color: rgba(255, 255, 255, 0.5);
  margin-top: 2px;
}
.urgency-score {
  font-size: 14px;
  font-weight: 700;
  padding: 3px 7px;
  border-radius: 6px;
  min-width: 36px;
  text-align: center;
}
.urgency-score.danger { background: rgba(239, 68, 68, 0.2); color: rgba(254, 202, 202, 0.95); }
.urgency-score.warn { background: rgba(245, 158, 11, 0.2); color: rgba(254, 215, 170, 0.95); }
.urgency-score.soon { background: rgba(59, 130, 246, 0.2); color: rgba(191, 219, 254, 0.95); }
.urgency-score.mild { background: rgba(139, 92, 246, 0.15); color: rgba(221, 214, 254, 0.9); }

/* 各日压力 */
.day-list { list-style: none; padding: 0; margin: 0; display: flex; flex-direction: column; gap: 4px; }
.day-row {
  display: flex; align-items: center; gap: 8px;
  padding: 4px 0;
  font-size: 11px;
}
.day-row.stacked .day-date { color: rgba(245, 158, 11, 0.9); }
.day-date {
  display: flex; flex-direction: column;
  min-width: 60px;
  color: rgba(255, 255, 255, 0.7);
}
.day-rel { font-size: 9px; color: rgba(255, 255, 255, 0.4); }
.day-bar-container {
  flex: 1;
  height: 6px;
  background: rgba(255, 255, 255, 0.05);
  border-radius: 3px;
  overflow: hidden;
}
.day-bar {
  height: 100%;
  background: linear-gradient(90deg, rgba(0, 212, 255, 0.7), rgba(0, 212, 255, 0.4));
  border-radius: 3px;
}
.day-row.stacked .day-bar {
  background: linear-gradient(90deg, rgba(245, 158, 11, 0.85), rgba(239, 68, 68, 0.7));
}
.day-meta {
  display: flex; gap: 6px;
  min-width: 56px; justify-content: flex-end;
  color: rgba(255, 255, 255, 0.5);
  font-size: 10px;
}
.day-hours { color: rgba(255, 255, 255, 0.75); font-weight: 600; }

/* 建议 */
.advice-list { list-style: none; padding: 0; margin: 0; display: flex; flex-direction: column; gap: 6px; }
.advice-item {
  padding: 7px 9px;
  background: rgba(0, 212, 255, 0.06);
  border-left: 3px solid rgba(0, 212, 255, 0.5);
  border-radius: 6px;
  font-size: 11px;
  line-height: 1.5;
  color: rgba(255, 255, 255, 0.88);
  word-break: break-word;
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
