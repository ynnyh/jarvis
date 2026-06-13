<script setup lang="ts">
// 宠物形象选择：支持内置宠物和自定义宠物。
// 自定义宠物可通过上传 Lottie JSON / 图片 / GIF 添加。
//
// 切换后立刻生效（App.vue 里 PetAvatar 监听 petId 变化重载动画）。

import { ref, onMounted } from 'vue'
import { useConfigStore } from '../../stores/config'
import {
  PETS,
  PET_CATEGORY_LABELS,
  getAllPets,
  getCustomPets,
  isCustomPetId,
  loadCustomPets,
} from '../../petManifest'
import { customPetDelete } from '../../api/customPet'
import CustomPetEditor from './CustomPetEditor.vue'
import CustomDropdown from '../ui/CustomDropdown.vue'
import type { DropdownOption } from '../ui/CustomDropdown.vue'

const store = useConfigStore()

// ===== 自定义宠物管理 =====

const showEditor = ref(false)
const editingPetId = ref<string | undefined>()
const editingPet = ref<any>(undefined)
const petOptions = ref<DropdownOption[]>([])

// 删除确认
const showDeleteConfirm = ref(false)
const deletingPetId = ref('')
const deletingPetName = ref('')

// 刷新宠物选项列表
async function refreshOptions() {
  await loadCustomPets()
  petOptions.value = getAllPets().map(p => ({
    value: p.id,
    label: `${p.name}${p.description ? ' · ' + p.description : ''}`,
    group: PET_CATEGORY_LABELS[p.category],
  }))
}

// 加载自定义宠物
onMounted(() => {
  refreshOptions()
})

function openEditor(petId?: string, pet?: any) {
  editingPetId.value = petId
  editingPet.value = pet
  showEditor.value = true
}

function closeEditor() {
  showEditor.value = false
  editingPetId.value = undefined
  editingPet.value = undefined
}

function onEditorSaved(petId?: string) {
  closeEditor()
  refreshOptions().then(() => {
    // 上传成功后自动选中新宠物
    if (petId) {
      store.config.petId = petId
    }
  })
}

function onCancel() {
  closeEditor()
}

function openDeleteConfirm(id: string, name: string) {
  deletingPetId.value = id
  deletingPetName.value = name
  showDeleteConfirm.value = true
}

function closeDeleteConfirm() {
  showDeleteConfirm.value = false
  deletingPetId.value = ''
  deletingPetName.value = ''
}

async function confirmDelete() {
  const id = deletingPetId.value
  closeDeleteConfirm()
  try {
    // 如果删除的是当前使用的宠物，先切回默认
    if (store.config.petId === id) {
      store.config.petId = 'robo'
    }
    await customPetDelete(id)
    await refreshOptions()
  } catch (e) {
    console.error('删除自定义宠物失败:', e)
  }
}
</script>

