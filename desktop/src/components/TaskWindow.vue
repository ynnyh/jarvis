<script setup lang="ts">
import { ref, computed } from 'vue'
import { useAppStore } from '../stores/app'
import { useConfigStore, type CommitsRange } from '../stores/config'
import { useTaskAlerts } from '../composables/useTaskAlerts'
import { useTaskCommits } from '../composables/useTaskCommits'
import TaskItem from './TaskItem.vue'

const store = useAppStore()
const configStore = useConfigStore()
const { refresh } = useTaskAlerts()
const { fetchCommits } = useTaskCommits()

const rangeOptions: Array<{ value: CommitsRange; label: string }> = [
  { value: 'today',     label: '今天' },
  { value: 'yesterday', label: '昨天' },
  { value: 'thisWeek',  label: '本周' },
  { value: 'lastWeek',  label: '上周' },
  { value: 'last7days', label: '近 7 天' },
  { value: 'last30days', label: '近 30 天' },
  { value: 'thisMonth', label: '本月' },
  { value: 'all',       label: '全部' },
]

const refreshing = ref(false)
async function handleRefresh() {
  if (refreshing.value) return
  refreshing.value = true
  try {
    await Promise.all([refresh(), fetchCommits()])
  } finally {
    setTimeout(() => { refreshing.value = false }, 600)
  }
}

const connState = computed<'loading' | 'ok' | 'error'>(() => {
  if (!store.alertsLoaded) return 'loading'
  return store.alertsLastError ? 'error' : 'ok'
})

const connLabel = computed(() => {
  if (connState.value === 'loading') return '连接中…'
  if (connState.value === 'error') return store.alertsLastError || '连接失败'
  return '已连接禅道'
})

const hasAnyAlert = computed(() =>
  store.overdueCount > 0 || store.todayCount > 0 ||
  store.soonCount > 0 || store.upcomingCount > 0
)
</script>

