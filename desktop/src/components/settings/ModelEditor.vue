<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useConfigStore } from '../../stores/config'
import CustomDropdown from '../ui/CustomDropdown.vue'

const props = defineProps<{ profileId: string }>()
const emit = defineEmits<{ saved: [] }>()

const store = useConfigStore()
const isEdit = computed(() => !!props.profileId)

const form = ref({
  name: '',
  provider: 'deepseek' as 'deepseek' | 'openai' | 'custom',
  baseUrl: 'https://api.deepseek.com',
  model: 'deepseek-chat',
  apiKey: '',
  wireApi: 'chat' as 'chat' | 'responses' | 'anthropic',
})
const showKey = ref(false)
const keyDirty = ref(false)

const testState = ref<'idle' | 'testing' | 'ok' | 'fail'>('idle')
const testMessage = ref('')
const ccImportState = ref<'idle' | 'importing' | 'ok' | 'fail'>('idle')
const ccImportMessage = ref('')
const saving = ref(false)

const PRESETS: Record<string, { baseUrl: string; model: string }> = {
  deepseek: { baseUrl: 'https://api.deepseek.com', model: 'deepseek-chat' },
  openai: { baseUrl: 'https://api.openai.com', model: 'gpt-4o-mini' },
  custom: { baseUrl: '', model: '' },
}

const providerOptions = [
  { value: 'deepseek', label: 'DeepSeek' },
  { value: 'openai', label: 'OpenAI' },
  { value: 'custom', label: '自定义（OpenAI 兼容）' },
]

const wireApiOptions = [
  { value: 'chat', label: 'Chat Completions（/v1/chat/completions，默认）' },
  { value: 'responses', label: 'Responses（/v1/responses，Codex 协议）' },
  { value: 'anthropic', label: 'Anthropic Messages（/v1/messages，Claude 协议）' },
]

function onProviderChange(next: string) {
  if (next === 'custom') return
  const preset = PRESETS[next]
  if (!preset) return
  form.value.baseUrl = preset.baseUrl
  form.value.model = preset.model
}

onMounted(() => {
  if (props.profileId) {
    const p = store.config.llmProfiles?.find(p => p.id === props.profileId)
    if (p) {
      form.value.name = p.name
      form.value.provider = p.provider
      form.value.baseUrl = p.baseUrl
      form.value.model = p.model
      form.value.wireApi = p.wireApi ?? 'chat'
    }
  }
})

async function save() {
  if (!form.value.name.trim()) return
  saving.value = true
  try {
    const id = props.profileId || genId()
    const remote = await invoke<any>('llm_profile_upsert', {
      profileId: id,
      name: form.value.name.trim(),
      provider: form.value.provider,
      baseUrl: form.value.baseUrl,
      model: form.value.model,
      apiKey: keyDirty.value ? form.value.apiKey : '',
      wireApi: form.value.wireApi,
    })
    store.applyLlmProfile(remote)
    emit('saved')
  } catch (e) {
    console.error('保存模型配置失败:', e)
  } finally {
    saving.value = false
  }
}

async function testConnection() {
  testState.value = 'testing'
  testMessage.value = ''
  try {
    const r = await invoke<any>('llm_profile_test', {
      profileId: props.profileId || null,
      provider: form.value.provider,
      baseUrl: form.value.baseUrl,
      model: form.value.model,
      apiKey: keyDirty.value ? form.value.apiKey : '',
      allowSavedKeyWhenEmpty: !keyDirty.value,
      wireApi: form.value.wireApi,
    })
    if (r?.text) {
      testState.value = 'ok'
      testMessage.value = `连通：${r.model ?? form.value.model} -> "${String(r.text).slice(0, 40)}"`
    } else {
      testState.value = 'fail'
      testMessage.value = '调用成功，但没有文本返回'
    }
  } catch (e: any) {
    testState.value = 'fail'
    testMessage.value = String(e?.message ?? e)
  }
}

