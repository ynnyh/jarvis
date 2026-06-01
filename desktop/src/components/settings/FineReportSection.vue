<script setup lang="ts">
import { computed, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useConfigStore } from '../../stores/config'

const store = useConfigStore()

const password = ref('')
const state = ref<'idle' | 'testing' | 'ok' | 'fail'>('idle')
const message = ref('')

interface EffortRecord {
  date: string
  department: string
  employee: string
  dailyTotalHours: number
  itemHours: number
  projectName: string
  system: string
  taskName: string
  workContent: string
}

interface EffortTheme {
  name: string
  hours: number
  task_count: number
  project_count: number
  systems: string[]
  tasks: string[]
  work_items: string[]
}

interface EffortAppendixItem {
  [key: string]: string | number
}

interface EffortReportResponse {
  mode: 'effort' | 'report'
  begin: string
  end: string
  range: string
  summaryText: string
  themes: EffortTheme[]
  appendix: {
    begin: string
    end: string
    total_hours: number
    task_count: number
    project_count: number
    system_count: number
    task_hours: EffortAppendixItem[]
    project_hours: EffortAppendixItem[]
    daily_hours: EffortAppendixItem[]
  }
  records: EffortRecord[]
}

interface ToolResult<T> {
  success: boolean
  data?: T
  error?: string
}

type RangeKey =
  | 'yesterday'
  | 'today'
  | 'thisWeek'
  | 'lastWeek'
  | 'thisMonth'
  | 'thisQuarter'
  | 'last6Months'
  | 'thisYear'

const rangePresets: Array<{ key: RangeKey; label: string }> = [
  { key: 'yesterday', label: '昨天' },
  { key: 'today', label: '今天' },
  { key: 'thisWeek', label: '本周' },
  { key: 'lastWeek', label: '上周' },
  { key: 'thisMonth', label: '本月' },
  { key: 'thisQuarter', label: '本季度' },
  { key: 'last6Months', label: '近半年' },
  { key: 'thisYear', label: '本年' },
]

const beginDate = ref('')
const endDate = ref('')
const activePreset = ref<RangeKey | null>(null)

const queryState = ref<'idle' | 'fetching' | 'ok' | 'fail'>('idle')
const queryMessage = ref('')
const queryMode = ref<'effort' | 'report' | null>(null)
const effortRecords = ref<EffortRecord[]>([])
const reportResult = ref<EffortReportResponse | null>(null)

function fmtDate(date: Date): string {
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')}`
}

function dateRange(key: RangeKey): { begin: string; end: string } {
  const today = new Date()
  let begin = new Date(today)
  let end = new Date(today)

  switch (key) {
    case 'yesterday':
      begin.setDate(today.getDate() - 1)
      end = new Date(begin)
      break
    case 'today':
      break
    case 'thisWeek': {
      const day = today.getDay()
      const diff = day === 0 ? 6 : day - 1
      begin.setDate(today.getDate() - diff)
      break
    }
    case 'lastWeek': {
      const day = today.getDay()
      const diff = day === 0 ? 6 : day - 1
      const thisMonday = new Date(today)
      thisMonday.setDate(today.getDate() - diff)
      const lastSunday = new Date(thisMonday)
      lastSunday.setDate(thisMonday.getDate() - 1)
      const lastMonday = new Date(thisMonday)
      lastMonday.setDate(thisMonday.getDate() - 7)
      begin = lastMonday
      end = lastSunday
      break
    }
    case 'thisMonth':
      begin = new Date(today.getFullYear(), today.getMonth(), 1)
      break
    case 'thisQuarter': {
      const quarterStartMonth = Math.floor(today.getMonth() / 3) * 3
      begin = new Date(today.getFullYear(), quarterStartMonth, 1)
      break
    }
    case 'last6Months':
      begin.setDate(today.getDate() - 182)
      break
    case 'thisYear':
      begin = new Date(today.getFullYear(), 0, 1)
      break
  }

  return { begin: fmtDate(begin), end: fmtDate(end) }
}

function applyPreset(key: RangeKey) {
  const { begin, end } = dateRange(key)
  beginDate.value = begin
  endDate.value = end
  activePreset.value = key
}

applyPreset('today')

const hasRealName = computed(() => !!store.config.fineReport.realName.trim())
const totalItemHours = computed(() =>
  effortRecords.value.reduce((sum, item) => sum + (item.itemHours || 0), 0),
)

const showEffortDetail = ref(false)

interface DailyGroup {
  date: string
  hours: number
  count: number
}

