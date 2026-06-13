<script setup lang="ts">
import { ref, computed } from 'vue'
import { useConfigStore } from '../../stores/config'
import { generateCustomPetId, customPetSave, type CustomPet, type ImageAnimation } from '../../api/customPet'
import { loadCustomPets, type PetInfo } from '../../petManifest'
import CustomDropdown from '../ui/CustomDropdown.vue'

const props = defineProps<{
  /** 编辑时传入现有宠物 ID，新建时为空 */
  editPetId?: string
  /** 编辑时传入现有宠物数据 */
  editPet?: PetInfo
}>()

const emit = defineEmits<{
  saved: [petId: string]
  cancel: []
}>()

const store = useConfigStore()

const form = ref({
  name: '',
  description: '',
  animation: 'breath' as ImageAnimation,
})

const fileInput = ref<HTMLInputElement | null>(null)
const fileData = ref<unknown>(null)
const fileType = ref<'lottie' | 'image' | 'gif'>('lottie')
const fileName = ref('')
const saving = ref(false)
const error = ref('')

const isEdit = computed(() => !!props.editPetId)
const hasFile = computed(() => fileData.value !== null)
// 编辑模式下始终显示动画选择器；新建模式下只有图片类型显示
const showAnimationSelect = computed(() => isEdit.value || fileType.value === 'image')

const animationOptions = [
  { value: 'breath', label: '呼吸（缓慢缩放）' },
  { value: 'swing', label: '摇摆（左右摇晃）' },
  { value: 'rotate', label: '旋转（缓慢旋转）' },
  { value: 'bounce', label: '弹跳（上下弹跳）' },
  { value: 'none', label: '静止' },
]

/** 接受的文件类型 */
const acceptTypes = '.json,.png,.jpg,.jpeg,.gif'

function onFileChange(e: Event) {
  const input = e.target as HTMLInputElement
  const file = input.files?.[0]
  if (!file) return

  error.value = ''
  fileName.value = file.name

  const ext = file.name.split('.').pop()?.toLowerCase()
  if (ext === 'json') {
    fileType.value = 'lottie'
    readJsonFile(file)
  } else if (ext === 'gif') {
    fileType.value = 'gif'
    readImageFile(file)
  } else if (['png', 'jpg', 'jpeg'].includes(ext ?? '')) {
    fileType.value = 'image'
    readImageFile(file)
  } else {
    error.value = '不支持的文件格式，请上传 .json / .png / .jpg / .gif 文件'
    return
  }

  // 编辑模式下自动填充名称
  if (!isEdit.value && !form.value.name.trim()) {
    form.value.name = file.name.replace(/\.[^.]+$/, '')
  }
}

function readJsonFile(file: File) {
  const reader = new FileReader()
  reader.onload = () => {
    try {
      const json = JSON.parse(reader.result as string)
      fileData.value = json
    } catch {
      error.value = 'JSON 文件解析失败，请检查文件格式'
      fileData.value = null
    }
  }
  reader.readAsText(file)
}

function readImageFile(file: File) {
  const reader = new FileReader()
  reader.onload = () => {
    fileData.value = reader.result as string
  }
  reader.readAsDataURL(file)
}

function clearFile() {
  fileData.value = null
  fileName.value = ''
  fileType.value = 'lottie'
  error.value = ''
  if (fileInput.value) {
    fileInput.value.value = ''
  }
}

async function save() {
  if (!form.value.name.trim() || !fileData.value) return
  saving.value = true
  error.value = ''
  try {
    const id = props.editPetId || generateCustomPetId()
    const pet: CustomPet = {
      id,
      name: form.value.name.trim(),
      description: form.value.description.trim(),
      type: fileType.value,
      data: fileData.value,
      animation: form.value.animation,
    }
    await customPetSave(pet)
    await loadCustomPets()
    emit('saved', id)
  } catch (e: any) {
    error.value = String(e?.message ?? e)
  } finally {
    saving.value = false
  }
}

// 编辑模式：填充现有数据
if (props.editPet) {
  form.value.name = props.editPet.name
  form.value.description = props.editPet.description
  form.value.animation = props.editPet.imageAnimation ?? 'breath'
  // 编辑模式不重新加载文件数据，只编辑元信息
}

