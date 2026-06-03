<script setup lang="ts">
import { ref, computed } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useConfigStore } from '../../stores/config'
import ModelEditor from './ModelEditor.vue'

const store = useConfigStore()

const switching = ref('')
const editing = ref(false)
const editingId = ref('')

const profiles = computed(() => store.config.llmProfiles ?? [])
const activeId = computed(() => store.config.activeLlmProfileId ?? '')

// ===== CC Switch 全量导入 =====

interface CcProvider {
  id: string
  name: string
  appType: string
  baseUrl: string
  model: string
  wireApi: string
  hasApiKey: boolean
}

const showImport = ref(false)
const importLoading = ref(false)
const ccProviders = ref<CcProvider[]>([])
const selected = ref<Set<string>>(new Set())
const importing = ref(false)

const claudeProviders = computed(() => ccProviders.value.filter(p => p.appType === 'claude'))
const codexProviders = computed(() => ccProviders.value.filter(p => p.appType === 'codex'))

const existingKeys = computed(() => {
  const set = new Set<string>()
  for (const p of profiles.value) {
    const url = (p.baseUrl || '').toLowerCase().replace(/\/+$/, '')
    set.add(`${url}::${p.model}`)
  }
  return set
})

function isImported(p: CcProvider): boolean {
  const url = (p.baseUrl || '').toLowerCase().replace(/\/+$/, '')
  return existingKeys.value.has(`${url}::${p.model}`)
}

async function openImport() {
  showImport.value = true
  importLoading.value = true
  try {
    const list = await invoke<CcProvider[]>('cc_switch_list_providers')
    ccProviders.value = list
    selected.value = new Set()
  } catch (e) {
    console.error('加载 CC Switch providers 失败:', e)
  } finally {
    importLoading.value = false
  }
}

function closeImport() {
  showImport.value = false
  ccProviders.value = []
  selected.value = new Set()
}

function toggleSelect(id: string) {
  const s = new Set(selected.value)
  if (s.has(id)) s.delete(id)
  else s.add(id)
  selected.value = s
}

const selectedCount = computed(() => selected.value.size)

async function doImport() {
  if (selected.value.size === 0) return
  importing.value = true
  const ids = Array.from(selected.value)
  let okCount = 0
  for (const pid of ids) {
    try {
      const r = await invoke<any>('cc_switch_import_provider', { providerId: pid })
      if (r?.success) okCount++
    } catch (e) {
      console.error('导入 CC Switch provider 失败:', pid, e)
    }
  }
  importing.value = false
  // 重新加载 config
  const remote = await invoke<any>('config_load')
  store.applyLlmProfile(remote)
  closeImport()
}

// ===== 原有功能 =====

function providerLabel(p: string) {
  if (p === 'deepseek') return 'DeepSeek'
  if (p === 'openai') return 'OpenAI'
  return '自定义'
}

function openEditor(profileId?: string) {
  editingId.value = profileId ?? ''
  editing.value = true
}

function closeEditor() {
  editing.value = false
  editingId.value = ''
}

async function switchProfile(id: string) {
  if (id === activeId.value) return
  switching.value = id
  try {
    const remote = await invoke<any>('llm_profile_switch', { profileId: id })
    store.applyLlmProfile(remote)
  } catch (e) {
    console.error('切换 profile 失败:', e)
  } finally {
    switching.value = ''
  }
}

async function deleteProfile(id: string) {
  try {
    const remote = await invoke<any>('llm_profile_delete', { profileId: id })
    store.applyLlmProfile(remote)
  } catch (e) {
    console.error('删除 profile 失败:', e)
  }
}
</script>