<template>
  <section class="settings-section">
    <h3 class="settings-section-title">宠物形象</h3>

    <!-- 上传按钮 -->
    <div class="pet-actions">
      <button class="settings-btn" @click="openEditor()">
        上传形象
      </button>
    </div>

    <!-- 宠物选择下拉 -->
    <label class="settings-field">
      <span class="settings-field-label">选什么</span>
      <CustomDropdown
        v-model="store.config.petId"
        :options="petOptions"
        placeholder="请选择宠物"
      />
    </label>

    <!-- 自定义宠物管理列表 -->
    <div v-if="getCustomPets().length > 0" class="custom-pets-list">
      <div class="custom-pets-header">自定义形象</div>
      <div
        v-for="pet in getCustomPets()"
        :key="pet.id"
        class="custom-pet-item"
        :class="{ active: store.config.petId === pet.id }"
      >
        <div class="custom-pet-info">
          <span class="custom-pet-name">{{ pet.name }}</span>
          <span class="custom-pet-desc">{{ pet.description }}</span>
        </div>
        <div class="custom-pet-actions">
          <button
            class="settings-btn settings-btn-sm"
            @click="openEditor(pet.id, pet)"
            title="编辑"
          >
            编辑
          </button>
          <button
            class="settings-btn settings-btn-sm settings-btn-danger"
            @click="openDeleteConfirm(pet.id, pet.name)"
            title="删除"
          >
            删除
          </button>
        </div>
      </div>
    </div>

    <!-- 编辑器弹窗 -->
    <div v-if="showEditor" class="pet-editor-overlay">
      <div class="pet-editor-modal">
        <h4 class="pet-editor-title">
          {{ editingPetId ? '编辑形象' : '上传形象' }}
        </h4>
        <CustomPetEditor
          :edit-pet-id="editingPetId"
          :edit-pet="editingPet"
          @saved="onEditorSaved"
          @cancel="onCancel"
        />
      </div>
    </div>

    <!-- 删除确认弹窗 -->
    <div v-if="showDeleteConfirm" class="pet-modal-mask" @click.self="closeDeleteConfirm">
      <div class="pet-modal">
        <h4 class="pet-modal-title">删除形象</h4>
        <p class="pet-modal-body">
          确定要删除「{{ deletingPetName }}」吗？此操作不可恢复。
        </p>
        <div class="pet-modal-actions">
          <button class="settings-btn" @click="closeDeleteConfirm">取消</button>
          <button class="settings-btn settings-btn-danger" @click="confirmDelete">删除</button>
        </div>
      </div>
    </div>

    <p class="settings-section-hint">
      支持 Lottie JSON、PNG、JPG、GIF 格式。自定义形象存储在本地 ~/.jarvis/custom-pets/。
    </p>
  </section>
</template>

<style scoped>
.pet-actions {
  margin-bottom: 8px;
}

.custom-pets-list {
  margin-top: 8px;
  border: var(--divider);
  border-radius: 8px;
  overflow: hidden;
}
.custom-pets-header {
  padding: 6px 10px;
  font-size: 11px;
  font-weight: 600;
  color: var(--text-dim);
  background: var(--surface);
  border-bottom: var(--divider);
}
.custom-pet-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 8px 10px;
  background: var(--bg);
  border-bottom: var(--divider);
  transition: background 0.15s;
}
.custom-pet-item:last-child {
  border-bottom: none;
}
.custom-pet-item:hover {
  background: var(--surface-item-hover);
}
.custom-pet-item.active {
  background: color-mix(in srgb, var(--accent) 8%, transparent);
}
.custom-pet-info {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}
.custom-pet-name {
  font-size: 12px;
  font-weight: 500;
  color: var(--text);
}
.custom-pet-desc {
  font-size: 10px;
  color: var(--text-dim);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.custom-pet-actions {
  display: flex;
  gap: 4px;
  flex-shrink: 0;
}

/* 编辑器弹窗 */
.pet-editor-overlay {
  position: fixed;
  inset: 0;
  z-index: 1000;
  background: rgba(0, 0, 0, 0.5);
  display: flex;
  align-items: center;
  justify-content: center;
}
.pet-editor-modal {
  background: var(--popup-bg);
  border: var(--panel-border);
  border-radius: 12px;
  padding: 16px;
  width: 400px;
  max-width: 90vw;
  max-height: 80vh;
  overflow-y: auto;
  box-shadow: var(--panel-shadow);
}
.pet-editor-title {
  margin: 0 0 12px 0;
  font-size: 14px;
  font-weight: 600;
  color: var(--text);
}

/* 删除确认弹窗 */
.pet-modal-mask {
  position: fixed;
  inset: 0;
  z-index: 1001;
  background: color-mix(in srgb, #000 45%, transparent);
  display: flex;
  align-items: center;
  justify-content: center;
}
.pet-modal {
  width: min(320px, 88vw);
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 16px;
  background: var(--popup-bg);
  border: var(--panel-border);
  border-radius: 10px;
  box-shadow: 0 8px 28px rgba(0, 0, 0, 0.35);
}
.pet-modal-title {
  margin: 0;
  font-size: 13px;
  font-weight: 600;
  color: var(--text);
}
.pet-modal-body {
  margin: 0;
  font-size: 12px;
  line-height: 1.6;
  color: var(--text-dim);
}
.pet-modal-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}
</style>
