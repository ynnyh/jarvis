<script setup lang="ts">
import { ref, computed, watch, onMounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useConfigStore } from '../../stores/config'

interface ProjectInfo {
  id: number | string
  name: string
}

interface PersonRate {
  hourlyRate: number
  displayName: string
}

const store = useConfigStore()
const projects = ref<ProjectInfo[]>([])
const projectSearch = ref('')
const showDropdown = ref(false)

const filteredProjects = computed(() => {
  const q = projectSearch.value.trim().toLowerCase()
  if (!q) return projects.value
  return projects.value.filter(p => p.name.toLowerCase().includes(q))
})
const members = ref<string[]>([])
const rates = ref<Record<string, PersonRate>>({})
const loadingProjects = ref(false)
const loadingMembers = ref(false)
const saving = ref(false)
const error = ref<string | null>(null)
const unifiedRate = ref<number>(0)

const featureEnabled = computed({
  get: () => store.config.costFeatureEnabled,
  set: (v: boolean) => { store.config.costFeatureEnabled = v },
})

async function loadProjects() {
  loadingProjects.value = true
  error.value = null
  try {
    projects.value = await invoke<ProjectInfo[]>('list_projects')
  } catch (e) {
    error.value = `加载项目列表失败: ${e instanceof Error ? e.message : String(e)}`
  } finally {
    loadingProjects.value = false
  }
}

async function loadMembers() {
  const proj = projectSearch.value.trim()
  if (!proj) return
  loadingMembers.value = true
  error.value = null
  try {
    const [names, allRates] = await Promise.all([
      invoke<string[]>('cost_team_members', { projectName: proj }),
      invoke<Record<string, PersonRate>>('cost_rates_load'),
    ])
    members.value = names
    rates.value = allRates
  } catch (e) {
    error.value = `加载人员失败: ${e instanceof Error ? e.message : String(e)}`
  } finally {
    loadingMembers.value = false
  }
}

function getRate(name: string): number {
  return rates.value[name]?.hourlyRate ?? 0
}

function setRate(name: string, val: number) {
  rates.value = {
    ...rates.value,
    [name]: {
      hourlyRate: val,
      displayName: name,
    },
  }
}

/** 统一时薪：将所有已加载成员的时薪设为同一个值 */
function applyUnifiedRate() {
  if (unifiedRate.value <= 0) return
  const updated = { ...rates.value }
  for (const name of members.value) {
    updated[name] = {
      hourlyRate: unifiedRate.value,
      displayName: name,
    }
  }
  rates.value = updated
}

let saveTimer: ReturnType<typeof setTimeout> | null = null

async function saveRates() {
  saving.value = true
  error.value = null
  try {
    await invoke('cost_rates_save', { rates: rates.value })
  } catch (e) {
    error.value = `保存失败: ${e instanceof Error ? e.message : String(e)}`
  } finally {
    saving.value = false
  }
}

/** 防抖自动保存：rates 变化后 600ms 自动写盘 */
watch(rates, () => {
  if (Object.keys(rates.value).length === 0) return
  if (saveTimer) clearTimeout(saveTimer)
  saveTimer = setTimeout(() => { saveRates() }, 600)
}, { deep: true })

onMounted(() => { if (featureEnabled.value) loadProjects() })
</script>

