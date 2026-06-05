<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { useTheme } from './composables/useTheme'
import MatrixRain from './components/MatrixRain.vue'
import CyberParticles from './components/CyberParticles.vue'
useTheme()

interface CandidateTask {
  id: string
  name: string
  status: string
  priority: string
  score: number
  reason: string
  systemHint?: string
}

interface CustomPlanItem {
  id: string
  name: string
  estimatedHours: number
  kind: string
}

interface TodayPlanResponse {
  date: string
  taskIds: string[]
  workStyle: string
  candidateTasks: CandidateTask[]
  estimatedHours: Record<string, number>
  customItems: CustomPlanItem[]
}

const TX_PRESETS = [
  { id: 'tx-standup', name: '晨会/站会', hours: 0.5 },
  { id: 'tx-review', name: '代码评审', hours: 1.0 },
  { id: 'tx-docs', name: '文档/周报整理', hours: 0.5 },
  { id: 'tx-meeting', name: '会议/需求沟通', hours: 1.0 },
  { id: 'tx-ops', name: '运维/问题排查', hours: 1.0 },
]

const loading = ref(true)
const loadError = ref('')
const saving = ref(false)
const query = ref('')
const date = ref('')
const taskIds = ref<string[]>([])
const workStyle = ref('balanced')
const candidateTasks = ref<CandidateTask[]>([])
const estimatedHours = ref<Record<string, number>>({})
const customItems = ref<CustomPlanItem[]>([])
let nextCustomId = 0

const lookupId = ref('')
const lookupError = ref('')
const lookupLoading = ref(false)

let cleanupClose: (() => void) | null = null

const workStyleLabel = computed(() => {
  switch (workStyle.value) {
    case 'focused': return '专注模式'
    case 'multi': return '并行模式'
    case 'transactional': return '事务模式'
    default: return '平衡模式'
  }
})

const allTasks = computed(() => {
  const found = new Set(candidateTasks.value.map(t => t.id))
  const out = [...candidateTasks.value]
  // 把已选但不在候选里的任务也显示出来（手动添加的禅道任务）
  for (const id of taskIds.value) {
    if (!found.has(id)) {
      out.push({
        id,
        name: `任务 #${id}`,
        status: '',
        priority: '',
        score: 0,
        reason: 'manually added',
      })
    }
  }
  return out
})

const filteredTasks = computed(() => {
  const q = query.value.trim().toLowerCase()
  if (!q) return allTasks.value
  return allTasks.value.filter(task => {
    const text = `${task.id} ${task.name} ${task.systemHint ?? ''}`.toLowerCase()
    return text.includes(q)
  })
})

const totalEstimatedHours = computed(() => {
  let total = 0
  for (const id of taskIds.value) {
    total += estimatedHours.value[id] || 0
  }
  for (const item of customItems.value) {
    total += item.estimatedHours || 0
  }
  return total
})

function toggleTask(id: string) {
  if (taskIds.value.includes(id)) {
    taskIds.value = taskIds.value.filter(item => item !== id)
  } else {
    taskIds.value = [...taskIds.value, id]
  }
}

function setEstimatedHours(id: string, h: number) {
  estimatedHours.value = { ...estimatedHours.value, [id]: h }
}

async function loadPlan() {
  loading.value = true
  loadError.value = ''
  try {
    const data = await invoke<TodayPlanResponse>('today_plan_load')
    date.value = data.date
    taskIds.value = data.taskIds
    workStyle.value = data.workStyle
    candidateTasks.value = data.candidateTasks
    estimatedHours.value = data.estimatedHours || {}
    customItems.value = data.customItems || []
    nextCustomId = customItems.value.length
  } catch (error) {
    loadError.value = error instanceof Error ? error.message : String(error)
  } finally {
    loading.value = false
  }
}