const effortDailyGroups = computed<DailyGroup[]>(() => {
  const map = new Map<string, { hours: number; count: number }>()
  for (const r of effortRecords.value) {
    const d = r.date || '未知日期'
    const entry = map.get(d) || { hours: 0, count: 0 }
    entry.hours += r.itemHours || 0
    entry.count += 1
    map.set(d, entry)
  }
  return Array.from(map.entries())
    .map(([date, { hours, count }]) => ({ date, hours, count }))
    .sort((a, b) => a.date.localeCompare(b.date))
})

const effortWorkDays = computed(() => {
  const days = effortDailyGroups.value.length
  return days > 1
})

const FULL_DAY_HOURS = 8

const topProjects = computed(() => reportResult.value?.appendix.project_hours ?? [])
const topTasks = computed(() => reportResult.value?.appendix.task_hours ?? [])
const dailyHours = computed(() => reportResult.value?.appendix.daily_hours ?? [])

function parseDate(value: string): Date | null {
  if (!value) return null
  const date = new Date(`${value}T00:00:00`)
  return Number.isNaN(date.getTime()) ? null : date
}

function rangeDays(begin: string, end: string): number | null {
  const beginTime = parseDate(begin)
  const endTime = parseDate(end)
  if (!beginTime || !endTime) return null
  const diff = endTime.getTime() - beginTime.getTime()
  return Math.floor(diff / 86400000) + 1
}

function shouldUseReportMode(): boolean {
  if (activePreset.value && ['thisMonth', 'thisQuarter', 'last6Months', 'thisYear'].includes(activePreset.value)) {
    return true
  }
  const days = rangeDays(beginDate.value, endDate.value)
  return days !== null && days > 7
}

function currentModeLabel(): string {
  return shouldUseReportMode() ? '工作汇报' : '工时明细'
}

async function copyText(text: string, successText: string) {
  try {
    await navigator.clipboard.writeText(text)
    queryMessage.value = successText
    queryState.value = 'ok'
  } catch (error) {
    queryMessage.value = `复制失败：${error instanceof Error ? error.message : String(error)}`
    queryState.value = 'fail'
  }
}

async function testConnection() {
  state.value = 'testing'
  message.value = ''
  try {
    const result = await invoke<{ ok: boolean; message: string }>('finereport_test_connection', {
      req: {
        baseUrl: store.config.fineReport.baseUrl,
        account: store.config.fineReport.account,
        password: password.value,
      },
    })
    state.value = result.ok ? 'ok' : 'fail'
    message.value = result.message
  } catch (error) {
    state.value = 'fail'
    message.value = error instanceof Error ? error.message : String(error)
  }
}

async function savePassword() {
  if (!store.config.fineReport.account.trim()) {
    state.value = 'fail'
    message.value = '请先填写帆软账号'
    return
  }
  if (!password.value) {
    state.value = 'fail'
    message.value = '请输入密码'
    return
  }
  try {
    await invoke('finereport_credentials_set', {
      account: store.config.fineReport.account,
      password: password.value,
    })
    state.value = 'ok'
    message.value = '密码已加密保存到系统密钥链'
    password.value = ''
  } catch (error) {
    state.value = 'fail'
    message.value = `保存密码失败：${error instanceof Error ? error.message : String(error)}`
  }
}

async function fetchEfforts() {
  const realName = store.config.fineReport.realName.trim()
  if (!realName) {
    queryState.value = 'fail'
    queryMessage.value = '请先填写“中文姓名”，避免查到他人工时。'
    effortRecords.value = []
    reportResult.value = null
    queryMode.value = null
    return
  }
  if (!beginDate.value || !endDate.value) {
    queryState.value = 'fail'
    queryMessage.value = '请选择开始与结束日期。'
    return
  }
  if (beginDate.value > endDate.value) {
    queryState.value = 'fail'
    queryMessage.value = '开始日期不能晚于结束日期。'
    return
  }

  queryState.value = 'fetching'
  queryMessage.value = ''
  effortRecords.value = []
  reportResult.value = null
  queryMode.value = null
  showEffortDetail.value = false

  const useReport = shouldUseReportMode()
  const toolName = useReport ? 'get_effort_report' : 'get_efforts'

  try {
    const result = await invoke<ToolResult<EffortReportResponse | { begin: string; end: string; count: number; totalHours: number; records: EffortRecord[] }>>('tool_execute', {
      name: toolName,
      input: {
        begin: beginDate.value,
        end: endDate.value,
        realName,
      },
    })

    if (!result.success || !result.data) {
      throw new Error(result.error || '查询失败')
    }

    if (useReport) {
      const data = result.data as EffortReportResponse
      reportResult.value = data
      effortRecords.value = data.records ?? []
      queryMode.value = 'report'
      queryState.value = 'ok'
      queryMessage.value = `${data.begin} ~ ${data.end} 已生成工作汇报，累计 ${data.appendix.total_hours.toFixed(1)}h。`
    } else {
      const data = result.data as { begin: string; end: string; count: number; totalHours: number; records: EffortRecord[] }
      effortRecords.value = data.records ?? []
      queryMode.value = 'effort'
      queryState.value = 'ok'
      queryMessage.value = `${data.begin} ~ ${data.end} 共 ${data.count} 条工时，合计 ${data.totalHours.toFixed(1)}h。`
    }
  } catch (error) {
    queryState.value = 'fail'
    queryMessage.value = error instanceof Error ? error.message : String(error)
  }
}
</script>