<template>
  <section class="settings-section">
    <h3 class="settings-section-title">项目成本分析</h3>

    <label class="settings-field">
      <span class="settings-field-label">启用功能</span>
      <input type="checkbox" v-model="featureEnabled" />
    </label>
    <p class="settings-section-hint">
      启用后，右键菜单将出现「项目成本」入口。
    </p>

    <template v-if="featureEnabled">
      <div class="cost-divider" />

      <!-- 选项目（模糊搜索） -->
      <label class="settings-field">
        <span class="settings-field-label">选择项目</span>
        <div class="search-wrap">
          <input
            v-model="projectSearch"
            class="settings-input"
            placeholder="输入项目名搜索…"
            @focus="showDropdown = true"
            @blur="showDropdown = false"
          />
          <div v-if="showDropdown && filteredProjects.length > 0" class="search-dropdown">
            <button
              v-for="p in filteredProjects"
              :key="p.id"
              class="search-option"
              @mousedown.prevent="projectSearch = p.name; showDropdown = false; loadMembers()"
            >
              {{ p.name }}
            </button>
          </div>
        </div>
      </label>

      <div v-if="error" class="cost-error">{{ error }}</div>

      <!-- 人员时薪表 -->
      <div v-if="members.length > 0" class="cost-table-wrap">
        <h4 style="color: rgba(255,255,255,0.55); font-size: 12.5px; margin: 0 0 6px;">
          人员时薪（{{ projectSearch }}）
        </h4>

        <!-- 统一时薪 -->
        <div class="unified-row">
          <span class="unified-label">统一时薪</span>
          <input
            type="number"
            class="cost-rate-input"
            v-model.number="unifiedRate"
            min="0"
            step="1"
            placeholder="输入时薪"
          />
          <button class="cost-btn cost-btn-apply" :disabled="unifiedRate <= 0" @click="applyUnifiedRate">
            应用
          </button>
          <span v-if="saving" class="auto-save-hint">自动保存中…</span>
        </div>

        <table class="cost-table">
          <thead>
            <tr>
              <th>姓名</th>
              <th>时薪（元/小时）</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="name in members" :key="name">
              <td class="cost-name">{{ name }}</td>
              <td>
                <input
                  type="number"
                  class="cost-rate-input"
                  :value="getRate(name)"
                  min="0"
                  step="1"
                  @input="setRate(name, Number(($event.target as HTMLInputElement).value))"
                />
              </td>
            </tr>
          </tbody>
        </table>
      </div>
    </template>
  </section>
</template>

<style scoped>
.search-wrap { position: relative; width: 100%; }
.search-wrap .settings-input { width: 100%; box-sizing: border-box; }
.search-dropdown {
  position: absolute;
  top: 100%; left: 0; right: 0;
  max-height: 200px;
  overflow-y: auto;
  background: rgba(15, 23, 42, 0.98);
  border: 1px solid rgba(100, 200, 255, 0.2);
  border-radius: 6px;
  z-index: 10;
  margin-top: 2px;
}
.search-option {
  display: block;
  width: 100%;
  padding: 7px 10px;
  font-size: 12.5px;
  text-align: left;
  color: rgba(255, 255, 255, 0.85);
  background: transparent;
  border: none;
  cursor: pointer;
  font-family: inherit;
}
.search-option:hover {
  background: rgba(100, 200, 255, 0.12);
  color: white;
}

.cost-divider {
  margin: 8px 0;
  border-top: 1px solid rgba(255, 255, 255, 0.06);
}
.cost-btn {
  padding: 6px 14px;
  font-size: 12.5px;
  font-weight: 500;
  border-radius: 6px;
  cursor: pointer;
  transition: all 0.15s;
  margin-top: 8px;
}
.cost-btn-apply {
  color: rgba(0, 212, 255, 0.95);
  background: rgba(0, 212, 255, 0.1);
  border: 1px solid rgba(0, 212, 255, 0.25);
  margin-top: 0;
  padding: 4px 12px;
}
.cost-btn-apply:hover:not(:disabled) {
  background: rgba(0, 212, 255, 0.18);
}
.cost-btn-apply:disabled { opacity: 0.4; cursor: not-allowed; }
.cost-error {
  margin-top: 8px;
  padding: 8px 10px;
  font-size: 12.5px;
  color: rgba(248, 113, 113, 0.95);
  background: rgba(239, 68, 68, 0.1);
  border-radius: 6px;
}
.cost-table-wrap { margin-top: 4px; }
.unified-row {
  display: flex; align-items: center; gap: 8px;
  margin-bottom: 8px; padding: 6px 0;
}
.unified-label {
  font-size: 12px; color: rgba(255, 255, 255, 0.55);
  min-width: 56px;
}
.auto-save-hint {
  font-size: 11px; color: rgba(0, 212, 255, 0.7);
  margin-left: 4px;
}
.cost-table {
  width: 100%;
  border-collapse: collapse;
  font-size: 12.5px;
}
.cost-table th {
  text-align: left;
  padding: 6px 8px;
  color: rgba(255, 255, 255, 0.45);
  font-weight: 500;
  border-bottom: 1px solid rgba(255, 255, 255, 0.08);
}
.cost-table td { padding: 6px 8px; border-bottom: 1px solid rgba(255, 255, 255, 0.04); }
.cost-name {
  color: rgba(255, 255, 255, 0.85);
}
.cost-rate-input {
  width: 80px;
  padding: 4px 8px;
  font-size: 12.5px;
  background: rgba(0, 0, 0, 0.25);
  border: 1px solid rgba(255, 255, 255, 0.1);
  border-radius: 4px;
  color: rgba(255, 255, 255, 0.9);
}
</style>
