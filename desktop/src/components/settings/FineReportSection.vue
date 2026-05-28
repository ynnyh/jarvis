<script setup lang="ts">
// 工时统计 section：地址 / 账号 / 密码 + 测试连接 + 查询工时明细。
//
// 同禅道：密码不入磁盘，存 OS keychain；密码框留空提交时，test 走 keychain 已存值。

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
interface EffortFetchResult {
  cid: string
  sessionId: string
  records: EffortRecord[]
  summaryHtml: string
  detailHtml: string
}

type RangeKey = 'yesterday' | 'today' | 'week' | 'month' | 'year'
const rangePresets: { key: RangeKey; label: string }[] = [
  { key: 'yesterday', label: '昨天' },
  { key: 'today', label: '今日' },
  { key: 'week', label: '本周' },
  { key: 'month', label: '本月' },
  { key: 'year', label: '本年' },
]

const beginDate = ref('')
const endDate = ref('')

const effortState = ref<'idle' | 'fetching' | 'ok' | 'fail'>('idle')
const effortMsg = ref('')
const effortResult = ref<EffortFetchResult | null>(null)

function fmtDate(d: Date): string {
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`
}

/** 按 RangeKey 计算 [begin, end]。 */
function dateRange(key: RangeKey): { begin: string; end: string } {
  const today = new Date()
  let begin: Date
  let end: Date = today
  switch (key) {
    case 'yesterday': {
      const y = new Date(today)
      y.setDate(today.getDate() - 1)
      begin = y
      end = y
      break
    }
    case 'today':
      begin = today
      break
    case 'week': {
      const day = today.getDay()
      const diff = day === 0 ? 6 : day - 1
      begin = new Date(today)
      begin.setDate(today.getDate() - diff)
      break
    }
    case 'month':
      begin = new Date(today.getFullYear(), today.getMonth(), 1)
      break
    case 'year':
      begin = new Date(today.getFullYear(), 0, 1)
      break
  }
  return { begin: fmtDate(begin), end: fmtDate(end) }
}

function applyPreset(key: RangeKey) {
  const { begin, end } = dateRange(key)
  beginDate.value = begin
  endDate.value = end
}

// 默认今日
applyPreset('today')

const hasRealName = computed(() => !!store.config.fineReport.realName?.trim())

const totalItemHours = computed(() =>
  effortResult.value?.records.reduce((sum, x) => sum + (x.itemHours || 0), 0) ?? 0
)

async function testConnection() {
  state.value = 'testing'
  message.value = ''
  try {
    const r = await invoke<{ ok: boolean; message: string }>('finereport_test_connection', {
      req: {
        baseUrl: store.config.fineReport.baseUrl,
        account: store.config.fineReport.account,
        password: password.value,
      },
    })
    state.value = r.ok ? 'ok' : 'fail'
    message.value = r.message
  } catch (e: any) {
    state.value = 'fail'
    message.value = String(e?.message ?? e)
  }
}

async function savePassword() {
  if (!store.config.fineReport.account.trim()) {
    message.value = '请先填写帆软账号'
    state.value = 'fail'
    return
  }
  if (!password.value) {
    message.value = '请输入密码'
    state.value = 'fail'
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
  } catch (e: any) {
    state.value = 'fail'
    message.value = '保存密码失败：' + String(e?.message ?? e)
  }
}

async function fetchEfforts() {
  const realName = store.config.fineReport.realName?.trim() ?? ''
  if (!realName) {
    effortState.value = 'fail'
    effortMsg.value = '请先在上方填写"中文姓名"。未填姓名时不查询，避免拉到他人数据。'
    effortResult.value = null
    return
  }
  const begin = beginDate.value
  const end = endDate.value
  if (!begin || !end) {
    effortState.value = 'fail'
    effortMsg.value = '请选择开始与结束日期。'
    return
  }
  if (begin > end) {
    effortState.value = 'fail'
    effortMsg.value = '开始日期不能晚于结束日期。'
    return
  }
  effortState.value = 'fetching'
  effortMsg.value = ''
  effortResult.value = null
  try {
    const r = await invoke<EffortFetchResult>('finereport_get_efforts', {
      begin,
      end,
      realName,
    })
    effortResult.value = r
    effortState.value = 'ok'
    effortMsg.value = `${begin} ~ ${end}：${realName} 共 ${r.records.length} 条明细，合计 ${
      r.records.reduce((s, x) => s + (x.itemHours || 0), 0)
    } 工时`
  } catch (e: any) {
    effortState.value = 'fail'
    effortMsg.value = String(e?.message ?? e)
  }
}
</script>

<template>
  <section class="settings-section">
    <h3 class="settings-section-title">工时统计</h3>
    <p class="settings-section-hint">
      通过帆软报表查询禅道工时。地址和账号一般同禅道，首次使用请先测试连接并保存密码。
    </p>
    <label class="settings-field">
      <span class="settings-field-label">地址</span>
      <input class="settings-input" type="url" placeholder="http://REDACTED_DOMAIN"
        v-model="store.config.fineReport.baseUrl" />
    </label>
    <label class="settings-field">
      <span class="settings-field-label">账号</span>
      <input class="settings-input" type="text" placeholder="你的帆软用户名"
        v-model="store.config.fineReport.account" />
    </label>
    <label class="settings-field">
      <span class="settings-field-label">密码</span>
      <input class="settings-input" type="password" placeholder="留空表示不修改密钥链中的密码"
        v-model="password" />
    </label>
    <label class="settings-field">
      <span class="settings-field-label">中文姓名</span>
      <input class="settings-input" type="text" placeholder="例如 张三，必填——用于按本人过滤工时"
        v-model="store.config.fineReport.realName" />
    </label>
    <div class="settings-actions">
      <button class="settings-btn" :disabled="state === 'testing'" @click="testConnection">
        {{ state === 'testing' ? '测试中…' : '测试连接' }}
      </button>
      <button class="settings-btn settings-btn-primary" @click="savePassword">
        保存密码到密钥链
      </button>
    </div>
    <p v-if="message" class="settings-msg" :class="`settings-msg-${state}`">{{ message }}</p>
    <p class="settings-section-hint">密码不会写入任何文件，仅保存在系统密钥链中</p>

    <div class="settings-divider"></div>

    <h4 class="settings-subsection-title">工时查询</h4>
    <p class="settings-section-hint">
      未填中文姓名时不会发起查询，避免拉到他人数据。
    </p>

    <div class="date-row">
      <input class="settings-input date-input" type="date" v-model="beginDate" :max="endDate || undefined" />
      <span class="date-sep">至</span>
      <input class="settings-input date-input" type="date" v-model="endDate" :min="beginDate || undefined" />
    </div>
    <div class="range-tabs">
      <button v-for="opt in rangePresets" :key="opt.key"
        class="range-tab" type="button"
        @click="applyPreset(opt.key)">
        {{ opt.label }}
      </button>
    </div>

    <div class="settings-actions">
      <button class="settings-btn settings-btn-primary"
        :disabled="effortState === 'fetching' || !hasRealName"
        @click="fetchEfforts">
        {{ effortState === 'fetching' ? '拉取中…' : '查询' }}
      </button>
    </div>
    <p v-if="!hasRealName" class="settings-msg settings-msg-fail">
      请先填写"中文姓名"再查询。
    </p>
    <p v-if="effortMsg" class="settings-msg" :class="`settings-msg-${effortState}`">{{ effortMsg }}</p>

    <div v-if="effortResult && effortResult.records.length" class="effort-table-wrap">
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
          <tr v-for="(r, i) in effortResult.records" :key="i">
            <td>{{ r.date }}</td>
            <td class="num">{{ r.itemHours }}</td>
            <td>{{ r.projectName }}</td>
            <td>{{ r.taskName }}</td>
            <td class="content-cell">{{ r.workContent }}</td>
          </tr>
        </tbody>
        <tfoot v-if="effortResult.records.length > 1">
          <tr class="total-row">
            <td>合计</td>
            <td class="num">{{ totalItemHours }}</td>
            <td colspan="3"></td>
          </tr>
        </tfoot>
      </table>
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
</style>