<template>
  <section class="settings-section">
    <h3 class="settings-section-title">工时统计</h3>
    <p class="settings-section-hint">
      通过帆软报表查询禅道工时。短周期默认展示工时明细，长周期自动切到工作汇报，桌面端和机器人端保持同一套规则。
    </p>

    <label class="settings-field">
      <span class="settings-field-label">地址</span>
      <input
        v-model="store.config.fineReport.baseUrl"
        class="settings-input"
        type="url"
        placeholder="http://REDACTED_DOMAIN"
      />
    </label>

    <label class="settings-field">
      <span class="settings-field-label">账号</span>
      <input
        v-model="store.config.fineReport.account"
        class="settings-input"
        type="text"
        placeholder="你的帆软用户名"
      />
    </label>

    <label class="settings-field">
      <span class="settings-field-label">密码</span>
      <input
        v-model="password"
        class="settings-input"
        type="password"
        placeholder="留空表示不修改密钥链中的密码"
      />
    </label>

    <label class="settings-field">
      <span class="settings-field-label">中文姓名</span>
      <input
        v-model="store.config.fineReport.realName"
        class="settings-input"
        type="text"
        placeholder="例如 张三，用于按本人过滤工时"
      />
    </label>

    <div class="settings-actions">
      <button class="settings-btn" :disabled="state === 'testing'" @click="testConnection">
        {{ state === 'testing' ? '测试中...' : '测试连接' }}
      </button>
      <button class="settings-btn settings-btn-primary" @click="savePassword">保存密码到密钥链</button>
    </div>

    <p v-if="message" class="settings-msg" :class="`settings-msg-${state}`">{{ message }}</p>
    <p class="settings-section-hint">密码不会写入任何文件，仅保存在系统密钥链中。</p>

    <div class="settings-divider"></div>

    <h4 class="settings-subsection-title">查询与汇报</h4>
    <p class="settings-section-hint">
      今天、昨天、本周默认看明细；本月、本季度、近半年、本年，以及自定义超过 7 天的区间，自动生成完整工作汇报。
    </p>

    <div class="date-row">
      <input
        v-model="beginDate"
        class="settings-input date-input"
        type="date"
        :max="endDate || undefined"
        @change="activePreset = null"
      />
      <span class="date-sep">至</span>
      <input
        v-model="endDate"
        class="settings-input date-input"
        type="date"
        :min="beginDate || undefined"
        @change="activePreset = null"
      />
    </div>

    <div class="range-tabs">
      <button
        v-for="opt in rangePresets"
        :key="opt.key"
        type="button"
        class="range-tab"
        :class="{ active: activePreset === opt.key }"
        @click="applyPreset(opt.key)"
      >
        {{ opt.label }}
      </button>
    </div>

    <div class="mode-hint">
      <span class="mode-badge" :class="shouldUseReportMode() ? 'report' : 'effort'">{{ currentModeLabel() }}</span>
      <span class="mode-text">
        {{ shouldUseReportMode() ? '会生成完整文字版工作汇报和数据附录。' : '会返回逐条工时明细，方便核对当天或当周记录。' }}
      </span>
    </div>

    <div class="settings-actions">
      <button
        class="settings-btn settings-btn-primary"
        :disabled="queryState === 'fetching' || !hasRealName"
        @click="fetchEfforts"
      >
        {{ queryState === 'fetching' ? '查询中...' : '开始查询' }}
      </button>
      <button
        v-if="queryMode === 'report' && reportResult?.summaryText"
        class="settings-btn"
        @click="copyText(reportResult.summaryText, '工作汇报正文已复制')"
      >
        复制正文
      </button>
      <button
        v-if="queryMode === 'report' && reportResult"
        class="settings-btn"
        @click="copyText(JSON.stringify(reportResult.appendix, null, 2), '数据附录已复制')"
      >
        复制附录
      </button>
    </div>

    <p v-if="!hasRealName" class="settings-msg settings-msg-fail">请先填写“中文姓名”再查询。</p>
    <p v-if="queryMessage" class="settings-msg" :class="`settings-msg-${queryState}`">{{ queryMessage }}</p>

    <div v-if="queryMode === 'report' && reportResult" class="report-wrap">
      <section class="report-card">
        <div class="report-head">
          <div>
            <h5 class="report-title">工作汇报</h5>
            <p class="report-range">{{ reportResult.begin }} ~ {{ reportResult.end }}</p>
          </div>
          <div class="report-summary">
            <span>{{ reportResult.appendix.total_hours.toFixed(1) }}h</span>
            <small>总工时</small>
          </div>
        </div>
        <pre class="report-text">{{ reportResult.summaryText }}</pre>
      </section>

      <section class="report-card">
        <h5 class="report-title">重点主题</h5>
        <ul class="theme-list">
          <li v-for="theme in reportResult.themes" :key="theme.name" class="theme-item">
            <div class="theme-row">
              <strong>{{ theme.name }}</strong>
              <span>{{ theme.hours.toFixed(1) }}h</span>
            </div>
            <div v-if="theme.tasks.length" class="theme-meta">任务：{{ theme.tasks.join('；') }}</div>
            <div v-if="theme.work_items.length" class="theme-meta">事项：{{ theme.work_items.join('；') }}</div>
          </li>
        </ul>
      </section>

      <section class="report-grid">
        <div class="report-card compact">
          <h5 class="report-title">项目分布</h5>
          <ul class="appendix-list">
            <li v-for="item in topProjects" :key="`${item.projectName}-${item.hours}`">
              <span>{{ item.projectName || '未命名项目' }}</span>
              <strong>{{ Number(item.hours || 0).toFixed(1) }}h</strong>
            </li>
          </ul>
        </div>

        <div class="report-card compact">
          <h5 class="report-title">任务分布</h5>
          <ul class="appendix-list">
            <li v-for="item in topTasks" :key="`${item.projectName}-${item.taskName}-${item.hours}`">
              <span>{{ item.taskName || '未命名任务' }}</span>
              <strong>{{ Number(item.hours || 0).toFixed(1) }}h</strong>
            </li>
          </ul>
        </div>

        <div class="report-card compact">
          <h5 class="report-title">每日工时</h5>
          <ul class="appendix-list">
            <li v-for="item in dailyHours" :key="`${item.date}-${item.hours}`">
              <span>{{ item.date }}</span>
              <strong>{{ Number(item.hours || 0).toFixed(1) }}h</strong>
            </li>
          </ul>
        </div>
      </section>
    </div>

    <div v-else-if="queryMode === 'effort' && effortRecords.length" class="effort-table-wrap">
      <div v-if="effortWorkDays && !showEffortDetail" class="effort-daily-summary">
        <div class="daily-header">
          <span class="daily-title">每日工时汇总</span>
          <button class="toggle-detail-btn" @click="showEffortDetail = true">
            展开明细
          </button>
        </div>
        <ul class="daily-list">
          <li v-for="g in effortDailyGroups" :key="g.date" class="daily-item" :class="{ low: g.hours < FULL_DAY_HOURS }">
            <span class="daily-date">{{ g.date }}</span>
            <span class="daily-hours">{{ g.hours.toFixed(1) }}h</span>
            <span class="daily-badge" :class="g.hours >= FULL_DAY_HOURS ? 'ok' : 'low'">
              {{ g.hours >= FULL_DAY_HOURS ? '达标' : '不足' }}
            </span>
            <span class="daily-count">{{ g.count }} 条</span>
          </li>
        </ul>
      </div>
      <template v-else>
        <div v-if="effortWorkDays" class="daily-header" style="margin-bottom: 10px;">
          <button class="toggle-detail-btn" @click="showEffortDetail = false">
            收起明细
          </button>
        </div>
        <table class="effort-table">
        <colgroup>
          <col class="col-date" />
          <col class="col-hours" />
          <col class="col-project" />
          <col class="col-task" />
          <col class="col-content" />
        </colgroup>
        <thead>
          <tr>
            <th>日期</th>
            <th class="num">工时</th>
            <th>项目</th>
            <th>任务</th>
            <th>工作内容</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="(record, index) in effortRecords" :key="`${record.date}-${record.taskName}-${index}`">
            <td>{{ record.date }}</td>
            <td class="num">{{ record.itemHours }}</td>
            <td>{{ record.projectName }}</td>
            <td>{{ record.taskName }}</td>
            <td class="content-cell">{{ record.workContent }}</td>
          </tr>
        </tbody>
        <tfoot v-if="effortRecords.length > 1">
          <tr class="total-row">
            <td>合计</td>
            <td class="num">{{ totalItemHours.toFixed(1) }}</td>
            <td colspan="3"></td>
          </tr>
        </tfoot>
      </table>
      </template>
    </div>
  </section>
