<script setup lang="ts">
// 成本分析独立大窗口：项目搜索 + 概览卡片 + 条形图 + 成本占比 + 明细表（内联时薪编辑）。
// 从 in-app overlay 升级为独立 Tauri 窗口，走 cost_open / cost_close 命令。
import { ref, computed, onMounted } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { invoke } from '@tauri-apps/api/core'
import { useConfigStore } from './stores/config'
import { useTheme } from './composables/useTheme'
import ToggleSwitch from './components/ui/ToggleSwitch.vue'
import MatrixRain from './components/MatrixRain.vue'
import CyberParticles from './components/CyberParticles.vue'

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

// 主题：应用当前视觉风格到本窗口，并随设置里切换实时变化（config-changed 广播）
const configStore = useConfigStore()
useTheme()

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
      // 对齐 spec §7 与后端 resolve_range：含当前月往前共 6 个月，起点 = (当前月-5) 月 1 号
      return { start: ymd(new Date(y, m - 5, 1)), end: ymd(now) }
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

// 成本占比颜色板 — 从 CSS 变量读取，随主题切换
function getCostColors(): string[] {
  const s = getComputedStyle(document.documentElement)
  return [
    s.getPropertyValue('--chart-1').trim(),
    s.getPropertyValue('--chart-2').trim(),
    s.getPropertyValue('--chart-3').trim(),
    s.getPropertyValue('--chart-4').trim(),
    s.getPropertyValue('--chart-5').trim(),
    s.getPropertyValue('--chart-6').trim(),
    s.getPropertyValue('--chart-7').trim(),
    s.getPropertyValue('--chart-8').trim(),
  ]
}

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
      includeOvertime: true,
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
  // 读一次 styleTheme 使其成为 computed 的响应依赖，主题切换时自动重算
  void configStore.config.styleTheme
  const chartColors = getCostColors()
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
        color: chartColors[i % chartColors.length],
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
  configStore.load()
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
  <div class="cost-root theme-bg theme-scanline">
    <MatrixRain />
    <CyberParticles />
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
        <ToggleSwitch v-model="includeResigned" label="含离职" />
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
          <ToggleSwitch v-model="includeOvertime" label="含加班" />
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
  background: var(--theme-bg);
  color: var(--text);
  font-family: var(--font-sans);
}

.cost-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 10px 14px;
  background: var(--surface-2);
  border-bottom: 1px solid var(--border-soft);
  -webkit-app-region: drag;
  user-select: none;
}
.title { font-size: 13px; font-weight: 600; }
.close-btn {
  width: 24px; height: 24px;
  display: inline-flex; align-items: center; justify-content: center;
  font-size: 18px; line-height: 1;
  color: var(--text-dim);
  background: transparent; border: none; border-radius: var(--radius-md);
  cursor: pointer;
  -webkit-app-region: no-drag;
}
.close-btn:hover { color: var(--text); background: var(--surface-hover); }

.cost-body {
  flex: 1;
  overflow-y: auto;
  padding: 10px 14px 16px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}

/* 控制栏 */
.control-bar {
  display: flex; gap: 8px; align-items: center;
  background: var(--card-bg);
  border: var(--card-border);
  border-radius: var(--radius-lg);
  padding: 8px 10px;
  box-shadow: var(--card-shadow);
}
.search-wrap { position: relative; flex: 1; }
.control-input {
  width: 100%; box-sizing: border-box;
  padding: 7px 10px; font-size: 13px;
  background: var(--surface-2); border: 1px solid var(--border);
  border-radius: var(--radius-md); color: var(--text);
  font-family: inherit;
}
.control-input:focus { border-color: var(--accent); outline: none; }
.search-dropdown {
  position: absolute; top: 100%; left: 0; right: 0;
  max-height: 180px; overflow-y: auto;
  background: var(--bg);
  border: 1px solid var(--border);
  border-radius: var(--radius-md); z-index: 10; margin-top: 2px;
}
.search-option {
  display: block; width: 100%; padding: 7px 10px;
  font-size: 12.5px; text-align: left;
  color: var(--text);
  background: transparent; border: none; cursor: pointer; font-family: inherit;
}
.search-option:hover { background: var(--surface-hover); color: var(--text); }

