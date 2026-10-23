<script setup lang="ts">
// 宠物形象选择：支持内置宠物、自定义宠物和宠物包 v2（多动作素材包）。
// 自定义宠物可通过上传 Lottie JSON / 图片 / GIF 添加；
// 宠物包通过选整个文件夹导入（idle/walk/held/sleep + 任意小动作）。
//
// 切换后立刻生效（App.vue 里 PetAvatar 监听 petId 变化重载动画）。
// 宠物大小滑杆只改 CSS 变量 --pet-size（config.petSizePx），同样实时生效、无重载。

import { ref, onMounted, nextTick } from 'vue'
import { useConfigStore, PET_SIZE_MIN, PET_SIZE_MAX } from '../../stores/config'
import {
  PET_CATEGORY_LABELS,
  getAllPets,
  getCustomPets,
  getPackPets,
  loadCustomPets,
  loadPackPets,
  clearPackClipCache,
  isPackPetId,
  MASCOT_PET_ID,
  type PetInfo,
} from '../../petManifest'
import { customPetDelete } from '../../api/customPet'
import {
  petPackDelete,
  petPackSave,
  sanitizePackIdLocal,
  type PetPackClipType,
  type PetPackClips,
  type PetPackFacing,
  type PetPackFileIn,
} from '../../api/petPack'
import CustomPetEditor from './CustomPetEditor.vue'
import CustomDropdown from '../ui/CustomDropdown.vue'
import ToggleSwitch from '../ui/ToggleSwitch.vue'
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

// 宠物包是否有 idle 以外的动作（「多动作」徽标用）
function packIsMultiAction(pet: PetInfo): boolean {
  const clips = pet.pack?.clips
  if (!clips) return false
  return !!(clips.walk || clips.held || clips.sleep) || (clips.actions?.length ?? 0) > 0
}

// 刷新宠物选项列表
async function refreshOptions() {
  await Promise.all([loadCustomPets(), loadPackPets()])
  // mascot 在 PETS 首位（getAllPets 保持数组序）→ 内置列表自动排第一；
  // 「新」徽标以文本缀进 label（DropdownOption 无徽标字段，下拉项够用）
  petOptions.value = getAllPets().map(p => ({
    value: p.id,
    label: `${p.name}${p.id === MASCOT_PET_ID ? '（新）' : ''}${p.description ? ' · ' + p.description : ''}${isPackPetId(p.id) && packIsMultiAction(p) ? ' · 多动作' : ''}`,
    group: PET_CATEGORY_LABELS[p.category],
  }))
}