</template>

<style scoped>
.settings-divider {
  margin: 18px 0 12px;
  border-top: 1px solid rgba(255, 255, 255, 0.08);
}

.settings-subsection-title {
  margin: 0 0 6px;
  font-size: 13px;
  color: rgba(255, 255, 255, 0.85);
}

.date-row {
  display: flex;
  align-items: center;
  gap: 8px;
  margin: 10px 0 4px;
}

.date-input {
  width: 160px;
  flex: 0 0 auto;
}

.date-sep {
  color: rgba(255, 255, 255, 0.6);
  font-size: 12px;
}

.range-tabs {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  margin: 8px 0 12px;
}

.range-tab {
  padding: 3px 10px;
  border: 1px solid rgba(255, 255, 255, 0.12);
  background: rgba(255, 255, 255, 0.03);
  color: rgba(255, 255, 255, 0.7);
  border-radius: 12px;
  font-size: 11px;
  cursor: pointer;
  transition: all 0.15s;
}

.range-tab:hover {
  background: rgba(147, 197, 253, 0.18);
  border-color: rgba(147, 197, 253, 0.5);
  color: rgba(191, 219, 254, 1);
}

.range-tab.active {
  background: rgba(59, 130, 246, 0.28);
  border-color: rgba(147, 197, 253, 0.65);
  color: rgba(219, 234, 254, 1);
  font-weight: 600;
}

