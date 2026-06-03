<script setup lang="ts">
// 成本分析独立大窗口：项目搜索 + 概览卡片 + 条形图 + 成本占比 + 明细表（内联时薪编辑）。
// 从 in-app overlay 升级为独立 Tauri 窗口，走 cost_open / cost_close 命令。
import { ref, computed, onMounted } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { invoke } from '@tauri-apps/api/core'

interface MemberCost {
  account: string
  displayName: string
  hours: number
  hourlyRate: number
  cost: number
  taskCount: number
  normalHours?: number
  overtimeHours?: number
  normalCost?: number
  overtimeCost?: number
}

interface CostSummaryResult {
  projectName: string
  members: MemberCost[]
  totalHours: number
  totalCost: number
  totalNormalHours?: number
  totalOvertimeHours?: number
}

interface ProjectInfo {
  id: number | string
  name: string
}

// ===== 状态 =====
const projects = ref<ProjectInfo[]>([])
const projectSearch = ref('')
const showProjectDropdown = ref(false)
const loading = ref(false)
const includeOvertime = ref(false)
const includeResigned = ref(false)
const error = ref<string | null>(null)
const result = ref<CostSummaryResult | null>(null)

// 时间范围：快捷档 key + 自定义起止。默认本月，全周期档已移除（数据量太大会让帆软超时）。
type RangePreset = 'thisMonth' | 'thisQuarter' | 'halfYear' | 'thisYear' | 'custom'
// 默认本月：全周期数据量太大会让帆软查询超时（error sending request）。
const rangePreset = ref<RangePreset>('thisMonth')
const customStart = ref('')
const customEnd = ref('')

const RANGE_PRESETS: Array<{ key: RangePreset; label: string }> = [
  { key: 'thisMonth', label: '本月' },
  { key: 'thisQuarter', label: '本季' },
  { key: 'halfYear', label: '近半年' },
  { key: 'thisYear', label: '今年' },
  { key: 'custom', label: '自定义' },
]