// 加载自定义宠物与宠物包
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
  refreshOptions().then(async () => {
    // 等待 Vue 响应式更新完成后再设置 petId
    await nextTick()
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

// ===== 宠物包 v2 导入 =====
// 选整个文件夹（webkitdirectory）→ 按文件名关键词预填动作槽 → 逐文件可改指派
// → 前端读字节转 base64 → pet_pack_save 整包落盘 ~/.jarvis/pets/<id>/。

type PackSlot = 'idle' | 'walk' | 'held' | 'sleep' | 'action' | 'skip'

interface PackImportRow {
  file: File
  name: string          // 上传用 basename（= file.name，与 clips[].file 一致）
  ext: string
  slot: PackSlot
  /** slot=action 时的动作名（点播菜单显示），默认文件名去扩展 */
  actionName: string
}

const SLOT_OPTIONS: Array<{ value: PackSlot; label: string }> = [
  { value: 'idle', label: '待机' },
  { value: 'walk', label: '走路' },
  { value: 'held', label: '被拎起' },
  { value: 'sleep', label: '睡觉' },
  { value: 'action', label: '小动作' },
  { value: 'skip', label: '不导入' },
]

const PACK_SUPPORTED_EXTS = ['gif', 'webm', 'png', 'jpg', 'jpeg']

const showPackImport = ref(false)
const packDirInput = ref<HTMLInputElement | null>(null)
const packRows = ref<PackImportRow[]>([])
const packForm = ref({ id: '', name: '', facing: 'left' as PetPackFacing })
// spritesheet 素材（png）的帧参数：整包一套（多数素材包全包统一帧规格）
const spriteForm = ref({ frameW: 64, frameH: 64, fps: 8, frames: 4, columns: 4 })
const packSaving = ref(false)
const packError = ref('')

/** 按文件名关键词预填动作槽（PRD 映射表；大小写不敏感，先判长词防误切） */
function guessSlot(fileName: string): Exclude<PackSlot, 'skip'> {
  const lower = fileName.toLowerCase()
  if (lower.includes('idle') || lower.includes('待机')) return 'idle'
  if (lower.includes('walk') || lower.includes('走') || lower.includes('移动')) return 'walk'
  if (lower.includes('held') || lower.includes('拎')) return 'held'
  if (lower.includes('sleep') || lower.includes('睡') || lower.includes('打瞌睡')) return 'sleep'
  return 'action'
}

function openPackImport() {
  packRows.value = []
  packError.value = ''
  packForm.value = { id: '', name: '', facing: 'left' }
  // 直接拉起系统文件夹选择器（按钮点击 = 用户手势）；选完在 onPackDirChange 里进指派界面
  packDirInput.value?.click()
}

function closePackImport() {
  showPackImport.value = false
  packRows.value = []
  packError.value = ''
  if (packDirInput.value) packDirInput.value.value = ''
}

function onPackDirChange(e: Event) {
  const input = e.target as HTMLInputElement
  const files = Array.from(input.files ?? [])
  packError.value = ''
  const takenSlots = new Set<Exclude<PackSlot, 'skip' | 'action'>>()
  const rows: PackImportRow[] = []
  let rootName = ''
  for (const file of files) {
    const rel = (file as File & { webkitRelativePath?: string }).webkitRelativePath ?? ''
    if (!rootName && rel.includes('/')) rootName = rel.split('/')[0]
    const ext = file.name.split('.').pop()?.toLowerCase() ?? ''
    if (file.name.startsWith('.') || !PACK_SUPPORTED_EXTS.includes(ext)) continue
    let slot = guessSlot(file.name)
    // 固定槽位先到先得，重复命中的落到小动作（界面里仍可手动改）
    if (slot !== 'action') {
      if (takenSlots.has(slot)) slot = 'action'
      else takenSlots.add(slot)
    }
    rows.push({ file, name: file.name, ext, slot, actionName: file.name.replace(/\.[^.]+$/, '') })
  }
  packRows.value = rows
  if (!packForm.value.id) packForm.value.id = sanitizePackIdLocal(rootName || 'pack')
  if (!packForm.value.name) packForm.value.name = rootName || packForm.value.id
  input.value = '' // 清空以便重复选同一文件夹也触发 change
  // 一个可用素材都没有：不进指派界面，直接提示
  if (rows.length === 0) {
    packError.value = '所选文件夹里没有可用的素材文件（支持 GIF / WebM / PNG / JPG）'
    return
  }
  showPackImport.value = true
}

function fileToBase64(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader()
    reader.onload = () => {
      const result = String(reader.result ?? '')
      const comma = result.indexOf(',')
      // readAsDataURL 产出 data:...;base64,XXXX —— 只取 base64 正文
      resolve(comma >= 0 ? result.slice(comma + 1) : result)
    }
    reader.onerror = () => reject(reader.error ?? new Error(`读取文件失败: ${file.name}`))
    reader.readAsDataURL(file)
  })
}

function packClipFor(row: PackImportRow): { file: string; type: PetPackClipType; frameW?: number; frameH?: number; fps?: number; frames?: number; columns?: number } {
  const type: PetPackClipType = row.ext === 'gif' ? 'gif' : row.ext === 'webm' ? 'webm' : 'spritesheet'
  if (type !== 'spritesheet') return { file: row.name, type }
  return { file: row.name, type, ...spriteForm.value }
}