async function lookupTask() {
  const raw = lookupId.value.trim()
  if (!raw) return
  lookupLoading.value = true
  lookupError.value = ''
  try {
    const task = await invoke<CandidateTask>('today_plan_lookup_task', { taskId: raw })
    if (!candidateTasks.value.find(t => t.id === task.id)) {
      candidateTasks.value = [...candidateTasks.value, task]
    }
    if (!taskIds.value.includes(task.id)) {
      taskIds.value = [...taskIds.value, task.id]
    }
    lookupId.value = ''
  } catch (error) {
    lookupError.value = error instanceof Error ? error.message : String(error)
  } finally {
    lookupLoading.value = false
  }
}

function addTxPreset(id: string, name: string, hours: number) {
  const already = customItems.value.find(c => c.id === id)
  if (already) return
  customItems.value = [...customItems.value, { id, name, estimatedHours: hours, kind: 'transaction' }]
}

function addCustomItem() {
  const id = `custom-${nextCustomId++}`
  customItems.value = [...customItems.value, { id, name: '', estimatedHours: 0, kind: 'custom' }]
}

function removeCustomItem(id: string) {
  customItems.value = customItems.value.filter(c => c.id !== id)
}

function updateCustomItem(id: string, patch: Partial<CustomPlanItem>) {
  customItems.value = customItems.value.map(c => c.id === id ? { ...c, ...patch } : c)
}

async function savePlan() {
  saving.value = true
  try {
    loadError.value = ''
    await invoke('today_plan_save', {
      taskIds: taskIds.value,
      date: date.value,
      estimatedHours: estimatedHours.value,
      customItems: customItems.value,
    })
    await closeWindow()
  } catch (error) {
    loadError.value = error instanceof Error ? error.message : String(error)
  } finally {
    saving.value = false
  }
}

async function clearPlan() {
  saving.value = true
  try {
    loadError.value = ''
    await invoke('today_plan_clear', { date: date.value })
    await closeWindow()
  } catch (error) {
    loadError.value = error instanceof Error ? error.message : String(error)
  } finally {
    saving.value = false
  }
}

async function closeWindow() {
  await invoke('today_plan_close')
}

onMounted(async () => {
  await loadPlan()
  const win = getCurrentWindow()
  cleanupClose = await win.onCloseRequested(async event => {
    event.preventDefault()
    await closeWindow()
  })
})

onUnmounted(() => {
  cleanupClose?.()
})
</script>