function handleCancel() {
  // 清空所有数据
  form.value.name = ''
  form.value.description = ''
  form.value.animation = 'breath'
  fileData.value = null
  fileName.value = ''
  fileType.value = 'lottie'
  error.value = ''
  if (fileInput.value) {
    fileInput.value.value = ''
  }
  emit('cancel')
}
</script>

<template>
  <div class="pet-editor">
    <!-- 文件上传 -->
    <div v-if="!isEdit" class="pet-upload-area">
      <input
        ref="fileInput"
        type="file"
        :accept="acceptTypes"
        class="pet-file-input"
        @change="onFileChange"
      />
      <button
        class="settings-btn pet-upload-btn"
        @click="fileInput?.click()"
      >
        {{ hasFile ? '重新选择文件' : '选择文件' }}
      </button>
      <span v-if="fileName" class="pet-file-name">{{ fileName }}</span>
      <button v-if="hasFile" class="settings-btn pet-clear-btn" @click="clearFile">
        清除
      </button>
    </div>

    <!-- 文件预览 -->
    <div v-if="hasFile" class="pet-preview">
      <div class="pet-preview-box">
        <img
          v-if="fileType !== 'lottie'"
          class="pet-preview-img"
          :src="typeof fileData === 'string' ? fileData : ''"
          alt=""
        />
        <span v-else class="pet-preview-lottie">Lottie 动画</span>
      </div>
      <span class="pet-preview-type">
        {{ fileType === 'lottie' ? 'Lottie' : fileType === 'gif' ? 'GIF' : '图片' }}
      </span>
    </div>

    <!-- 名称 -->
    <label class="settings-field">
      <span class="settings-field-label">名称</span>
      <input
        class="settings-input"
        type="text"
        v-model="form.name"
        placeholder="给宠物起个名字"
      />
    </label>

    <!-- 介绍 -->
    <label class="settings-field">
      <span class="settings-field-label">介绍</span>
      <input
        class="settings-input"
        type="text"
        v-model="form.description"
        placeholder="一句话描述"
      />
    </label>

    <!-- 图片动画效果 -->
    <label v-if="showAnimationSelect" class="settings-field">
      <span class="settings-field-label">动画效果</span>
      <CustomDropdown
        v-model="form.animation"
        :options="animationOptions"
      />
    </label>

    <!-- 错误提示 -->
    <p v-if="error" class="settings-msg settings-msg-fail">{{ error }}</p>

    <!-- 操作按钮 -->
    <div class="settings-actions" style="margin-top: 8px;">
      <button
        class="settings-btn settings-btn-primary"
        :disabled="saving || !form.name.trim() || (!isEdit && !hasFile)"
        @click="save"
      >
        {{ saving ? '保存中…' : '保存' }}
      </button>
      <button class="settings-btn" @click="handleCancel">
        取消
      </button>
    </div>

    <p class="settings-section-hint" style="margin-top: 8px;">
      支持 Lottie JSON、PNG、JPG、GIF 格式。图片可选动画效果。
    </p>
  </div>
</template>

<style scoped>
.pet-editor {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.pet-upload-area {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 4px;
}
.pet-file-input {
  display: none;
}
.pet-upload-btn {
  flex-shrink: 0;
}
.pet-file-name {
  font-size: 12px;
  color: var(--text-dim);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  max-width: 180px;
}
.pet-clear-btn {
  flex-shrink: 0;
  padding: 4px 8px;
  font-size: 11px;
}

.pet-preview {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-bottom: 4px;
}
.pet-preview-box {
  width: 56px;
  height: 56px;
  border-radius: 50%;
  background: var(--surface);
  border: var(--divider);
  display: flex;
  align-items: center;
  justify-content: center;
  overflow: hidden;
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.15);
}
.pet-preview-img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}
.pet-preview-lottie {
  font-size: 10px;
  color: var(--text-dim);
}
.pet-preview-type {
  font-size: 11px;
  color: var(--text-dim);
}
</style>