async function savePack() {
  packError.value = ''
  const assigned = packRows.value.filter(r => r.slot !== 'skip')
  if (!assigned.some(r => r.slot === 'idle')) {
    packError.value = '必须指派一个「待机（idle）」文件'
    return
  }
  packSaving.value = true
  try {
    const clips: PetPackClips = { idle: packClipFor(assigned.find(r => r.slot === 'idle')!) }
    const files: PetPackFileIn[] = []
    for (const row of assigned) {
      files.push({ name: row.name, dataBase64: await fileToBase64(row.file) })
      if (row.slot === 'idle') continue
      if (row.slot === 'action') {
        const action = { name: row.actionName.trim() || row.name, weight: 1, ...packClipFor(row) }
        ;(clips.actions ??= []).push(action)
      } else {
        clips[row.slot] = packClipFor(row)
      }
    }
    const packId = sanitizePackIdLocal(packForm.value.id)
    await petPackSave({
      id: packId,
      name: packForm.value.name.trim() || packId,
      facing: packForm.value.facing,
      clips,
      files,
    })
    // 重新导入覆盖旧包：素材缓存必须作废，否则渲染的还是旧 data URL
    clearPackClipCache(packId)
    closePackImport()
    refreshOptions().then(async () => {
      await nextTick()
      store.config.petId = `pack-${packId}`
    })
  } catch (e) {
    packError.value = String(e)
  } finally {
    packSaving.value = false
  }
}

// ===== 宠物包删除 =====

const showPackDeleteConfirm = ref(false)
const deletingPackId = ref('')
const deletingPackName = ref('')

function openPackDeleteConfirm(packId: string, name: string) {
  deletingPackId.value = packId
  deletingPackName.value = name
  showPackDeleteConfirm.value = true
}

function closePackDeleteConfirm() {
  showPackDeleteConfirm.value = false
  deletingPackId.value = ''
  deletingPackName.value = ''
}