.mode-hint {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 12px;
  padding: 10px 12px;
  border-radius: 10px;
  background: rgba(255, 255, 255, 0.03);
  border: 1px solid rgba(255, 255, 255, 0.06);
}

.mode-badge {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 68px;
  height: 24px;
  padding: 0 10px;
  border-radius: 999px;
  font-size: 11px;
  font-weight: 600;
}

.mode-badge.effort {
  background: rgba(59, 130, 246, 0.18);
  color: rgba(191, 219, 254, 0.96);
}

.mode-badge.report {
  background: rgba(168, 85, 247, 0.18);
  color: rgba(233, 213, 255, 0.96);
}

.mode-text {
  font-size: 11px;
  color: rgba(255, 255, 255, 0.6);
  line-height: 1.5;
}

.report-wrap {
  display: flex;
  flex-direction: column;
  gap: 12px;
  margin-top: 12px;
}

.report-card {
  padding: 14px;
  border-radius: 10px;
  background: rgba(255, 255, 255, 0.03);
  border: 1px solid rgba(255, 255, 255, 0.06);
}

.report-card.compact {
  padding: 12px;
}

.report-head {
  display: flex;
  justify-content: space-between;
  align-items: flex-start;
  gap: 12px;
  margin-bottom: 10px;
}

.report-title {
  margin: 0;
  font-size: 13px;
  color: rgba(255, 255, 255, 0.9);
}

.report-range {
  margin: 4px 0 0;
  font-size: 11px;
  color: rgba(255, 255, 255, 0.45);
}

.report-summary {
  display: flex;
  flex-direction: column;
  align-items: flex-end;
}

.report-summary span {
  font-size: 22px;
  font-weight: 700;
  color: rgba(191, 219, 254, 0.96);
  line-height: 1;
}

.report-summary small {
  margin-top: 4px;
  font-size: 10px;
  color: rgba(255, 255, 255, 0.45);
}

.report-text {
  margin: 0;
  white-space: pre-wrap;
  word-break: break-word;
  font-size: 12px;
  line-height: 1.7;
  color: rgba(255, 255, 255, 0.84);
  font-family: inherit;
}

.theme-list,
.appendix-list {
  list-style: none;
  margin: 0;
  padding: 0;
}