<template>
  <section class="settings-section">
    <template v-if="!editing && !showImport">
      <h3 class="settings-section-title">AI 模型</h3>

      <div v-if="profiles.length > 0" class="model-list">
        <div v-for="p in profiles" :key="p.id"
          class="model-card"
          :class="{ active: p.id === activeId }">
          <div class="model-info" @click="switchProfile(p.id)">
            <span class="model-name">{{ p.name }}</span>
            <span class="model-meta">{{ providerLabel(p.provider) }} · {{ p.model }}</span>
          </div>
          <span v-if="p.id === activeId" class="model-badge">启用中</span>
          <button v-else class="model-switch-btn"
            :disabled="switching === p.id"
            @click="switchProfile(p.id)">
            {{ switching === p.id ? '…' : '切换' }}
          </button>
          <button class="model-edit-btn" @click="openEditor(p.id)" title="编辑">✎</button>
          <button class="model-del-btn" @click.stop="deleteProfile(p.id)" title="删除">×</button>
        </div>
      </div>

      <p v-else class="settings-section-hint" style="text-align:center;padding:16px 0;">
        还没有配置模型，点击下方按钮添加
      </p>

      <div class="model-actions-row">
        <button class="settings-btn model-add-btn" @click="openEditor()">
          + 新增模型
        </button>
        <button class="settings-btn model-add-btn" @click="openImport()">
          从 CC Switch 导入
        </button>
      </div>
    </template>

    <!-- CC Switch 导入面板 -->
    <template v-else-if="showImport && !editing">
      <div class="editor-header">
        <h3 class="settings-section-title">从 CC Switch 导入</h3>
        <button class="settings-btn" @click="closeImport" style="font-size:11px;">← 返回列表</button>
      </div>

      <div v-if="importLoading" class="cc-loading">正在扫描 CC Switch 数据库...</div>

      <template v-else>
        <p v-if="ccProviders.length === 0" class="settings-section-hint" style="text-align:center;padding:16px 0;">
          未检测到 CC Switch 配置，或数据库中没有 provider
        </p>

        <template v-else>
          <!-- Claude 分组 -->
          <div v-if="claudeProviders.length > 0" class="cc-group">
            <div class="cc-group-title">Claude 模型</div>
            <div class="cc-list">
              <div v-for="p in claudeProviders" :key="p.id"
                class="cc-item"
                :class="{ 'cc-imported': isImported(p), 'cc-selected': selected.has(p.id) }"
                @click="!isImported(p) && toggleSelect(p.id)">
                <input type="checkbox"
                  :checked="selected.has(p.id)"
                  :disabled="isImported(p)"
                  @click.stop
                  @change="toggleSelect(p.id)" />
                <div class="cc-item-info">
                  <span class="cc-item-name">{{ p.name }}</span>
                  <span class="cc-item-meta">{{ p.model }} · {{ p.baseUrl }}</span>
                </div>
                <span v-if="isImported(p)" class="cc-imported-badge">已导入</span>
                <span v-else-if="!p.hasApiKey" class="cc-nokey-badge">无密钥</span>
              </div>
            </div>
          </div>

          <!-- Codex 分组 -->
          <div v-if="codexProviders.length > 0" class="cc-group">
            <div class="cc-group-title">OpenAI 兼容模型</div>
            <div class="cc-list">
              <div v-for="p in codexProviders" :key="p.id"
                class="cc-item"
                :class="{ 'cc-imported': isImported(p), 'cc-selected': selected.has(p.id) }"
                @click="!isImported(p) && toggleSelect(p.id)">
                <input type="checkbox"
                  :checked="selected.has(p.id)"
                  :disabled="isImported(p)"
                  @click.stop
                  @change="toggleSelect(p.id)" />
                <div class="cc-item-info">
                  <span class="cc-item-name">{{ p.name }}</span>
                  <span class="cc-item-meta">{{ p.model }} · {{ p.baseUrl }}</span>
                </div>
                <span v-if="isImported(p)" class="cc-imported-badge">已导入</span>
                <span v-else-if="!p.hasApiKey" class="cc-nokey-badge">无密钥</span>
              </div>
            </div>
          </div>

          <div class="cc-actions">
            <button class="settings-btn settings-btn-primary"
              :disabled="importing || selectedCount === 0"
              @click="doImport">
              {{ importing ? '导入中...' : `导入选中 (${selectedCount})` }}
            </button>
          </div>
        </template>
      </template>
    </template>

    <template v-else>
      <div class="editor-header">
        <h3 class="settings-section-title">{{ editingId ? '编辑模型' : '新增模型' }}</h3>
        <button class="settings-btn" @click="closeEditor" style="font-size:11px;">← 返回列表</button>
      </div>
      <ModelEditor :profile-id="editingId" @saved="closeEditor" />
    </template>
  </section>
</template>