async function confirmPackDelete() {
  const packId = deletingPackId.value
  closePackDeleteConfirm()
  try {
    // 删除的是当前宠物则先切回默认
    if (store.config.petId === `pack-${packId}`) {
      store.config.petId = 'robo'
    }
    await petPackDelete(packId)
    clearPackClipCache(packId)
    await refreshOptions()
  } catch (e) {
    console.error('删除宠物包失败:', e)
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
      <button class="settings-btn pet-pack-btn" @click="openPackImport">
        导入宠物包
      </button>
    </div>
    <!-- 选文件夹入口：webkitdirectory 拿整包素材，change 里按关键词预填动作槽 -->
    <input
      ref="packDirInput"
      type="file"
      webkitdirectory
      multiple
      class="pet-file-input"
      @change="onPackDirChange"
    />

    <!-- 宠物选择下拉 -->
    <label class="settings-field">
      <span class="settings-field-label">选什么</span>
      <CustomDropdown
        v-model="store.config.petId"
        :options="petOptions"
        placeholder="请选择宠物"
      />
    </label>

    <!-- 宠物大小滑杆：写 config.petSizePx，avatar 窗经 CSS 变量即时生效。
         range 48–176 step 4，UI 不暴露越界输入（钳制兜底在 config.load）；
         越界值绑定到 :min/:max 常量而非字面量，与 stores/config.ts 单一来源对齐 -->
    <div class="settings-field">
      <span class="settings-field-label">宠物大小</span>
      <span class="size-slider-label" aria-hidden="true">小</span>
      <input
        v-model.number="store.config.petSizePx"
        class="size-slider"
        type="range"
        :min="PET_SIZE_MIN"
        :max="PET_SIZE_MAX"
        step="4"
        aria-label="宠物大小（像素）"
      />
      <span class="size-slider-label" aria-hidden="true">大</span>
      <span class="size-slider-value">{{ store.config.petSizePx }}px</span>
    </div>

    <!-- 桌面漫步（E2 漫游引擎总开关）：仅宠物包形象生效；关闭 = 引擎完全不启动，
         与无漫游版本行为一致。写 config.petRoaming，avatar 窗经 config-changed 同步 -->
    <div class="settings-field">
      <span class="settings-field-label">桌面漫步</span>
      <ToggleSwitch
        v-model="store.config.petRoaming"
        label="宠物会自己在桌面上散步、转身、做小动作，犯困了还会睡一会儿"
      />
    </div>

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

    <!-- 宠物包管理列表：只能删不能编辑（改动作 = 重新导入文件夹覆盖） -->
    <div v-if="getPackPets().length > 0" class="custom-pets-list">
      <div class="custom-pets-header">宠物包</div>
      <div
        v-for="pack in getPackPets()"
        :key="pack.id"
        class="custom-pet-item"
        :class="{ active: store.config.petId === pack.id }"
      >
        <div class="custom-pet-info">
          <div class="pack-name-row">
            <span class="custom-pet-name">{{ pack.name }}</span>
            <span v-if="packIsMultiAction(pack)" class="pack-badge">多动作</span>
          </div>
          <span class="custom-pet-desc">{{ pack.description }}</span>
        </div>
        <div class="custom-pet-actions">
          <button
            class="settings-btn settings-btn-sm settings-btn-danger"
            @click="openPackDeleteConfirm(pack.id.replace(/^pack-/, ''), pack.name)"
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

    <!-- 宠物包导入指派弹窗：逐文件指派动作槽，spritesheet 参数整包一套 -->
    <div v-if="showPackImport" class="pet-editor-overlay">
      <div class="pet-editor-modal">
        <h4 class="pet-editor-title">导入宠物包</h4>

        <label class="settings-field">
          <span class="settings-field-label">ID（目录名，仅字母数字-_）</span>
          <input v-model="packForm.id" class="settings-input" type="text" placeholder="my-pet" />
        </label>
        <label class="settings-field">
          <span class="settings-field-label">名称</span>
          <input v-model="packForm.name" class="settings-input" type="text" placeholder="给宠物起个名字" />
        </label>
        <label class="settings-field">
          <span class="settings-field-label">素材默认朝向</span>
          <select v-model="packForm.facing" class="pack-slot-select">
            <option value="left">朝左</option>
            <option value="right">朝右</option>
          </select>
        </label>

        <!-- spritesheet 帧参数：png 素材按雪碧图渲染时用 -->
        <div class="settings-field">
          <span class="settings-field-label">雪碧图帧参数（PNG 素材用）</span>
          <div class="pack-sprite-grid">
            <label class="pack-sprite-field">帧宽<input v-model.number="spriteForm.frameW" type="number" min="1" /></label>
            <label class="pack-sprite-field">帧高<input v-model.number="spriteForm.frameH" type="number" min="1" /></label>
            <label class="pack-sprite-field">帧率<input v-model.number="spriteForm.fps" type="number" min="1" /></label>
            <label class="pack-sprite-field">帧数<input v-model.number="spriteForm.frames" type="number" min="1" /></label>
            <label class="pack-sprite-field">列数<input v-model.number="spriteForm.columns" type="number" min="1" /></label>
          </div>
        </div>

        <div class="pack-files-list">
          <div v-for="row in packRows" :key="row.name" class="pack-file-row">
            <span class="pack-file-name" :title="row.name">{{ row.name }}</span>
            <select v-model="row.slot" class="pack-slot-select" :class="{ 'slot-action': row.slot === 'action' }">
              <option v-for="opt in SLOT_OPTIONS" :key="opt.value" :value="opt.value">{{ opt.label }}</option>
            </select>
            <input
              v-if="row.slot === 'action'"
              v-model="row.actionName"
              class="settings-input pack-action-name"
              type="text"
              placeholder="动作名（点播菜单显示）"
            />
          </div>
        </div>

        <p v-if="packError" class="settings-msg settings-msg-fail">{{ packError }}</p>

        <div class="settings-actions" style="margin-top: 8px;">
          <button
            class="settings-btn settings-btn-primary"
            :disabled="packSaving || packRows.length === 0"
            @click="savePack"
          >
            {{ packSaving ? '保存中…' : '保存' }}
          </button>
          <button class="settings-btn" @click="closePackImport">取消</button>
        </div>
      </div>
    </div>

    <!-- 宠物包删除确认弹窗 -->
    <div v-if="showPackDeleteConfirm" class="pet-modal-mask" @click.self="closePackDeleteConfirm">
      <div class="pet-modal">
        <h4 class="pet-modal-title">删除宠物包</h4>
        <p class="pet-modal-body">
          确定要删除宠物包「{{ deletingPackName }}」吗？整个素材目录将被删除，此操作不可恢复。
        </p>
        <div class="pet-modal-actions">
          <button class="settings-btn" @click="closePackDeleteConfirm">取消</button>
          <button class="settings-btn settings-btn-danger" @click="confirmPackDelete">删除</button>
        </div>
      </div>
    </div>

    <p class="settings-section-hint">
      支持 Lottie JSON、PNG、JPG、GIF 格式。自定义形象存储在本地 ~/.jarvis/custom-pets/。
      宠物包支持 GIF / WebM / 雪碧图 PNG 多动作素材，存储在本地 ~/.jarvis/pets/。
    </p>
  </section>
</template>

<style scoped>
.pet-actions {
  margin-bottom: 8px;
  display: flex;
  gap: 8px;
}
.pet-pack-btn {
  flex-shrink: 0;
}
.pet-file-input {
  display: none;
}

/* 宠物包行内徽标与名称排布 */
.pack-name-row {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
}
.pack-badge {
  flex-shrink: 0;
  font-size: 10px;
  padding: 1px 6px;
  border-radius: var(--radius-sm);
  color: var(--accent-text);
  background: color-mix(in srgb, var(--accent) 12%, transparent);
  border: 1px solid var(--accent-border);
}

/* 导入指派弹窗：文件行（名称 + 槽位下拉 + 动作名） */
.pack-files-list {
  margin-top: 8px;
  max-height: 220px;
  overflow-y: auto;
  border: var(--divider);
  border-radius: 8px;
}
.pack-file-row {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 5px 8px;
  border-bottom: var(--divider);
}
.pack-file-row:last-child {
  border-bottom: none;
}
.pack-file-name {
  flex: 1;
  min-width: 0;
  font-size: 11px;
  color: var(--text);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
/* 原生 select 下拉：OS 弹层不吃大部分 CSS（theming spec 允许的既定例外） */
.pack-slot-select {
  flex-shrink: 0;
  width: 84px;
  height: 24px;
  font-size: 11px;
  font-family: inherit;
  color: #222;
  background: #fff;
  border: 1px solid var(--border, #444);
  border-radius: 6px;
  padding: 0 4px;
}
.pack-action-name {
  width: 120px;
  flex-shrink: 0;
  height: 24px;
  font-size: 11px;
  padding: 0 6px;
}
.pack-sprite-grid {
  display: grid;
  grid-template-columns: repeat(5, 1fr);
  gap: 6px;
  max-width: 320px;
}
.pack-sprite-field {
  display: flex;
  flex-direction: column;
  gap: 2px;
  font-size: 10px;
  color: var(--text-dim);
}
.pack-sprite-field input {
  width: 100%;
  height: 22px;
  font-size: 11px;
  padding: 0 4px;
  color: #222;
  background: #fff;
  border: 1px solid var(--border, #444);
  border-radius: 6px;
}

/* 宠物大小滑杆：沿用 .settings-field 行布局（label 80px + 控件区），
   原生 range 用 accent-color 着色跟主题走，不自绘轨道 */
.size-slider {
  flex: 1;
  min-width: 0;
  height: 18px;
  margin: 0;
  accent-color: var(--accent);
}
.size-slider:focus-visible {
  outline: none;
}
.size-slider-label {
  flex-shrink: 0;
  font-size: 10px;
  color: var(--text-dim);
}
.size-slider-value {
  flex-shrink: 0;
  min-width: 36px;
  text-align: right;
  font-size: 11px;
  /* tabular-nums：拖动时数字宽度不抖 */
  font-variant-numeric: tabular-nums;
  color: var(--text);
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
