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

interface MemberBrief {
  account: string
  realname: string
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
const members = ref<MemberBrief[]>([])
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
    const [brief, allRates] = await Promise.all([
      invoke<MemberBrief[]>('cost_team_members', { projectName: proj }),
      invoke<Record<string, PersonRate>>('cost_rates_load'),
    ])
    members.value = brief
    rates.value = allRates
  } catch (e) {
    error.value = `加载人员失败: ${e instanceof Error ? e.message : String(e)}`
  } finally {
    loadingMembers.value = false
  }
}

/** 姓名列显示：禅道中文名（空则账号） */
function displayName(m: MemberBrief): string {
  return m.realname && m.realname !== m.account ? m.realname : m.account
}

function getRate(account: string): number {
  return rates.value[account]?.hourlyRate ?? 0
}

function setRate(m: MemberBrief, val: number) {
  rates.value = {
    ...rates.value,
    [m.account]: {
      hourlyRate: val,
      // 存真名作为 displayName，空则回退账号
      displayName: m.realname || m.account,
    },
  }
}

/** 统一时薪：将所有已加载成员的时薪设为同一个值 */
function applyUnifiedRate() {
  if (unifiedRate.value <= 0) return
  const updated = { ...rates.value }
  for (const m of members.value) {
    updated[m.account] = {
      hourlyRate: unifiedRate.value,
      displayName: m.realname || m.account,
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
        <h4 style="color: var(--text-dim); font-size: 12.5px; margin: 0 0 6px;">
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
            <tr v-for="m in members" :key="m.account">
              <td class="cost-name">{{ displayName(m) }}</td>
              <td>
                <input
                  type="number"
                  class="cost-rate-input"
                  :value="getRate(m.account)"
                  min="0"
                  step="1"
                  @input="setRate(m, Number(($event.target as HTMLInputElement).value))"
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
  background: var(--panel-bg);
  border: var(--panel-border);
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
  color: var(--text);
  background: transparent;
  border: none;
  cursor: pointer;
  font-family: inherit;
}
.search-option:hover {
  background: var(--surface-item-hover);
  color: var(--btn-primary-color);
}

.cost-divider {
  margin: 8px 0;
  border-top: var(--divider);
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
  color: var(--accent-text);
  background: color-mix(in srgb, var(--accent) 10%, transparent);
  border: 1px solid color-mix(in srgb, var(--accent) 25%, transparent);
  margin-top: 0;
  padding: 4px 12px;
}
.cost-btn-apply:hover:not(:disabled) {
  background: color-mix(in srgb, var(--accent) 18%, transparent);
}
.cost-btn-apply:disabled { opacity: 0.4; cursor: not-allowed; }
.cost-error {
  margin-top: 8px;
  padding: 8px 10px;
  font-size: 12.5px;
  color: var(--red-text);
  background: var(--red-bg);
  border-radius: 6px;
}
.cost-table-wrap { margin-top: 4px; }
.unified-row {
  display: flex; align-items: center; gap: 8px;
  margin-bottom: 8px; padding: 6px 0;
}
.unified-label {
  font-size: 12px; color: var(--text-dim);
  min-width: 56px;
}
.auto-save-hint {
  font-size: 11px; color: color-mix(in srgb, var(--accent) 70%, transparent);
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
  color: var(--text-dim);
  font-weight: 500;
  border-bottom: var(--divider);
}
.cost-table td { padding: 6px 8px; border-bottom: var(--divider-soft); }
.cost-name {
  color: var(--text);
}
.cost-account {
  color: var(--text-dim);
  font-family: monospace;
  font-size: 11.5px;
}
.cost-rate-input {
  width: 80px;
  padding: 4px 8px;
  font-size: 12.5px;
  background: var(--input-bg);
  border: var(--input-border);
  border-radius: 4px;
  color: var(--text);
}
</style>