<template>
  <Transition name="panel">
    <div v-if="store.showTaskWindow" class="task-panel pointer-target">
      <header class="panel-header">
        <div class="panel-title">
          <span class="title-icon">🔔</span>
          <span class="title-text">任务提醒</span>
        </div>
        <div class="panel-actions">
          <button
            class="icon-btn"
            :class="{ spinning: refreshing }"
            title="刷新"
            @click="handleRefresh"
          >↻</button>
          <button class="icon-btn" title="关闭" @click="store.showTaskWindow = false">×</button>
        </div>
      </header>

      <div class="conn-bar" :class="`conn-${connState}`">
        <span class="conn-dot" />
        <span class="conn-text">{{ connLabel }}</span>
        <span v-if="connState === 'ok'" class="conn-meta">
          {{ store.taskAlerts.length }} 条
        </span>
        <select
          class="range-select"
          v-model="configStore.config.commitsRange"
          title="commit 关联范围"
        >
          <option v-for="opt in rangeOptions" :key="opt.value" :value="opt.value">
            {{ opt.label }}
          </option>
        </select>
      </div>

      <!-- 堆叠风险横幅：未来 7 天同一天 ≥3 个任务 -->
      <div v-if="store.stackedDays.length > 0" class="stack-banner">
        <span class="stack-icon">⚠️</span>
        <div class="stack-content">
          <div class="stack-title">检测到任务堆叠风险</div>
          <div class="stack-detail">
            <template v-for="(s, i) in store.stackedDays" :key="s.date">
              <span v-if="i > 0">、</span>
              <span class="stack-day">{{ s.date }}</span>
              <span class="stack-num">×{{ s.count }}</span>
            </template>
          </div>
        </div>
      </div>

      <div class="panel-body">
        <!-- 🔥 逾期 -->
        <section v-if="store.overdueTasks.length > 0" class="group group-danger">
          <h3 class="group-title">
            <span>🔥 逾期</span>
            <span class="group-count">{{ store.overdueCount }}</span>
          </h3>
          <ul class="task-list">
            <TaskItem v-for="t in store.overdueTasks" :key="t.id" :task="t" variant="danger" />
          </ul>
        </section>

        <!-- ⏰ 今日截止 -->
        <section v-if="store.todayAlertTasks.length > 0" class="group group-warn">
          <h3 class="group-title">
            <span>⏰ 今日截止</span>
            <span class="group-count">{{ store.todayCount }}</span>
          </h3>
          <ul class="task-list">
            <TaskItem v-for="t in store.todayAlertTasks" :key="t.id" :task="t" variant="warn" />
          </ul>
        </section>

        <!-- ⚡ 3 天内 -->
        <section v-if="store.soonTasks.length > 0" class="group group-soon">
          <h3 class="group-title">
            <span>⚡ 3 天内</span>
            <span class="group-count">{{ store.soonCount }}</span>
          </h3>
          <ul class="task-list">
            <TaskItem v-for="t in store.soonTasks" :key="t.id" :task="t" variant="soon" />
          </ul>
        </section>

        <!-- 📅 一周内 -->
        <section v-if="store.upcomingTasks.length > 0" class="group group-upcoming">
          <h3 class="group-title">
            <span>📅 一周内</span>
            <span class="group-count">{{ store.upcomingCount }}</span>
          </h3>
          <ul class="task-list">
            <TaskItem v-for="t in store.upcomingTasks" :key="t.id" :task="t" variant="upcoming" />
          </ul>
        </section>

        <!-- 空状态 -->
        <div v-if="connState === 'ok' && !hasAnyAlert" class="empty">
          <span class="empty-icon">✓</span>
          <p class="empty-text">7 天内无紧急任务</p>
          <p class="empty-hint">禅道里你的任务都还宽裕</p>
        </div>

        <!-- 加载中 -->
        <div v-else-if="connState === 'loading'" class="empty">
          <span class="empty-icon loading">⟳</span>
          <p class="empty-hint">正在拉取禅道数据…</p>
        </div>

        <!-- 错误 -->
        <div v-else-if="connState === 'error'" class="empty">
          <span class="empty-icon error">!</span>
          <p class="empty-text">禅道连接失败</p>
          <p class="empty-hint">{{ store.alertsLastError }}</p>
          <button class="retry-btn" @click="handleRefresh">重试</button>
        </div>
      </div>
    </div>
  </Transition>
</template>

<style scoped>
.task-panel {
  position: fixed;
  inset: var(--panel-top, 8px) var(--panel-right, 8px) var(--panel-bottom, 90px) var(--panel-left, 8px);
  display: flex;
  flex-direction: column;
  background: linear-gradient(135deg, rgba(20, 30, 56, 0.97), rgba(15, 23, 42, 0.97));
  border: 1px solid rgba(100, 200, 255, 0.16);
  border-radius: 14px;
  box-shadow: 0 12px 32px rgba(0, 0, 0, 0.5);
  overflow: hidden;
  z-index: 50;
  color: rgba(255, 255, 255, 0.92);
}

/* ===== 标题栏 ===== */
.panel-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 8px 10px;
  background: rgba(0, 0, 0, 0.2);
  border-bottom: 1px solid rgba(255, 255, 255, 0.06);
}
.panel-title {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 13px;
  font-weight: 600;
  color: rgba(255, 255, 255, 0.95);
}
.title-icon { font-size: 14px; }
.panel-actions { display: flex; gap: 2px; }
.icon-btn {
  width: 22px;
  height: 22px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  font-size: 14px;
  line-height: 1;
  color: rgba(255, 255, 255, 0.55);
  background: transparent;
  border: none;
  border-radius: 6px;
  cursor: pointer;
  transition: background 0.15s, color 0.15s;
}
.icon-btn:hover {
  color: rgba(255, 255, 255, 0.95);
  background: rgba(255, 255, 255, 0.08);
}
.icon-btn.spinning {
  animation: spin 0.6s linear infinite;
  color: rgba(0, 212, 255, 0.95);
}
@keyframes spin {
  to { transform: rotate(360deg); }
}