function ymd(d: Date): string {
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`
}

/** 根据当前 preset / 自定义输入算出 [start, end]，空串表示该侧不限。 */
const effectiveRange = computed<{ start: string; end: string }>(() => {
  const now = new Date()
  const y = now.getFullYear()
  const m = now.getMonth()
  switch (rangePreset.value) {
    case 'thisQuarter': {
      const qStart = Math.floor(m / 3) * 3
      return { start: ymd(new Date(y, qStart, 1)), end: ymd(new Date(y, qStart + 3, 0)) }
    }
    case 'halfYear': {
      const start = new Date(now)
      start.setMonth(start.getMonth() - 6)
      return { start: ymd(start), end: ymd(now) }
    }
    case 'thisYear':
      return { start: ymd(new Date(y, 0, 1)), end: ymd(new Date(y, 11, 31)) }
    case 'custom':
      return { start: customStart.value, end: customEnd.value }
    case 'thisMonth':
    default:
      return { start: ymd(new Date(y, m, 1)), end: ymd(new Date(y, m + 1, 0)) }
  }
})

const rangeHint = computed(() => {
  const { start, end } = effectiveRange.value
  if (start && end) return `${start} ~ ${end}`
  if (start) return `${start} 起`
  if (end) return `截至 ${end}`
  return '请选择日期'
})

// 时薪编辑：account → hourlyRate
const rates = ref<Record<string, number>>({})
// 统一时薪快捷输入
const unifiedRate = ref<number | null>(null)

let saveTimer: ReturnType<typeof setTimeout> | null = null

const filteredProjects = computed(() => {
  const q = projectSearch.value.trim().toLowerCase()
  if (!q) return projects.value
  return projects.value.filter(p => p.name.toLowerCase().includes(q))
})

const memberCount = computed(() => result.value?.members.length ?? 0)
const hasOvertime = computed(() => includeOvertime.value && result.value?.totalOvertimeHours != null)

const maxHours = computed(() => {
  if (!result.value) return 1
  return Math.max(...result.value.members.map(m => m.hours), 1)
})

const avgCost = computed(() => {
  if (memberCount.value === 0) return 0
  return (result.value?.totalCost ?? 0) / memberCount.value
})

// 成本占比颜色板
const COST_COLORS = [
  'rgba(0, 212, 255, 0.8)',
  'rgba(245, 158, 11, 0.8)',
  'rgba(16, 185, 129, 0.8)',
  'rgba(236, 72, 153, 0.8)',
  'rgba(139, 92, 246, 0.8)',
  'rgba(34, 211, 238, 0.8)',
  'rgba(251, 146, 60, 0.8)',
  'rgba(163, 230, 53, 0.8)',
]

// ===== 数据加载 =====
async function loadProjects() {
  try {
    projects.value = await invoke<ProjectInfo[]>('list_projects')
  } catch (e) {
    error.value = `加载项目列表失败: ${e instanceof Error ? e.message : String(e)}`
  }
}

async function loadRates() {
  try {
    const loaded = await invoke<Record<string, { hourlyRate: number; displayName: string }>>('cost_rates_load')
    const rateMap: Record<string, number> = {}
    for (const [k, v] of Object.entries(loaded)) {
      rateMap[k] = v.hourlyRate
    }
    rates.value = rateMap
  } catch {
    rates.value = {}
  }
}

async function runQuery() {
  if (!projectSearch.value) return
  loading.value = true
  error.value = null
  result.value = null
  try {
    const { start, end } = effectiveRange.value
    result.value = await invoke<CostSummaryResult>('project_cost_summary', {
      projectName: projectSearch.value,
      include_overtime: true,
      startDate: start || null,
      endDate: end || null,
      includeResigned: includeResigned.value,
    })
    // 把结果中的时薪同步到编辑态
    if (result.value) {
      for (const m of result.value.members) {
        if (rates.value[m.account] === undefined) {
          rates.value[m.account] = m.hourlyRate
        }
      }
    }
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e)
  } finally {
    loading.value = false
  }
}

// ===== 内联编辑 =====

/** 饼图用：conic-gradient 数据。有成本按成本占比，没配时薪（totalCost=0）回退按工时占比。 */
const pieSegments = computed(() => {
  if (!result.value) return { gradient: '', items: [] as Array<{ account: string; value: number; pct: number; color: string }>, metric: 'cost' as 'cost' | 'hours' }
  const useCost = result.value.totalCost > 0
  const total = useCost ? result.value.totalCost : result.value.totalHours
  if (total <= 0) return { gradient: '', items: [], metric: useCost ? 'cost' : 'hours' }
  const items = result.value.members
    .map((m, i) => {
      const value = useCost ? m.cost : m.hours
      return {
        account: m.account,
        value,
        pct: (value / total) * 100,
        color: COST_COLORS[i % COST_COLORS.length],
      }
    })
    .filter(s => s.value > 0)
    .sort((a, b) => b.value - a.value)

  let acc = 0
  const stops: string[] = []
  for (const item of items) {
    stops.push(`${item.color} ${acc}% ${acc + item.pct}%`)
    acc += item.pct
  }
  return { gradient: `conic-gradient(${stops.join(', ')})`, items, metric: (useCost ? 'cost' : 'hours') as 'cost' | 'hours' }
})

/** 内联编辑时薪后调用 */
function onRateChange(account: string, val: string) {
  const num = parseFloat(val)
  if (isNaN(num) || num < 0) return
  rates.value[account] = num
  // 同步更新结果中的时薪和成本
  if (result.value) {
    const m = result.value.members.find(m => m.account === account)
    if (m) {
      m.hourlyRate = num
      m.cost = m.hours * num
      if (m.normalHours != null) m.normalCost = m.normalHours * num
      if (m.overtimeHours != null) m.overtimeCost = m.overtimeHours * num
      recalcTotals()
    }
  }
  debouncedSave()
}

/** 应用统一时薪 */
function applyUnifiedRate() {
  if (unifiedRate.value === null || unifiedRate.value < 0) return
  const r = unifiedRate.value
  for (const key of Object.keys(rates.value)) {
    rates.value[key] = r
  }
  // 同步结果
  if (result.value) {
    for (const m of result.value.members) {
      m.hourlyRate = r
      m.cost = m.hours * r
      if (m.normalHours != null) m.normalCost = m.normalHours * r
      if (m.overtimeHours != null) m.overtimeCost = m.overtimeHours * r
    }
    recalcTotals()
  }
  debouncedSave()
}

function recalcTotals() {
  if (!result.value) return
  result.value.totalCost = result.value.members.reduce((s, m) => s + m.cost, 0)
}

function debouncedSave() {
  if (saveTimer) clearTimeout(saveTimer)
  saveTimer = setTimeout(saveRates, 600)
}

async function saveRates() {
  // 构造完整的 RatesMap 格式 { account: { hourlyRate, displayName } }。
  // 数据源是帆软，account 本身就是员工中文名，displayName 直接等于 account。
  const map: Record<string, { hourlyRate: number; displayName: string }> = {}
  for (const account of Object.keys(rates.value)) {
    map[account] = {
      hourlyRate: rates.value[account] ?? 0,
      displayName: account,
    }
  }
  try {
    await invoke('cost_rates_save', { rates: map })
  } catch (e) {
    console.error('保存时薪失败:', e)
  }
}

// ===== 窗口控制 =====
async function close() {
  await invoke('cost_close')
}

function hideDropdownLater() {
  setTimeout(() => { showProjectDropdown.value = false }, 200)
}

function fmt(n: number, digits = 1): string {
  return n.toFixed(digits)
}

function fmtMoney(n: number): string {
  if (n >= 10000) return (n / 10000).toFixed(1) + '万'
  return n.toFixed(0)
}

// ===== 生命周期 =====
onMounted(async () => {
  document.title = '项目成本分析'
  await loadProjects()
  await loadRates()
  const win = getCurrentWindow()
  win.onCloseRequested(async (e) => {
    e.preventDefault()
    await close()
  })
})
</script>

<template>
  <div class="cost-root">
    <!-- 标题栏（可拖拽） -->
    <header class="cost-header" data-tauri-drag-region>
      <span class="title">项目成本分析</span>
      <button class="close-btn" @click="close" title="关闭">×</button>
    </header>

    <!-- 主体 -->
    <div class="cost-body">
      <!-- 控制栏 -->
      <div class="control-bar">
        <div class="search-wrap">
          <input
            v-model="projectSearch"
            class="control-input"
            placeholder="输入项目名搜索…"
            @focus="showProjectDropdown = true"
            @blur="hideDropdownLater()"
          />
          <div v-if="showProjectDropdown && filteredProjects.length > 0" class="search-dropdown">
            <button
              v-for="p in filteredProjects"
              :key="p.id"
              class="search-option"
              @mousedown.prevent="projectSearch = p.name; showProjectDropdown = false"
            >
              {{ p.name }}
            </button>
          </div>
        </div>
        <button class="query-btn" :disabled="loading || !projectSearch" @click="runQuery">
          {{ loading ? '查询中…' : '查询' }}
        </button>
        <label class="overtime-check">
          <input type="checkbox" v-model="includeResigned" />
          <span>含离职</span>
        </label>
      </div>

      <!-- 时间范围 -->
      <div class="range-bar">
        <button
          v-for="p in RANGE_PRESETS"
          :key="p.key"
          class="range-chip"
          :class="{ active: rangePreset === p.key }"
          @click="rangePreset = p.key"
        >
          {{ p.label }}
        </button>
        <template v-if="rangePreset === 'custom'">
          <input v-model="customStart" type="date" class="range-date" />
          <span class="range-sep">~</span>
          <input v-model="customEnd" type="date" class="range-date" />
        </template>
        <span class="range-hint">{{ rangeHint }}</span>
      </div>

      <div v-if="error" class="cost-error">{{ error }}</div>

      <template v-if="result">
        <!-- 显示选项：含加班只切换本地展示，不重新查询 -->
        <div class="display-options">
          <label class="overtime-check">
            <input type="checkbox" v-model="includeOvertime" />
            <span>含加班</span>
          </label>
        </div>

        <!-- 概览卡片 -->
        <div class="summary-cards">
          <div class="summary-card">
            <div class="card-num">{{ fmt(result.totalHours) }}h</div>
            <div class="card-label">总工时</div>
          </div>
          <div v-if="hasOvertime" class="summary-card">
            <div class="card-num">{{ fmt(result.totalNormalHours ?? 0) }}h / {{ fmt(result.totalOvertimeHours ?? 0) }}h</div>
            <div class="card-label">正常 / 加班</div>
          </div>
          <div class="summary-card">
            <div class="card-num">&yen;{{ fmtMoney(result.totalCost) }}</div>
            <div class="card-label">总成本</div>
          </div>
          <div class="summary-card">
            <div class="card-num">{{ memberCount }}</div>
            <div class="card-label">团队人数</div>
          </div>
          <div class="summary-card">
            <div class="card-num">&yen;{{ fmtMoney(avgCost) }}</div>
            <div class="card-label">人均成本</div>
          </div>
        </div>

        <!-- 双栏：工时对比 + 成本占比 -->
        <div class="charts-row">
          <!-- 左栏：条形图 -->
          <section class="chart-section chart-left">
            <h3 class="section-title">人均工时对比</h3>
            <div class="bar-chart">
              <div v-for="m in result.members" :key="m.account" class="bar-row">
                <div class="bar-name">{{ m.account }}</div>
                <div class="bar-track">
                  <div
                    class="bar-fill"
                    :style="{ width: (m.hours / maxHours * 100) + '%' }"
                  />
                </div>
                <div class="bar-value">{{ fmt(m.hours) }}h</div>
              </div>
            </div>
          </section>

          <!-- 右栏：饼图 -->
          <section v-if="pieSegments.items.length > 0" class="chart-section chart-right">
            <h3 class="section-title">{{ pieSegments.metric === 'cost' ? '成本占比' : '工时占比' }}</h3>
            <div class="pie-container">
              <div class="pie-chart" :style="{ background: pieSegments.gradient }">
                <div class="pie-center">{{ pieSegments.metric === 'cost' ? '¥' + fmtMoney(result!.totalCost) : fmt(result!.totalHours) + 'h' }}</div>
              </div>
              <div class="pie-legend">
                <div v-for="seg in pieSegments.items" :key="seg.account" class="pie-legend-item">
                  <span class="legend-dot" :style="{ background: seg.color }" />
                  <span class="pie-legend-name">{{ seg.account }}</span>
                  <span class="pie-legend-pct">{{ seg.pct.toFixed(1) }}%</span>
                </div>
              </div>
            </div>
          </section>
        </div>

        <!-- 明细表 -->
        <section class="chart-section">
          <h3 class="section-title">成本明细</h3>
          <!-- 统一时薪快捷栏 -->
          <div class="unified-rate-bar">
            <span class="unified-label">统一时薪：</span>
            <input
              v-model.number="unifiedRate"
              type="number"
              class="unified-input"
              min="0"
              placeholder="输入时薪"
            />
            <button class="unified-btn" :disabled="unifiedRate === null || unifiedRate < 0" @click="applyUnifiedRate">
              应用
            </button>
          </div>
          <div class="table-wrap">
            <table class="result-table">
              <thead>
                <tr>
                  <th>姓名</th>
                  <th class="num">工时</th>
                  <th v-if="hasOvertime" class="num">正常</th>
                  <th v-if="hasOvertime" class="num">加班</th>
                  <th class="num">任务数</th>
                  <th class="num">时薪 (元/h)</th>
                  <th class="num">成本</th>
                </tr>
              </thead>
              <tbody>
                <tr v-for="m in result.members" :key="m.account">
                  <td class="name-cell">{{ m.account }}</td>
                  <td class="num">{{ fmt(m.hours) }}</td>
                  <td v-if="hasOvertime" class="num">{{ fmt(m.normalHours ?? 0) }}</td>
                  <td v-if="hasOvertime" class="num overtime-num">{{ fmt(m.overtimeHours ?? 0) }}</td>
                  <td class="num">{{ m.taskCount }}</td>
                  <td class="num rate-cell">
                    <input
                      type="number"
                      class="rate-input"
                      :value="rates[m.account] ?? m.hourlyRate"
                      min="0"
                      @change="onRateChange(m.account, ($event.target as HTMLInputElement).value)"
                    />
                  </td>
                  <td class="num cost-total">{{ m.cost > 0 ? '&yen;' + fmtMoney(m.cost) : '-' }}</td>
                </tr>
              </tbody>
              <tfoot>
                <tr class="total-row">
                  <td>合计</td>
                  <td class="num">{{ fmt(result.totalHours) }}</td>
                  <td v-if="hasOvertime" class="num">{{ fmt(result.totalNormalHours ?? 0) }}</td>
                  <td v-if="hasOvertime" class="num overtime-num">{{ fmt(result.totalOvertimeHours ?? 0) }}</td>
                  <td class="num">-</td>
                  <td class="num">-</td>
                  <td class="num cost-total">&yen;{{ fmtMoney(result.totalCost) }}</td>
                </tr>
              </tfoot>
            </table>
          </div>
        </section>

        <!-- 底部汇总 -->
        <div class="bottom-summary">
          <span>总工时 <strong>{{ fmt(result.totalHours) }}h</strong></span>
          <span class="sep">·</span>
          <span>总成本 <strong>&yen;{{ fmtMoney(result.totalCost) }}</strong></span>
        </div>
      </template>
    </div>
  </div>
</template>

<style scoped>
.cost-root {
  height: 100vh;
  display: flex;
  flex-direction: column;
  background: linear-gradient(135deg, rgba(20, 30, 56, 1), rgba(15, 23, 42, 1));
  color: rgba(255, 255, 255, 0.92);
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif;
}

.cost-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 10px 14px;
  background: rgba(0, 0, 0, 0.25);
  border-bottom: 1px solid rgba(255, 255, 255, 0.06);
  -webkit-app-region: drag;
  user-select: none;
}
.title { font-size: 13px; font-weight: 600; }
.close-btn {
  width: 24px; height: 24px;
  display: inline-flex; align-items: center; justify-content: center;
  font-size: 18px; line-height: 1;
  color: rgba(255, 255, 255, 0.6);
  background: transparent; border: none; border-radius: 6px;
  cursor: pointer;
  -webkit-app-region: no-drag;
}
.close-btn:hover { color: #fff; background: rgba(255, 255, 255, 0.08); }

.cost-body {
  flex: 1;
  overflow-y: auto;
  padding: 10px 14px 16px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}

/* 控制栏 */
.control-bar { display: flex; gap: 8px; align-items: center; }
.search-wrap { position: relative; flex: 1; }
.control-input {
  width: 100%; box-sizing: border-box;
  padding: 7px 10px; font-size: 13px;
  background: rgba(0, 0, 0, 0.25); border: 1px solid rgba(255, 255, 255, 0.1);
  border-radius: 6px; color: rgba(255, 255, 255, 0.9);
  font-family: inherit;
}
.control-input:focus { border-color: rgba(0, 212, 255, 0.5); outline: none; }
.search-dropdown {
  position: absolute; top: 100%; left: 0; right: 0;
  max-height: 180px; overflow-y: auto;
  background: rgba(15, 23, 42, 0.98);
  border: 1px solid rgba(100, 200, 255, 0.2);
  border-radius: 6px; z-index: 10; margin-top: 2px;
}
.search-option {
  display: block; width: 100%; padding: 7px 10px;
  font-size: 12.5px; text-align: left;
  color: rgba(255, 255, 255, 0.85);
  background: transparent; border: none; cursor: pointer; font-family: inherit;
}
.search-option:hover { background: rgba(100, 200, 255, 0.12); color: white; }

.query-btn {
  padding: 7px 18px; font-size: 13px; font-weight: 500;
  color: white; background: linear-gradient(135deg, rgba(0, 212, 255, 0.9), rgba(59, 130, 246, 0.9));
  border: none; border-radius: 6px; cursor: pointer;
  white-space: nowrap; font-family: inherit;
}
.query-btn:hover:not(:disabled) { box-shadow: 0 4px 12px rgba(0, 212, 255, 0.3); }
.query-btn:disabled { opacity: 0.4; cursor: not-allowed; }

.overtime-check {
  display: flex; align-items: center; gap: 4px;
  font-size: 12px; color: rgba(255, 255, 255, 0.6);
  white-space: nowrap; cursor: pointer; user-select: none;
}
.overtime-check input { accent-color: rgba(0, 212, 255, 0.8); }

/* 时间范围栏 */
.range-bar {
  display: flex; align-items: center; flex-wrap: wrap; gap: 6px;
  margin-top: -4px;
}
.range-chip {
  padding: 4px 10px; font-size: 11.5px;
  color: rgba(255, 255, 255, 0.6);
  background: rgba(255, 255, 255, 0.04);
  border: 1px solid rgba(255, 255, 255, 0.08);
  border-radius: 6px; cursor: pointer; font-family: inherit;
  white-space: nowrap;
}
.range-chip:hover { background: rgba(255, 255, 255, 0.08); color: rgba(255, 255, 255, 0.85); }
.range-chip.active {
  color: rgba(0, 212, 255, 0.95);
  background: rgba(0, 212, 255, 0.12);
  border-color: rgba(0, 212, 255, 0.35);
}
.range-date {
  padding: 4px 8px; font-size: 11.5px;
  background: rgba(0, 0, 0, 0.25); border: 1px solid rgba(255, 255, 255, 0.1);
  border-radius: 6px; color: rgba(255, 255, 255, 0.9);
  font-family: inherit; color-scheme: dark;
}
.range-date:focus { border-color: rgba(0, 212, 255, 0.5); outline: none; }
.range-sep { color: rgba(255, 255, 255, 0.4); font-size: 12px; }
.range-hint {
  font-size: 11px; color: rgba(255, 255, 255, 0.4);
  margin-left: auto; white-space: nowrap;
}

.cost-error {
  padding: 8px; font-size: 12.5px;
  color: rgba(248, 113, 113, 0.95);
  background: rgba(239, 68, 68, 0.1); border-radius: 6px;
}

/* 概览卡片 */
.display-options { display: flex; justify-content: flex-end; }
.summary-cards { display: grid; grid-template-columns: repeat(4, 1fr); gap: 6px; }
.summary-card {
  padding: 10px 8px; text-align: center;
  background: rgba(255, 255, 255, 0.03);
  border: 1px solid rgba(255, 255, 255, 0.06);
  border-radius: 8px;
}
.card-num { font-size: 18px; font-weight: 700; line-height: 1.2; color: rgba(255, 255, 255, 0.95); }
.card-label { font-size: 10px; color: rgba(255, 255, 255, 0.45); margin-top: 2px; }

/* 图表区域 */
.charts-row { display: flex; gap: 14px; }
.chart-section { display: flex; flex-direction: column; gap: 6px; }
.chart-left { flex: 1; min-width: 0; }
.chart-right { flex: 1; min-width: 0; }
.section-title {
  margin: 0; font-size: 11px; font-weight: 600;
  color: rgba(0, 212, 255, 0.85); letter-spacing: 0.3px;
}

/* 条形图 */
.bar-chart { display: flex; flex-direction: column; gap: 4px; }
.bar-row { display: flex; align-items: center; gap: 6px; font-size: 11px; }
.bar-name {
  min-width: 56px; max-width: 72px;
  overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  color: rgba(255, 255, 255, 0.75); text-align: right;
}
.bar-track {
  flex: 1; height: 14px;
  background: rgba(255, 255, 255, 0.04);
  border-radius: 3px; overflow: hidden;
}
.bar-fill {
  height: 100%; transition: width 0.3s;
  background: rgba(0, 212, 255, 0.8); border-radius: 3px;
}
.bar-value { min-width: 40px; text-align: right; font-weight: 600; color: rgba(255, 255, 255, 0.7); font-size: 10.5px; }

/* 饼图 */
.pie-container { display: flex; flex-direction: column; align-items: center; gap: 12px; }
.pie-chart {
  width: 180px; height: 180px; border-radius: 50%;
  position: relative; flex-shrink: 0;
}
.pie-center {
  position: absolute; top: 50%; left: 50%;
  transform: translate(-50%, -50%);
  width: 72px; height: 72px; border-radius: 50%;
  background: rgba(15, 23, 42, 0.95);
  display: flex; align-items: center; justify-content: center;
  font-size: 12px; font-weight: 700; color: rgba(0, 212, 255, 0.95);
  white-space: nowrap;
}
.pie-legend { display: flex; flex-direction: column; gap: 4px; width: 100%; }
.pie-legend-item { display: flex; align-items: center; gap: 6px; font-size: 11.5px; }
.legend-dot { display: inline-block; width: 8px; height: 8px; border-radius: 2px; flex-shrink: 0; }
.pie-legend-name {
  flex: 1; min-width: 0;
  overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  color: rgba(255, 255, 255, 0.75);
}
.pie-legend-pct { color: rgba(255, 255, 255, 0.5); font-variant-numeric: tabular-nums; min-width: 40px; text-align: right; }

/* 统一时薪 */
.unified-rate-bar {
  display: flex; align-items: center; gap: 6px;
  padding: 6px 0;
}
.unified-label { font-size: 11px; color: rgba(255, 255, 255, 0.5); white-space: nowrap; }
.unified-input {
  width: 80px; padding: 3px 6px; font-size: 12px;
  background: rgba(0, 0, 0, 0.25); border: 1px solid rgba(255, 255, 255, 0.1);
  border-radius: 4px; color: rgba(255, 255, 255, 0.9);
  font-family: inherit;
}
.unified-input:focus { border-color: rgba(0, 212, 255, 0.5); outline: none; }
.unified-btn {
  padding: 3px 10px; font-size: 11px; font-weight: 500;
  color: rgba(0, 212, 255, 0.95); background: rgba(0, 212, 255, 0.12);
  border: 1px solid rgba(0, 212, 255, 0.35); border-radius: 4px;
  cursor: pointer; font-family: inherit;
}
.unified-btn:hover:not(:disabled) { background: rgba(0, 212, 255, 0.2); }
.unified-btn:disabled { opacity: 0.4; cursor: not-allowed; }

/* 明细表 */
.table-wrap { overflow-x: auto; }
.result-table { width: 100%; border-collapse: collapse; font-size: 11.5px; }
.result-table th {
  text-align: left; padding: 5px 6px;
  color: rgba(255, 255, 255, 0.4); font-weight: 500;
  border-bottom: 1px solid rgba(255, 255, 255, 0.08);
  white-space: nowrap;
}
.result-table th.num { text-align: right; }
.result-table td { padding: 5px 6px; border-bottom: 1px solid rgba(255, 255, 255, 0.03); }
.result-table td.num { text-align: right; font-variant-numeric: tabular-nums; }
.cost-total { font-weight: 600; color: rgba(0, 212, 255, 0.95); }
.overtime-num { color: rgba(245, 158, 11, 0.85); }
.total-row td {
  border-top: 1px solid rgba(255, 255, 255, 0.12);
  font-weight: 600; color: rgba(255, 255, 255, 0.85); padding-top: 6px;
}

/* 内联时薪输入 */
.rate-cell { padding: 2px 4px !important; }
.rate-input {
  width: 80px; padding: 3px 6px; font-size: 12px; text-align: right;
  background: rgba(0, 0, 0, 0.2); border: 1px solid rgba(255, 255, 255, 0.08);
  border-radius: 4px; color: rgba(255, 255, 255, 0.9);
  font-family: inherit; font-variant-numeric: tabular-nums;
}
.rate-input:focus { border-color: rgba(0, 212, 255, 0.5); outline: none; background: rgba(0, 212, 255, 0.05); }

/* 姓名单元格（纯文本，account 即员工中文名） */
.name-cell { font-weight: 500; color: rgba(255, 255, 255, 0.9); white-space: nowrap; }

/* 底部汇总 */
.bottom-summary {
  padding: 8px 10px; font-size: 12.5px; font-weight: 600;
  background: rgba(0, 0, 0, 0.2); border-radius: 6px;
  display: flex; align-items: center; flex-wrap: wrap; gap: 4px;
}
.bottom-summary strong { color: rgba(0, 212, 255, 0.95); }
.sep { color: rgba(255, 255, 255, 0.3); }

/* 隐藏 number input 的 spin button */
.rate-input::-webkit-inner-spin-button,
.rate-input::-webkit-outer-spin-button,
.unified-input::-webkit-inner-spin-button,
.unified-input::-webkit-outer-spin-button {
  opacity: 0.3;
}
</style>
