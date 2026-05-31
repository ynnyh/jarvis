<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { getCurrentWindow } from '@tauri-apps/api/window'

interface CandidateTask {
  id: string
  name: string
  status: string
  priority: string
  score: number
  reason: string
  systemHint?: string
}

interface TodayPlanResponse {
  date: string
  taskIds: string[]
  workStyle: string
  candidateTasks: CandidateTask[]
}

const loading = ref(true)
const loadError = ref('')
const saving = ref(false)
const query = ref('')
const date = ref('')
const taskIds = ref<string[]>([])
const workStyle = ref('balanced')
const candidateTasks = ref<CandidateTask[]>([])
let cleanupClose: (() => void) | null = null

const filteredTasks = computed(() => {
  const q = query.value.trim().toLowerCase()
  if (!q) return candidateTasks.value
  return candidateTasks.value.filter(task => {
    const text = `${task.id} ${task.name} ${task.systemHint ?? ''}`.toLowerCase()
    return text.includes(q)
  })
})

function toggleTask(id: string) {
  if (taskIds.value.includes(id)) {
    taskIds.value = taskIds.value.filter(item => item !== id)
  } else {
    taskIds.value = [...taskIds.value, id]
  }
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
  } catch (error) {
    loadError.value = error instanceof Error ? error.message : String(error)
  } finally {
    loading.value = false
  }
}

async function savePlan() {
  saving.value = true
  try {
    loadError.value = ''
    await invoke('today_plan_save', { taskIds: taskIds.value, date: date.value })
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
  cleanupClose = await win.onCloseRequested(async (event) => {
    event.preventDefault()
    await closeWindow()
  })
})

onUnmounted(() => {
  cleanupClose?.()
})
</script>

<template>
  <div class="tp-root">
    <header class="tp-header" data-tauri-drag-region>
      <div>
        <h1>今日计划</h1>
        <p>先选出今天准备推进的任务，后续复盘和工时结算会把它当作兜底上下文。</p>
      </div>
      <button class="tp-close" @click="closeWindow">×</button>
    </header>

    <main class="tp-body">
      <div v-if="loading" class="tp-empty">正在加载任务候选...</div>
      <div v-else-if="loadError" class="tp-empty error">{{ loadError }}</div>
      <template v-else>
        <section class="tp-summary">
          <span>工作模式：{{ workStyle }}</span>
          <span>已选：{{ taskIds.length }}</span>
        </section>

        <input v-model="query" class="tp-search" type="text" placeholder="搜索任务 ID、名称或系统" />

        <section class="tp-list">
          <label v-for="task in filteredTasks" :key="task.id" class="tp-item">
            <input :checked="taskIds.includes(task.id)" type="checkbox" @change="toggleTask(task.id)" />
            <span class="tp-main">
              <strong>#{{ task.id }} {{ task.name }}</strong>
              <small>{{ task.systemHint || task.reason }}</small>
            </span>
          </label>
        </section>
      </template>
    </main>

    <footer class="tp-footer">
      <button class="tp-btn secondary" :disabled="saving" @click="clearPlan">清空</button>
      <button class="tp-btn secondary" :disabled="saving" @click="closeWindow">取消</button>
      <button class="tp-btn primary" :disabled="saving" @click="savePlan">保存计划</button>
    </footer>
  </div>
</template>

<style scoped>
.tp-root { display: flex; flex-direction: column; height: 100vh; background: #0b1120; color: rgba(255,255,255,.92); }
.tp-header { display: flex; align-items: center; justify-content: space-between; gap: 16px; padding: 18px 22px 14px; background: rgba(17,24,39,.98); border-bottom: 1px solid rgba(148,163,184,.18); user-select: none; }
.tp-header h1 { margin: 0; font-size: 19px; font-weight: 700; }
.tp-header p { margin: 5px 0 0; font-size: 12px; color: rgba(255,255,255,.46); }
.tp-close { width: 30px; height: 30px; border: none; border-radius: 6px; color: rgba(255,255,255,.6); background: transparent; cursor: pointer; font-size: 18px; }
.tp-close:hover { color: rgba(255,255,255,.95); background: rgba(255,255,255,.08); }
.tp-body { flex: 1; min-height: 0; overflow-y: auto; padding: 18px 22px 24px; display: flex; flex-direction: column; gap: 12px; }
.tp-summary { display: flex; gap: 12px; font-size: 12px; color: rgba(255,255,255,.6); }
.tp-search { padding: 8px 10px; font-size: 12px; color: rgba(255,255,255,.92); background: rgba(255,255,255,.06); border: 1px solid rgba(255,255,255,.08); border-radius: 6px; }
.tp-list { display: flex; flex-direction: column; gap: 6px; }
.tp-item { display: flex; gap: 10px; align-items: flex-start; padding: 8px 10px; border-radius: 8px; background: rgba(255,255,255,.04); cursor: pointer; }
.tp-item input { margin-top: 2px; }
.tp-main { display: flex; flex-direction: column; gap: 2px; }
.tp-main strong { font-size: 12px; color: rgba(255,255,255,.94); }
.tp-main small { font-size: 10px; color: rgba(255,255,255,.46); }
.tp-empty { flex: 1; display: flex; align-items: center; justify-content: center; color: rgba(255,255,255,.65); font-size: 12px; }
.tp-empty.error { color: rgba(252,165,165,.95); }
.tp-footer { display: flex; justify-content: flex-end; gap: 10px; padding: 12px 20px; background: rgba(0,0,0,.2); border-top: 1px solid rgba(255,255,255,.06); }
.tp-btn { padding: 8px 16px; font-size: 13px; border-radius: 6px; border: 1px solid transparent; cursor: pointer; }
.tp-btn.secondary { background: transparent; color: rgba(255,255,255,.75); border-color: rgba(255,255,255,.18); }
.tp-btn.primary { background: linear-gradient(135deg, rgba(59,130,246,.95), rgba(37,99,235,.95)); color: white; }
</style>