<template>
  <div class="tp-root theme-bg">
    <MatrixRain />
    <CyberParticles />
    <header class="tp-header" data-tauri-drag-region>
      <div>
        <h1>今日计划</h1>
        <p>选任务、估工时，下班批量写入时自动填充。</p>
      </div>
      <button class="tp-close" @click="closeWindow">x</button>
    </header>

    <main class="tp-body">
      <div v-if="loading" class="tp-empty">正在加载...</div>
      <div v-else-if="loadError" class="tp-empty error">{{ loadError }}</div>
      <template v-else>
        <section class="tp-summary">
          <span>{{ workStyleLabel }}</span>
          <span>已选 {{ taskIds.length + customItems.length }} 项</span>
          <span :class="totalEstimatedHours >= 8 ? 'tp-full' : ''">合计 {{ totalEstimatedHours.toFixed(1) }} / 8 h</span>
        </section>

        <input v-model="query" class="tp-search" type="text" placeholder="搜索任务 ID、名称或系统路径（前端过滤）" />

        <!-- 候选任务 -->
        <section class="tp-list">
          <div v-for="task in filteredTasks" :key="task.id" class="tp-item" :class="{ active: taskIds.includes(task.id) }">
            <input :checked="taskIds.includes(task.id)" type="checkbox" @change="toggleTask(task.id)" />
            <span class="tp-main">
              <strong>#{{ task.id }} {{ task.name }}</strong>
              <small>{{ task.systemHint || task.reason }}</small>
            </span>
            <input
              v-if="taskIds.includes(task.id)"
              class="tp-hours"
              type="number"
              min="0"
              max="12"
              step="0.5"
              :value="estimatedHours[task.id] || 0"
              @input="setEstimatedHours(task.id, parseFloat(($event.target as HTMLInputElement).value) || 0)"
              placeholder="h"
            />
          </div>
        </section>

        <!-- 手动添加禅道任务 -->
        <div class="tp-add-row">
          <input
            v-model="lookupId"
            class="tp-add-input"
            type="text"
            placeholder="输入禅道任务ID 直接添加"
            @keyup.enter="lookupTask()"
          />
          <button class="tp-btn-sm" :disabled="lookupLoading" @click="lookupTask()">添加</button>
        </div>
        <div v-if="lookupError" class="tp-error-text">{{ lookupError }}</div>

        <!-- 事务类预设 -->
        <section class="tp-section">
          <div class="tp-section-title">事务类（点击添加）</div>
          <div class="tp-tx-grid">
            <button
              v-for="tx in TX_PRESETS"
              :key="tx.id"
              class="tp-tx-chip"
              :class="{ used: customItems.some(c => c.id === tx.id) }"
              :disabled="customItems.some(c => c.id === tx.id)"
              @click="addTxPreset(tx.id, tx.name, tx.hours)"
            >{{ tx.name }} ({{ tx.hours }}h)</button>
          </div>
        </section>

        <!-- 自定义任务 & 事务项 -->
        <section class="tp-section">
          <div class="tp-section-title">自定义 & 已添加事务</div>
          <div v-if="customItems.length === 0" class="tp-empty-sm">暂无，上方点事务类快速添加，或点击 + 按钮添加自定义项</div>
          <div v-for="item in customItems" :key="item.id" class="tp-custom-row">
            <span class="tp-custom-tag">{{ item.kind === 'transaction' ? '事务' : '自定义' }}</span>
            <input
              class="tp-custom-name"
              type="text"
              placeholder="名称"
              :value="item.name"
              @input="updateCustomItem(item.id, { name: ($event.target as HTMLInputElement).value })"
            />
            <input
              class="tp-hours"
              type="number"
              min="0"
              max="12"
              step="0.5"
              :value="item.estimatedHours"
              @input="updateCustomItem(item.id, { estimatedHours: parseFloat(($event.target as HTMLInputElement).value) || 0 })"
              placeholder="h"
            />
            <button class="tp-btn-icon" @click="removeCustomItem(item.id)">x</button>
          </div>
          <button class="tp-btn-sm secondary" @click="addCustomItem()">+ 添加自定义</button>
        </section>
      </template>
    </main>

    <footer class="tp-footer">
      <div class="tp-footer-total">合计 {{ totalEstimatedHours.toFixed(1) }} / 8 h</div>
      <div style="flex:1" />
      <button class="tp-btn secondary" :disabled="saving" @click="clearPlan">清空</button>
      <button class="tp-btn secondary" :disabled="saving" @click="closeWindow">取消</button>
      <button class="tp-btn primary" :disabled="saving" @click="savePlan">保存计划</button>
    </footer>
  </div>
</template>

