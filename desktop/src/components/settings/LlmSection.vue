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
    <template v-if="!editing">
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

      <button class="settings-btn model-add-btn" @click="openEditor()">
        + 新增模型
      </button>
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
</style>