.query-btn {
  padding: 7px 18px; font-size: 13px; font-weight: 500;
  color: var(--on-accent); background: linear-gradient(135deg, var(--accent), var(--accent-2));
  border: none; border-radius: var(--radius-md); cursor: pointer;
  white-space: nowrap; font-family: inherit;
}
.query-btn:hover:not(:disabled) { box-shadow: var(--shadow-1), 0 0 var(--glow-spread) var(--glow); }
.query-btn:disabled { opacity: 0.4; cursor: not-allowed; }

/* 时间范围栏 */
.range-bar {
  display: flex; align-items: center; flex-wrap: wrap; gap: 6px;
  margin-top: -4px;
  background: var(--card-bg);
  border: var(--card-border);
  border-radius: var(--radius-lg);
  padding: 8px 10px;
}
.range-chip {
  padding: 4px 10px; font-size: 11.5px;
  color: var(--text-dim);
  background: var(--surface);
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-md); cursor: pointer; font-family: inherit;
  white-space: nowrap;
}
.range-chip:hover { background: var(--surface-hover); color: var(--text); }
.range-chip.active {
  color: var(--accent);
  background: var(--surface-hover);
  border-color: var(--accent);
}
.range-date {
  padding: 4px 8px; font-size: 11.5px;
  background: var(--surface-2); border: 1px solid var(--border);
  border-radius: var(--radius-md); color: var(--text);
  font-family: inherit;
}
.range-date:focus { border-color: var(--accent); outline: none; }
.range-sep { color: var(--text-dim); font-size: 12px; }
.range-hint {
  font-size: 11px; color: var(--text-dim);
  margin-left: auto; white-space: nowrap;
}

.cost-error {
  padding: 8px; font-size: 12.5px;
  color: var(--danger);
  background: color-mix(in srgb, var(--danger) 12%, transparent); border-radius: var(--radius-md);
}

/* 概览卡片 */
.display-options {
  display: flex; justify-content: flex-end;
  background: var(--card-bg);
  border: var(--card-border);
  border-radius: var(--radius-md);
  padding: 6px 10px;
}
.summary-cards { display: grid; grid-template-columns: repeat(4, 1fr); gap: 6px; }
.summary-card {
  padding: 10px 8px; text-align: center;
  background: var(--card-bg);
  border: var(--card-border);
  border-radius: var(--radius-lg);
  box-shadow: var(--card-shadow);
  transition: box-shadow var(--motion-base) var(--ease);
}
.card-num {
  font-size: 18px; font-weight: 700; line-height: 1.2; color: var(--text);
  font-family: var(--font-display);
  font-variant-numeric: var(--num-font-variant);
}
.card-label { font-size: 10px; color: var(--text-dim); margin-top: 2px; }

/* 图表区域 */
.charts-row { display: flex; gap: 14px; }
.chart-section {
  display: flex; flex-direction: column; gap: 6px;
  background: var(--card-bg);
  border: var(--card-border);
  border-radius: var(--radius-lg);
  padding: 10px;
  box-shadow: var(--card-shadow);
}
.chart-left { flex: 1; min-width: 0; }
.chart-right { flex: 1; min-width: 0; }
.section-title {
  margin: 0; font-size: 11px; font-weight: 600;
  color: var(--accent-text); letter-spacing: 0.3px;
  transition: text-shadow var(--motion-base) var(--ease);
}
.section-title:hover { text-shadow: 0 0 8px var(--glow); }

