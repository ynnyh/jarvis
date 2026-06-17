<script setup lang="ts">
import { computed, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useConfigStore } from '../../stores/config'
import FineReportResults from './FineReportResults.vue'

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

const showEffortDetail = ref(false)

const expandSummary = ref(false)
const expandThemes = ref(false)
const expandProjects = ref(false)
const expandTasks = ref(false)

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
    queryMessage.value = '请先填写"中文姓名"，避免查到他人工时。'
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
  expandSummary.value = false
  expandThemes.value = false
  expandProjects.value = false
  expandTasks.value = false

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
        placeholder="https://your-fine-report.example.com"
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

    <p v-if="!hasRealName" class="settings-msg settings-msg-fail">请先填写"中文姓名"再查询。</p>
    <p v-if="queryMessage" class="settings-msg" :class="`settings-msg-${queryState}`">{{ queryMessage }}</p>

    <FineReportResults
      :query-mode="queryMode"
      :effort-records="effortRecords"
      :report-result="reportResult"
      v-model:show-effort-detail="showEffortDetail"
      v-model:expand-summary="expandSummary"
      v-model:expand-themes="expandThemes"
      v-model:expand-projects="expandProjects"
      v-model:expand-tasks="expandTasks"
      @copy-text="copyText"
    />
  </section>
</template>

<style scoped>
.settings-divider {
  margin: 18px 0 12px;
  border-top: var(--divider);
}

.settings-subsection-title {
  margin: 0 0 6px;
  font-size: 13px;
  color: var(--text);
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
  color: var(--text-ghost);
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
  border: var(--input-border);
  background: var(--surface);
  color: var(--text-ghost);
  border-radius: 12px;
  font-size: 11px;
  cursor: pointer;
  transition: all 0.15s;
}

.range-tab:hover {
  background: color-mix(in srgb, var(--accent) 18%, transparent);
  border-color: color-mix(in srgb, var(--accent) 50%, transparent);
  color: var(--accent-text);
}

.range-tab.active {
  background: color-mix(in srgb, var(--accent) 28%, transparent);
  border-color: color-mix(in srgb, var(--accent) 65%, transparent);
  color: var(--accent-text);
  font-weight: 600;
}

.mode-hint {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 12px;
  padding: 10px 12px;
  border-radius: 10px;
  background: var(--surface);
  border: var(--divider);
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
  background: color-mix(in srgb, var(--accent) 18%, transparent);
  color: var(--accent-text);
}

.mode-badge.report {
  background: color-mix(in srgb, var(--purple) 18%, transparent);
  color: var(--purple-text);
}

.mode-text {
  font-size: 11px;
  color: var(--text-ghost);
  line-height: 1.5;
}
</style>