<style scoped>
.tp-root { display: flex; flex-direction: column; height: 100vh; background: var(--bg); color: var(--text); font-family: system-ui, -apple-system, sans-serif; }
.tp-header { display: flex; align-items: center; justify-content: space-between; gap: 16px; padding: 18px 22px 14px; background: var(--panel-bg); border-bottom: 1px solid var(--divider); user-select: none; }
.tp-header h1 { margin: 0; font-size: 19px; font-weight: 700; }
.tp-header p { margin: 5px 0 0; font-size: 12px; color: var(--text-ghost); }
.tp-close { width: 30px; height: 30px; border: none; border-radius: 6px; color: var(--text-dim); background: transparent; cursor: pointer; font-size: 18px; }
.tp-close:hover { color: var(--text); background: var(--surface-item-hover); }
.tp-body { flex: 1; min-height: 0; overflow-y: auto; padding: 16px 22px 24px; display: flex; flex-direction: column; gap: 12px; }
.tp-summary { display: flex; gap: 12px; font-size: 12px; color: var(--text-dim); }
.tp-summary .tp-full { color: var(--green-text); }
.tp-search { padding: 8px 10px; font-size: 12px; color: var(--text); background: var(--input-bg); border: 1px solid var(--input-border); border-radius: 6px; }
.tp-list { display: flex; flex-direction: column; gap: 4px; max-height: 340px; overflow-y: auto; }
.tp-item { display: flex; gap: 10px; align-items: center; padding: 7px 10px; border-radius: 8px; background: var(--surface); cursor: pointer; }
.tp-item.active { background: var(--blue-bg); }
.tp-item input[type="checkbox"] { margin: 0; }
.tp-main { flex: 1; display: flex; flex-direction: column; gap: 1px; min-width: 0; }
.tp-main strong { font-size: 12px; color: var(--text); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.tp-main small { font-size: 10px; color: var(--text-ghost); }
.tp-hours { width: 52px; padding: 3px 6px; font-size: 12px; color: var(--text); background: var(--input-bg); border: 1px solid var(--input-border); border-radius: 4px; text-align: center; }
.tp-add-row { display: flex; gap: 8px; }
.tp-add-input { flex: 1; padding: 7px 10px; font-size: 12px; color: var(--text); background: var(--input-bg); border: 1px solid var(--input-border); border-radius: 6px; }
.tp-error-text { font-size: 11px; color: var(--red-text); }
.tp-btn-sm { padding: 6px 14px; font-size: 12px; border-radius: 6px; border: 1px solid var(--border); background: var(--input-bg); color: var(--text); cursor: pointer; }
.tp-btn-sm.secondary { background: transparent; }
.tp-btn-sm:hover { background: var(--surface-item-hover); }
.tp-btn-sm:disabled { opacity: .4; cursor: not-allowed; }
.tp-section { margin-top: 4px; }
.tp-section-title { font-size: 11px; color: var(--text-ghost); text-transform: uppercase; letter-spacing: .04em; margin-bottom: 6px; }
.tp-tx-grid { display: flex; flex-wrap: wrap; gap: 6px; }
.tp-tx-chip { padding: 5px 10px; font-size: 11px; border-radius: 20px; border: 1px solid var(--border); background: var(--surface); color: var(--text); cursor: pointer; }
.tp-tx-chip:hover { background: var(--blue-bg); border-color: color-mix(in srgb, var(--blue-text) 30%, transparent); }
.tp-tx-chip.used { opacity: .35; cursor: not-allowed; border-color: var(--divider); }
.tp-tx-chip:disabled { cursor: not-allowed; }
.tp-custom-row { display: flex; gap: 8px; align-items: center; margin-bottom: 6px; }
.tp-custom-tag { font-size: 10px; padding: 1px 6px; border-radius: 3px; background: var(--surface-item-hover); color: var(--text-ghost); }
.tp-custom-name { flex: 1; padding: 5px 8px; font-size: 12px; color: var(--text); background: var(--input-bg); border: 1px solid var(--input-border); border-radius: 4px; }
.tp-btn-icon { width: 24px; height: 24px; border: none; border-radius: 4px; color: var(--text-ghost); background: transparent; cursor: pointer; font-size: 14px; }
.tp-btn-icon:hover { color: var(--red-text); background: var(--surface-item-hover); }
.tp-empty { flex: 1; display: flex; align-items: center; justify-content: center; color: var(--text-dim); font-size: 12px; }
.tp-empty.error { color: var(--red-text); }
.tp-empty-sm { font-size: 11px; color: var(--text-muted); padding: 6px 0; }
.tp-footer { display: flex; align-items: center; gap: 10px; padding: 12px 20px; background: var(--panel-bg); border-top: 1px solid var(--divider); }
.tp-footer-total { font-size: 12px; color: var(--text-dim); }
.tp-btn { padding: 8px 16px; font-size: 13px; border-radius: 6px; border: 1px solid transparent; cursor: pointer; }
.tp-btn.secondary { background: transparent; color: var(--text-dim); border-color: var(--border); }
.tp-btn.primary { background: linear-gradient(135deg, color-mix(in srgb, var(--accent) 90%, transparent), color-mix(in srgb, var(--accent) 70%, transparent)); color: white; }
</style>