/* 条形图 */
.bar-chart { display: flex; flex-direction: column; gap: 4px; }
.bar-row { display: flex; align-items: center; gap: 6px; font-size: 11px; }
.bar-name {
  min-width: 56px; max-width: 72px;
  overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  color: var(--text-dim); text-align: right;
}
.bar-track {
  flex: 1; height: 14px;
  background: var(--surface);
  border-radius: var(--radius-sm); overflow: hidden;
}
.bar-fill {
  height: 100%; transition: width 0.3s;
  background: var(--accent); border-radius: var(--radius-sm);
  box-shadow: 0 0 var(--glow-spread) var(--glow);
}
.bar-value { min-width: 40px; text-align: right; font-weight: 600; color: var(--text-dim); font-size: 10.5px; }

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
  background: var(--bg);
  display: flex; align-items: center; justify-content: center;
  font-size: 12px; font-weight: 700; color: var(--accent-text);
  white-space: nowrap;
}
.pie-legend { display: flex; flex-direction: column; gap: 4px; width: 100%; }
.pie-legend-item { display: flex; align-items: center; gap: 6px; font-size: 11.5px; }
.legend-dot { display: inline-block; width: 8px; height: 8px; border-radius: 2px; flex-shrink: 0; }
.pie-legend-name {
  flex: 1; min-width: 0;
  overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  color: var(--text-dim);
}
.pie-legend-pct { color: var(--text-dim); font-variant-numeric: tabular-nums; min-width: 40px; text-align: right; }

/* 统一时薪 */
.unified-rate-bar {
  display: flex; align-items: center; gap: 6px;
  padding: 8px 10px;
  background: var(--card-bg);
  border: var(--card-border);
  border-radius: var(--radius-md);
  box-shadow: var(--card-shadow);
}
.unified-label { font-size: 11px; color: var(--text-dim); white-space: nowrap; }
.unified-input {
  width: 80px; padding: 3px 6px; font-size: 12px;
  background: var(--surface-2); border: 1px solid var(--border);
  border-radius: var(--radius-sm); color: var(--text);
  font-family: inherit;
}
.unified-input:focus { border-color: var(--accent); outline: none; }
.unified-btn {
  padding: 3px 10px; font-size: 11px; font-weight: 500;
  color: var(--accent-text); background: var(--surface-hover);
  border: 1px solid var(--accent); border-radius: var(--radius-sm);
  cursor: pointer; font-family: inherit;
}
.unified-btn:hover:not(:disabled) { background: color-mix(in srgb, var(--accent) 22%, transparent); }
.unified-btn:disabled { opacity: 0.4; cursor: not-allowed; }

/* 明细表 */
.table-wrap { overflow-x: auto; }
.result-table { width: 100%; border-collapse: collapse; font-size: 11.5px; }
.result-table th {
  text-align: left; padding: 5px 6px;
  color: var(--text-dim); font-weight: 500;
  border-bottom: 1px solid var(--border);
  white-space: nowrap;
}
.result-table th.num { text-align: right; }
.result-table td { padding: 5px 6px; border-bottom: 1px solid var(--border-soft); }
.result-table td.num { text-align: right; font-variant-numeric: tabular-nums; }
.cost-total { font-weight: 600; color: var(--accent-text); }
.overtime-num { color: var(--warning); }
.total-row td {
  border-top: 1px solid var(--border);
  font-weight: 600; color: var(--text); padding-top: 6px;
}

/* 内联时薪输入 */
.rate-cell { padding: 2px 4px !important; }
.rate-input {
  width: 80px; padding: 3px 6px; font-size: 12px; text-align: right;
  background: var(--surface-2); border: 1px solid var(--border-soft);
  border-radius: var(--radius-sm); color: var(--text);
  font-family: inherit; font-variant-numeric: tabular-nums;
}
.rate-input:focus { border-color: var(--accent); outline: none; background: var(--surface-hover); }

/* 姓名单元格（纯文本，account 即员工中文名） */
.name-cell { font-weight: 500; color: var(--text); white-space: nowrap; }

/* 底部汇总 */
.bottom-summary {
  padding: 8px 10px; font-size: 12.5px; font-weight: 600;
  background: var(--card-bg);
  border: var(--card-border);
  box-shadow: var(--card-shadow);
  border-radius: var(--radius-md);
  display: flex; align-items: center; flex-wrap: wrap; gap: 4px;
}
.bottom-summary strong { color: var(--accent-text); }
.sep { color: var(--text-dim); }

/* 隐藏 number input 的 spin button */
.rate-input::-webkit-inner-spin-button,
.rate-input::-webkit-outer-spin-button,
.unified-input::-webkit-inner-spin-button,
.unified-input::-webkit-outer-spin-button {
  opacity: 0.3;
}
</style>
