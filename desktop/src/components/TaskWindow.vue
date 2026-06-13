<script setup lang="ts">
import { ref, computed } from 'vue'
import { useAppStore } from '../stores/app'
import { useConfigStore, type CommitsRange } from '../stores/config'
import { useTaskAlerts } from '../composables/useTaskAlerts'
import { useTaskCommits } from '../composables/useTaskCommits'
import TaskItem from './TaskItem.vue'
import CustomDropdown from './ui/CustomDropdown.vue'

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
        <CustomDropdown
          v-model="configStore.config.commitsRange"
          :options="rangeOptions"
          class="range-dropdown"
          title="commit 关联范围"
        />
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
  background: var(--popup-bg);
  backdrop-filter: blur(20px);
  -webkit-backdrop-filter: blur(20px);
  border: var(--panel-border);
  border-radius: var(--radius-lg);
  box-shadow: var(--panel-shadow);
  overflow: hidden;
  z-index: 50;
  color: var(--text);
}

/* ===== 标题栏 ===== */
.panel-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 8px 10px;
  background: var(--panel-header-bg);
  border-bottom: var(--panel-header-border);
}
.panel-title {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 13px;
  font-weight: 600;
  color: var(--text);
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
  color: var(--text-dim);
  background: transparent;
  border: none;
  border-radius: var(--radius-control);
  cursor: pointer;
  transition: background 0.15s, color 0.15s;
}
.icon-btn:hover {
  color: var(--text);
  background: var(--surface-item-hover);
}
.icon-btn.spinning {
  animation: spin 0.6s linear infinite;
  color: var(--accent-text);
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
  background: var(--panel-header-bg);
  border-bottom: var(--divider-soft);
  color: var(--text-dim);
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
.conn-ok { color: var(--green-text); }
.conn-error { color: var(--red-text); }
.conn-meta { margin-left: auto; color: var(--text-muted); font-family: var(--font-display); font-variant-numeric: var(--num-font-variant); }
.range-dropdown {
  margin-left: 6px;
  width: auto !important;
  min-width: 80px;
  flex-shrink: 0;
}
.range-dropdown :deep(.custom-dropdown) {
  width: auto !important;
}
.range-dropdown :deep(.dropdown-trigger) {
  padding: 2px 6px;
  font-size: 10px;
  color: var(--text-ghost);
  background: var(--input-bg);
  border: var(--input-border);
  border-radius: var(--radius-sm);
}
.range-dropdown :deep(.dropdown-trigger:hover) {
  background: var(--surface-item-active);
}
/* conn-meta 没有时 dropdown 自己往右 */
.conn-bar > .range-dropdown:not(:last-child) { margin-left: 6px; }
.conn-bar > .conn-meta + .range-dropdown { margin-left: 6px; }
.conn-bar > .conn-text + .range-dropdown { margin-left: auto; }
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
  border-radius: var(--radius-sm);
  background: var(--surface-item-hover);
  font-family: var(--font-display);
  font-variant-numeric: var(--num-font-variant);
}
.group-danger .group-title { color: var(--red-text); }
.group-warn .group-title { color: var(--yellow-text); }
.group-soon .group-title { color: var(--blue-text); }
.group-upcoming .group-title { color: var(--purple-text); }

.task-list { list-style: none; padding: 0; margin: 0; display: flex; flex-direction: column; gap: 4px; }
.task-item {
  padding: 7px 9px;
  border-radius: var(--radius-md);
  border: 1px solid transparent;
  background: var(--surface);
  display: flex;
  flex-direction: column;
  gap: 3px;
  cursor: pointer;
  transition: background 0.15s, border-color 0.15s, transform 0.1s;
}
.task-item:active { transform: scale(0.985); }
.task-item:hover { box-shadow: var(--shadow-1); }
.task-item.danger {
  background: var(--red-bg);
  border-color: var(--red-border);
}
.task-item.danger:hover { background: var(--red-bg); }
.task-item.warn {
  background: var(--yellow-bg);
  border-color: var(--yellow-border);
}
.task-item.warn:hover { background: var(--yellow-bg); }
.task-item.soon {
  background: var(--blue-bg);
  border-color: var(--blue-border);
}
.task-item.soon:hover { background: var(--blue-bg); }
.task-item.upcoming {
  background: var(--purple-bg);
  border-color: var(--purple-border);
}
.task-item.upcoming:hover { background: var(--purple-bg); }

.task-row1 {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 6px;
}
.task-title {
  font-size: 11.5px;
  line-height: 1.4;
  color: var(--text);
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
  border-radius: var(--radius-sm);
  white-space: nowrap;
}
.badge-danger { background: var(--red-bg-strong); color: var(--red-text-light); }
.badge-warn { background: var(--yellow-bg-strong); color: var(--yellow-text-light); }
.badge-soon { background: var(--blue-bg-strong); color: var(--blue-text-light); }
.badge-upcoming { background: var(--purple-bg-strong); color: var(--purple-text-light); }

.task-row2 {
  display: flex;
  align-items: center;
  gap: 4px;
  font-size: 10px;
  color: var(--text-muted);
}
.muted { color: var(--text-faint); }

.team-tag {
  display: inline-flex;
  align-items: center;
  padding: 0 4px;
  font-size: 9.5px;
  color: var(--purple-text);
  background: var(--purple-bg);
  border-radius: var(--radius-sm);
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
  border-radius: var(--radius-md);
  font-size: 10.5px;
}
.stack-icon { font-size: 14px; line-height: 1.4; flex-shrink: 0; }
.stack-content { flex: 1; min-width: 0; }
.stack-title {
  font-size: 11px;
  font-weight: 600;
  color: var(--yellow-text-light);
  margin-bottom: 2px;
}
.stack-detail {
  color: var(--text-dim);
  font-size: 10px;
  word-break: break-word;
}
.stack-day { color: var(--yellow-text-light); }
.stack-num {
  margin-left: 2px;
  font-weight: 600;
  color: var(--red-text);
  font-family: var(--font-display);
  font-variant-numeric: var(--num-font-variant);
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
  color: var(--text-muted);
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
  background: var(--green-bg);
  color: var(--green-text);
  font-weight: 600;
}
.empty-icon.loading {
  background: var(--yellow-bg);
  color: var(--yellow-text);
  animation: spin 1s linear infinite;
}
.empty-icon.error {
  background: var(--red-bg);
  color: var(--red-text);
}
.empty-text {
  font-size: 12px;
  color: var(--text-ghost);
  margin: 4px 0 2px;
}
.empty-hint {
  font-size: 10.5px;
  color: var(--text-muted);
  margin: 0;
  max-width: 200px;
}
.retry-btn {
  margin-top: 10px;
  padding: 4px 12px;
  font-size: 11px;
  color: var(--text-ghost);
  background: var(--surface-item-hover);
  border: 1px solid var(--border);
  border-radius: var(--radius-control);
  cursor: pointer;
}
.retry-btn:hover { background: var(--surface-item-active); }

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