<style scoped>
.model-actions-row {
  display: flex;
  gap: 8px;
}
.model-list {
  display: flex;
  flex-direction: column;
  gap: 4px;
  margin-bottom: 8px;
}
.model-card {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 8px 10px;
  background: rgba(255, 255, 255, 0.03);
  border: 1px solid rgba(255, 255, 255, 0.08);
  border-radius: 6px;
  transition: border-color 0.15s;
}
.model-card:hover {
  border-color: rgba(255, 255, 255, 0.15);
}
.model-card.active {
  background: rgba(147, 197, 253, 0.06);
  border-color: rgba(147, 197, 253, 0.2);
}
.model-info {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 1px;
  cursor: pointer;
}
.model-name {
  font-size: 13px;
  color: rgba(255, 255, 255, 0.9);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.model-meta {
  font-size: 11px;
  color: rgba(255, 255, 255, 0.35);
}
.model-badge {
  flex-shrink: 0;
  font-size: 10px;
  padding: 2px 8px;
  background: rgba(147, 197, 253, 0.15);
  color: rgba(147, 197, 253, 0.9);
  border-radius: 10px;
}
.model-switch-btn {
  flex-shrink: 0;
  font-size: 11px;
  padding: 2px 10px;
  background: rgba(255, 255, 255, 0.06);
  border: 1px solid rgba(255, 255, 255, 0.1);
  color: rgba(255, 255, 255, 0.7);
  border-radius: 4px;
  cursor: pointer;
}
.model-switch-btn:hover:not(:disabled) {
  background: rgba(147, 197, 253, 0.1);
  color: rgba(147, 197, 253, 0.9);
}
.model-edit-btn {
  flex-shrink: 0;
  width: 22px;
  height: 22px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: none;
  border: none;
  color: rgba(255, 255, 255, 0.25);
  cursor: pointer;
  border-radius: 4px;
  font-size: 13px;
}
.model-edit-btn:hover {
  color: rgba(147, 197, 253, 0.9);
  background: rgba(147, 197, 253, 0.1);
}
.model-del-btn {
  flex-shrink: 0;
  width: 22px;
  height: 22px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: none;
  border: none;
  color: rgba(255, 255, 255, 0.2);
  cursor: pointer;
  border-radius: 4px;
  font-size: 14px;
}
.model-del-btn:hover {
  color: rgba(248, 113, 113, 0.95);
  background: rgba(239, 68, 68, 0.1);
}
.model-add-btn {
  font-size: 12px;
  color: rgba(147, 197, 253, 0.8);
  padding: 6px 0;
}
.model-add-btn:hover {
  color: rgba(147, 197, 253, 1);
}
.editor-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}
.cc-loading {
  text-align: center;
  padding: 24px 0;
  font-size: 12px;
  color: rgba(255, 255, 255, 0.45);
}
.cc-group {
  margin-bottom: 8px;
}
.cc-group-title {
  font-size: 11px;
  color: rgba(255, 255, 255, 0.4);
  padding: 6px 0 4px;
  text-transform: uppercase;
  letter-spacing: 0.5px;
}
.cc-list {
  display: flex;
  flex-direction: column;
  gap: 3px;
}
.cc-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 7px 10px;
  background: rgba(255, 255, 255, 0.03);
  border: 1px solid rgba(255, 255, 255, 0.08);
  border-radius: 6px;
  cursor: pointer;
  transition: border-color 0.15s;
}
.cc-item:hover:not(.cc-imported) {
  border-color: rgba(255, 255, 255, 0.15);
}
.cc-item.cc-selected {
  background: rgba(147, 197, 253, 0.06);
  border-color: rgba(147, 197, 253, 0.25);
}
.cc-item.cc-imported {
  opacity: 0.5;
  cursor: default;
}
.cc-item input[type="checkbox"] {
  flex-shrink: 0;
  accent-color: rgba(147, 197, 253, 0.8);
}
.cc-item-info {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 1px;
}
.cc-item-name {
  font-size: 13px;
  color: rgba(255, 255, 255, 0.9);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.cc-item-meta {
  font-size: 10px;
  color: rgba(255, 255, 255, 0.3);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.cc-imported-badge {
  flex-shrink: 0;
  font-size: 10px;
  padding: 1px 6px;
  background: rgba(255, 255, 255, 0.06);
  color: rgba(255, 255, 255, 0.4);
  border-radius: 8px;
}
.cc-nokey-badge {
  flex-shrink: 0;
  font-size: 10px;
  padding: 1px 6px;
  background: rgba(239, 68, 68, 0.1);
  color: rgba(248, 113, 113, 0.8);
  border-radius: 8px;
}
.cc-actions {
  margin-top: 8px;
  display: flex;
  justify-content: flex-end;
}
</style>
