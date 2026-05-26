<script setup lang="ts">
// LLM 接入 section：服务商 / 地址 / 模型 / apiKey / 协议 + 测试 + 从 CC Switch 导入。
//
// apiKey 明文存 ~/.jarvis/config.json，不写密钥链——用户偏好（隐私无所谓，干就完了）。

import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useConfigStore } from '../../stores/config'

const store = useConfigStore()

const showKey = ref(false)
const testState = ref<'idle' | 'testing' | 'ok' | 'fail'>('idle')
const testMessage = ref('')
const ccImportState = ref<'idle' | 'importing' | 'ok' | 'fail'>('idle')
const ccImportMessage = ref('')

// 切换 provider 时把 baseUrl/model 顺手填成厂商默认值
const PRESETS: Record<string, { baseUrl: string; model: string }> = {
  deepseek: { baseUrl: 'https://api.deepseek.com', model: 'deepseek-chat' },
  openai: { baseUrl: 'https://api.openai.com', model: 'gpt-4o-mini' },
  custom: { baseUrl: '', model: '' },
}
function onProviderChange(next: string) {
  if (next === 'custom') return
  const preset = PRESETS[next]
  if (!preset) return
  store.config.llm.baseUrl = preset.baseUrl
  store.config.llm.model = preset.model
}

async function testConnection() {
  testState.value = 'testing'
  testMessage.value = ''
  // 等一次保存（store watcher 250ms 防抖），确保最新 apiKey 已落盘
  await new Promise(r => setTimeout(r, 400))
  try {
    const r = await invoke<{ success: boolean; data?: any; error?: string }>('tool_execute', {
      name: 'ask-llm',
      input: {
        messages: [
          { role: 'system', content: '只回一个字：好' },
          { role: 'user', content: 'ping' },
        ],
        maxTokens: 8,
      },
    })
    if (r.success && r.data?.text) {
      testState.value = 'ok'
      testMessage.value = `连通：${r.data.model ?? store.config.llm.model} → “${String(r.data.text).slice(0, 40)}”`
    } else {
      testState.value = 'fail'
      testMessage.value = r.error || '调用失败：无文本返回'
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
    // CC Switch 的 baseUrl 一般不是 OpenAI/DeepSeek，统一切 custom
    store.config.llm.provider = 'custom'
    store.config.llm.apiKey = data.provider.apiKey
    store.config.llm.baseUrl = data.provider.baseUrl
    store.config.llm.model = data.provider.model
    store.config.llm.wireApi = data.provider.wireApi === 'responses' ? 'responses' : 'chat'
    ccImportState.value = 'ok'
    let msg = `已导入「${data.provider.name}」：${data.provider.model}`
    if (data.provider.wireApi === 'responses') {
      msg += '\n✓ 检测到 Codex responses API，已切到 /v1/responses 协议'
    }
    ccImportMessage.value = msg
  } catch (e: any) {
    ccImportState.value = 'fail'
    ccImportMessage.value = String(e?.message ?? e)
  }
}
</script>

<template>
  <section class="settings-section">
    <h3 class="settings-section-title">LLM 接入</h3>
    <p class="settings-section-hint">日报、风险摘要、commit↔任务评分可选调用。apiKey 明文存 config.json，不写密钥链</p>
    <label class="settings-field">
      <span class="settings-field-label">服务商</span>
      <select class="settings-input"
        :value="store.config.llm.provider"
        @change="(e) => { const v = (e.target as HTMLSelectElement).value as any; store.config.llm.provider = v; onProviderChange(v) }">
        <option value="deepseek">DeepSeek</option>
        <option value="openai">OpenAI</option>
        <option value="custom">自定义（OpenAI 兼容）</option>
      </select>
    </label>
    <label class="settings-field">
      <span class="settings-field-label">地址</span>
      <input class="settings-input" type="url" placeholder="https://api.deepseek.com"
        v-model="store.config.llm.baseUrl" />
    </label>
    <label class="settings-field">
      <span class="settings-field-label">模型</span>
      <input class="settings-input" type="text" placeholder="deepseek-chat"
        v-model="store.config.llm.model" />
    </label>
    <label class="settings-field">
      <span class="settings-field-label">apiKey</span>
      <input class="settings-input" :type="showKey ? 'text' : 'password'"
        placeholder="sk-..." v-model="store.config.llm.apiKey" />
      <button class="settings-btn" style="margin-left:6px;padding:4px 8px;"
        @click="showKey = !showKey">
        {{ showKey ? '隐藏' : '显示' }}
      </button>
    </label>
    <label class="settings-field">
      <span class="settings-field-label">协议</span>
      <select class="settings-input" v-model="store.config.llm.wireApi">
        <option value="chat">Chat Completions（/v1/chat/completions，默认）</option>
        <option value="responses">Responses（/v1/responses，Codex 协议）</option>
      </select>
    </label>
    <div class="settings-actions">
      <button class="settings-btn settings-btn-primary"
        :disabled="testState === 'testing' || !store.config.llm.apiKey"
        @click="testConnection">
        {{ testState === 'testing' ? '测试中…' : '测试连接' }}
      </button>
      <button class="settings-btn"
        :disabled="ccImportState === 'importing'"
        @click="importFromCcSwitch"
        title="从 ~/.cc-switch/ 读取当前激活的 Codex（OpenAI）provider 一键填入">
        {{ ccImportState === 'importing' ? '导入中…' : '🔄 从 CC Switch 导入' }}
      </button>
    </div>
    <p v-if="testMessage" class="settings-msg" :class="`settings-msg-${testState}`">{{ testMessage }}</p>
    <p v-if="ccImportMessage" class="settings-msg"
      :class="`settings-msg-${ccImportState === 'importing' ? 'testing' : ccImportState}`">
      {{ ccImportMessage }}
    </p>
  </section>
</template>