/* ===== 连接状态条 ===== */
.conn-bar {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 4px 10px;
  font-size: 10px;
  background: rgba(0, 0, 0, 0.15);
  border-bottom: 1px solid rgba(255, 255, 255, 0.04);
  color: rgba(255, 255, 255, 0.6);
}
.conn-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  flex-shrink: 0;
}
.conn-loading .conn-dot {
  background: rgba(245, 158, 11, 0.9);
  animation: pulse-dot 1s ease-in-out infinite;
}
.conn-ok .conn-dot { background: rgba(16, 185, 129, 0.95); }
.conn-error .conn-dot { background: rgba(239, 68, 68, 0.95); }
.conn-ok { color: rgba(16, 185, 129, 0.85); }
.conn-error { color: rgba(239, 68, 68, 0.9); }
.conn-meta { margin-left: auto; color: rgba(255, 255, 255, 0.45); }
.range-select {
  margin-left: 6px;
  padding: 1px 4px;
  font-size: 10px;
  font-family: inherit;
  color: rgba(255, 255, 255, 0.75);
  background: rgba(255, 255, 255, 0.06);
  border: 1px solid rgba(255, 255, 255, 0.1);
  border-radius: 4px;
  cursor: pointer;
}
.range-select:hover { background: rgba(255, 255, 255, 0.1); }
.range-select:focus { outline: none; border-color: rgba(0, 212, 255, 0.5); }
.range-select option { color: #222; background: #fff; }
/* conn-meta 没有时 select 自己往右 */
.conn-bar > .range-select:not(:last-child) { margin-left: 6px; }
.conn-bar > .conn-meta + .range-select { margin-left: 6px; }
.conn-bar > .conn-text + .range-select { margin-left: auto; }
@keyframes pulse-dot {
  0%, 100% { opacity: 0.5; transform: scale(0.85); }
  50% { opacity: 1; transform: scale(1.15); }
}

/* ===== 主体 ===== */
.panel-body {
  flex: 1;
  overflow-y: auto;
  padding: 8px;
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.group {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.group-title {
  display: flex;
  align-items: center;
  gap: 6px;
  margin: 0 2px 2px;
  font-size: 11px;
  font-weight: 600;
}
.group-count {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 18px;
  padding: 0 5px;
  height: 16px;
  font-size: 10px;
  font-weight: 700;
  border-radius: 8px;
  background: rgba(255, 255, 255, 0.08);
}
.group-danger .group-title { color: rgba(248, 113, 113, 0.95); }
.group-warn .group-title { color: rgba(250, 204, 21, 0.95); }
.group-soon .group-title { color: rgba(96, 165, 250, 0.95); }
.group-upcoming .group-title { color: rgba(167, 139, 250, 0.9); }

.task-list { list-style: none; padding: 0; margin: 0; display: flex; flex-direction: column; gap: 4px; }
.task-item {
  padding: 7px 9px;
  border-radius: 8px;
  border: 1px solid transparent;
  background: rgba(255, 255, 255, 0.03);
  display: flex;
  flex-direction: column;
  gap: 3px;
  cursor: pointer;
  transition: background 0.15s, border-color 0.15s, transform 0.1s;
}
.task-item:active { transform: scale(0.985); }
.task-item.danger {
  background: rgba(239, 68, 68, 0.08);
  border-color: rgba(239, 68, 68, 0.18);
}
.task-item.danger:hover { background: rgba(239, 68, 68, 0.14); }
.task-item.warn {
  background: rgba(245, 158, 11, 0.08);
  border-color: rgba(245, 158, 11, 0.18);
}
.task-item.warn:hover { background: rgba(245, 158, 11, 0.14); }
.task-item.soon {
  background: rgba(59, 130, 246, 0.06);
  border-color: rgba(59, 130, 246, 0.18);
}
.task-item.soon:hover { background: rgba(59, 130, 246, 0.12); }
.task-item.upcoming {
  background: rgba(139, 92, 246, 0.05);
  border-color: rgba(139, 92, 246, 0.14);
}
.task-item.upcoming:hover { background: rgba(139, 92, 246, 0.1); }

.task-row1 {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 6px;
}
.task-title {
  font-size: 11.5px;
  line-height: 1.4;
  color: rgba(255, 255, 255, 0.92);
  overflow: hidden;
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  word-break: break-word;
}
.task-badge {
  flex-shrink: 0;
  font-size: 9.5px;
  padding: 1px 5px;
  border-radius: 4px;
  white-space: nowrap;
}
.badge-danger { background: rgba(239, 68, 68, 0.25); color: rgba(254, 202, 202, 0.95); }
.badge-warn { background: rgba(245, 158, 11, 0.25); color: rgba(254, 215, 170, 0.95); }
.badge-soon { background: rgba(59, 130, 246, 0.25); color: rgba(191, 219, 254, 0.95); }
.badge-upcoming { background: rgba(139, 92, 246, 0.2); color: rgba(221, 214, 254, 0.92); }

.task-row2 {
  display: flex;
  align-items: center;
  gap: 4px;
  font-size: 10px;
  color: rgba(255, 255, 255, 0.45);
}
.muted { color: rgba(255, 255, 255, 0.35); }

.team-tag {
  display: inline-flex;
  align-items: center;
  padding: 0 4px;
  font-size: 9.5px;
  color: rgba(167, 139, 250, 0.95);
  background: rgba(167, 139, 250, 0.12);
  border-radius: 4px;
  margin-left: 2px;
}

/* ===== 堆叠风险横幅 ===== */
.stack-banner {
  display: flex;
  align-items: flex-start;
  gap: 8px;
  margin: 6px 8px 0;
  padding: 6px 10px;
  background: linear-gradient(135deg, rgba(245, 158, 11, 0.15), rgba(239, 68, 68, 0.12));
  border: 1px solid rgba(245, 158, 11, 0.3);
  border-radius: 8px;
  font-size: 10.5px;
}
.stack-icon { font-size: 14px; line-height: 1.4; flex-shrink: 0; }
.stack-content { flex: 1; min-width: 0; }
.stack-title {
  font-size: 11px;
  font-weight: 600;
  color: rgba(254, 215, 170, 0.95);
  margin-bottom: 2px;
}
.stack-detail {
  color: rgba(255, 255, 255, 0.65);
  font-size: 10px;
  word-break: break-word;
}
.stack-day { color: rgba(254, 215, 170, 0.85); }
.stack-num {
  margin-left: 2px;
  font-weight: 600;
  color: rgba(248, 113, 113, 0.95);
}

/* ===== 空状态 / 加载态 / 错误态 ===== */
.empty {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  text-align: center;
  padding: 20px;
  color: rgba(255, 255, 255, 0.5);
}
.empty-icon {
  font-size: 28px;
  margin-bottom: 6px;
  width: 44px;
  height: 44px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 50%;
  background: rgba(16, 185, 129, 0.12);
  color: rgba(16, 185, 129, 0.95);
  font-weight: 600;
}
.empty-icon.loading {
  background: rgba(245, 158, 11, 0.12);
  color: rgba(245, 158, 11, 0.95);
  animation: spin 1s linear infinite;
}
.empty-icon.error {
  background: rgba(239, 68, 68, 0.12);
  color: rgba(239, 68, 68, 0.95);
}
.empty-text {
  font-size: 12px;
  color: rgba(255, 255, 255, 0.85);
  margin: 4px 0 2px;
}
.empty-hint {
  font-size: 10.5px;
  color: rgba(255, 255, 255, 0.4);
  margin: 0;
  max-width: 200px;
}
.retry-btn {
  margin-top: 10px;
  padding: 4px 12px;
  font-size: 11px;
  color: rgba(255, 255, 255, 0.85);
  background: rgba(255, 255, 255, 0.08);
  border: 1px solid rgba(255, 255, 255, 0.12);
  border-radius: 6px;
  cursor: pointer;
}
.retry-btn:hover { background: rgba(255, 255, 255, 0.14); }

/* ===== 进出动效 ===== */
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