async function importFromCcSwitch() {
  ccImportState.value = 'importing'
  ccImportMessage.value = ''
  try {
    const r = await invoke<{ success: boolean; data?: any; error?: string }>('tool_execute', {
      name: 'cc_switch_import',
      input: {},
    })
    if (!r.success || !r.data) {
      ccImportState.value = 'fail'
      ccImportMessage.value = r.error || '调用失败'
      return
    }
    const data = r.data as {
      found: boolean
      reason?: string
      provider?: { name: string; apiKey: string; baseUrl: string; model: string; wireApi?: string }
    }
    if (!data.found || !data.provider) {
      ccImportState.value = 'fail'
      ccImportMessage.value = data.reason || '未找到 CC Switch 配置'
      return
    }
    form.value.provider = 'custom'
    form.value.apiKey = data.provider.apiKey
    form.value.baseUrl = data.provider.baseUrl
    form.value.model = data.provider.model
    form.value.wireApi = (['responses', 'anthropic'] as const).includes(data.provider.wireApi as any)
      ? (data.provider.wireApi as 'responses' | 'anthropic')
      : 'chat'
    keyDirty.value = true
    if (!form.value.name.trim()) {
      form.value.name = data.provider.name
    }
    ccImportState.value = 'ok'
    let msg = `已导入「${data.provider.name}」：${data.provider.model}`
    if (data.provider.wireApi === 'responses') {
      msg += '\n✓ 检测到 Codex responses API，已切到 /v1/responses 协议'
    }
    if (data.provider.wireApi === 'anthropic') {
      msg += '\n✓ 检测到 Anthropic 协议，已切到 /v1/messages'
    }
    ccImportMessage.value = msg
  } catch (e: any) {
    ccImportState.value = 'fail'
    ccImportMessage.value = String(e?.message ?? e)
  }
}

function genId(): string {
  return 'lp' + Date.now().toString(36) + Math.random().toString(36).slice(2, 6)
}
</script>

<template>
  <div class="model-editor">
    <label class="settings-field">
      <span class="settings-field-label">名称</span>
      <input class="settings-input" type="text" v-model="form.name"
        placeholder="比如：工作用 DeepSeek" />
    </label>
    <label class="settings-field">
      <span class="settings-field-label">服务商</span>
      <CustomDropdown
        :model-value="form.provider"
        :options="providerOptions"
        @update:model-value="(v) => { form.provider = v as any; onProviderChange(v) }"
      />
    </label>
    <label class="settings-field">
      <span class="settings-field-label">地址</span>
      <input class="settings-input" type="url" placeholder="https://api.deepseek.com"
        v-model="form.baseUrl" />
    </label>
    <label class="settings-field">
      <span class="settings-field-label">模型</span>
      <input class="settings-input" type="text" placeholder="deepseek-chat"
        v-model="form.model" />
    </label>
    <label class="settings-field">
      <span class="settings-field-label">apiKey</span>
      <input class="settings-input"
        :type="showKey ? 'text' : 'password'"
        :placeholder="isEdit ? '已保存，留空则不变' : 'sk-...'"
        v-model="form.apiKey"
        @input="keyDirty = true" />
      <button class="settings-btn" style="margin-left:6px;padding:4px 8px;"
        @click="showKey = !showKey">
        {{ showKey ? '隐藏' : '显示' }}
      </button>
    </label>
    <label class="settings-field">
      <span class="settings-field-label">协议</span>
      <CustomDropdown
        v-model="form.wireApi"
        :options="wireApiOptions"
      />
    </label>

    <div class="settings-actions" style="margin-top: 8px;">
      <button class="settings-btn settings-btn-primary"
        :disabled="saving || !form.name.trim()"
        @click="save">
        {{ saving ? '保存中…' : '保存' }}
      </button>
      <button class="settings-btn"
        :disabled="testState === 'testing'"
        @click="testConnection">
        {{ testState === 'testing' ? '测试中…' : '测试连接' }}
      </button>
      <button class="settings-btn"
        :disabled="ccImportState === 'importing'"
        @click="importFromCcSwitch"
        title="从 ~/.cc-switch/ 读取当前激活的 Codex（OpenAI）provider 一键填入">
        {{ ccImportState === 'importing' ? '导入中…' : '从 CC Switch 导入' }}
      </button>
    </div>
    <p v-if="testMessage" class="settings-msg" :class="`settings-msg-${testState}`">{{ testMessage }}</p>
    <p v-if="ccImportMessage" class="settings-msg"
      :class="`settings-msg-${ccImportState === 'importing' ? 'testing' : ccImportState}`">
      {{ ccImportMessage }}
    </p>
    <p class="settings-section-hint" style="margin-top: 8px;">
      测试连接只验证当前表单，不需要先保存。保存会写入模型列表；启用模型请在列表里点击切换。
    </p>
  </div>
</template>

<style scoped>
.model-editor {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
</style>