.theme-list {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.theme-item {
  padding-top: 10px;
  border-top: 1px solid rgba(255, 255, 255, 0.06);
}

.theme-item:first-child {
  padding-top: 0;
  border-top: none;
}

.theme-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  font-size: 12px;
  color: rgba(255, 255, 255, 0.9);
}

.theme-meta {
  margin-top: 4px;
  font-size: 11px;
  line-height: 1.5;
  color: rgba(255, 255, 255, 0.58);
}

.report-grid {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 12px;
}

.appendix-list li {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 10px;
  padding: 8px 0;
  border-top: 1px solid rgba(255, 255, 255, 0.06);
  font-size: 11px;
  color: rgba(255, 255, 255, 0.72);
}

.appendix-list li:first-child {
  padding-top: 0;
  border-top: none;
}

.appendix-list strong {
  color: rgba(255, 255, 255, 0.92);
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}

.effort-table-wrap {
  margin-top: 12px;
  overflow-x: auto;
}

.effort-table {
  width: 100%;
  min-width: 720px;
  border-collapse: collapse;
  font-size: 12px;
  color: rgba(255, 255, 255, 0.85);
  table-layout: fixed;
}

.effort-table .col-date { width: 96px; }
.effort-table .col-hours { width: 56px; }
.effort-table .col-project { width: 22%; }
.effort-table .col-task { width: 22%; }
.effort-table .col-content { width: auto; }

.effort-table th,
.effort-table td {
  padding: 6px 10px;
  border-bottom: 1px solid rgba(255, 255, 255, 0.06);
  text-align: left;
  vertical-align: top;
  word-break: break-word;
  overflow-wrap: anywhere;
  line-height: 1.5;
}

.effort-table th {
  font-weight: 600;
  color: rgba(147, 197, 253, 0.9);
  background: rgba(255, 255, 255, 0.03);
  white-space: nowrap;
}

.effort-table td.num,
.effort-table th.num {
  text-align: right;
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}

.effort-table .content-cell {
  color: rgba(255, 255, 255, 0.75);
}

.effort-table .total-row td {
  font-weight: 600;
  color: rgba(147, 197, 253, 0.9);
  background: rgba(255, 255, 255, 0.04);
  border-top: 1px solid rgba(255, 255, 255, 0.12);
}

.effort-daily-summary {
  margin-top: 12px;
  padding: 14px;
  border-radius: 10px;
  background: rgba(255, 255, 255, 0.03);
  border: 1px solid rgba(255, 255, 255, 0.06);
}

.daily-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 10px;
}

.daily-title {
  font-size: 13px;
  font-weight: 600;
  color: rgba(255, 255, 255, 0.9);
}

.toggle-detail-btn {
  padding: 3px 10px;
  border: 1px solid rgba(147, 197, 253, 0.4);
  background: rgba(59, 130, 246, 0.12);
  color: rgba(191, 219, 254, 0.9);
  border-radius: 12px;
  font-size: 11px;
  cursor: pointer;
  transition: all 0.15s;
}

.toggle-detail-btn:hover {
  background: rgba(59, 130, 246, 0.25);
  border-color: rgba(147, 197, 253, 0.6);
}

.daily-list {
  list-style: none;
  margin: 0;
  padding: 0;
}

.daily-item {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 8px 0;
  border-top: 1px solid rgba(255, 255, 255, 0.06);
  font-size: 12px;
}

.daily-item:first-child {
  padding-top: 0;
  border-top: none;
}

.daily-date {
  min-width: 96px;
  color: rgba(255, 255, 255, 0.85);
  font-variant-numeric: tabular-nums;
}

.daily-hours {
  font-weight: 600;
  color: rgba(255, 255, 255, 0.92);
  font-variant-numeric: tabular-nums;
  min-width: 48px;
}

.daily-badge {
  display: inline-flex;
  align-items: center;
  padding: 1px 8px;
  border-radius: 999px;
  font-size: 10px;
  font-weight: 600;
}

.daily-badge.ok {
  background: rgba(34, 197, 94, 0.15);
  color: rgba(134, 239, 172, 0.95);
}

.daily-badge.low {
  background: rgba(245, 158, 11, 0.15);
  color: rgba(253, 224, 71, 0.95);
}

.daily-count {
  color: rgba(255, 255, 255, 0.45);
  font-size: 11px;
}

@media (max-width: 1100px) {
  .report-grid {
    grid-template-columns: 1fr;
  }
}
</style>
