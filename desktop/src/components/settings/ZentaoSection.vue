<script setup lang="ts">
// 禅道连接 section：地址 / 账号 / 密码 + 测试连接 + 保存到 keychain。
//
// 密码框只在用户输入时有值；空提交时 zentao_test_connection 会自动回退到
// keychain 已存值（见 src-tauri/src/credentials.rs::zentao_test_connection）。

import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useConfigStore } from '../../stores/config'
import { normalizeZentaoBaseUrl } from '../../composables/zentaoUrl'

const store = useConfigStore()

const password = ref('')
const state = ref<'idle' | 'testing' | 'ok' | 'fail'>('idle')
const message = ref('')

async function testConnection() {
  // 测试前规范化 URL，并把清洗后的值写回 store（watch 触发持久化）
  const cleaned = normalizeZentaoBaseUrl(store.config.zentao.baseUrl)
  if (cleaned !== store.config.zentao.baseUrl) store.config.zentao.baseUrl = cleaned

  state.value = 'testing'
  message.value = ''
  try {
    const r = await invoke<{ ok: boolean; message: string }>('zentao_test_connection', {
      req: {
        baseUrl: store.config.zentao.baseUrl,
        account: store.config.zentao.account,
        password: password.value,
      },
    })
    state.value = r.ok ? 'ok' : 'fail'
    message.value = r.message
  } catch (e: any) {
    state.value = 'fail'
    message.value = String(e?.message ?? e)
  }
}

async function savePassword() {
  if (!store.config.zentao.account.trim()) {
    message.value = '请先填写禅道账号'
    state.value = 'fail'
    return
  }
  if (!password.value) {
    message.value = '请输入密码'
    state.value = 'fail'
    return
  }
  try {
    await invoke('credentials_set', {
      account: store.config.zentao.account,
      password: password.value,
    })
    state.value = 'ok'
    message.value = '密码已加密保存到系统密钥链'
    password.value = ''
  } catch (e: any) {
    state.value = 'fail'
    message.value = '保存密码失败：' + String(e?.message ?? e)
  }
}
</script>

<template>
  <section class="settings-section">
    <h3 class="settings-section-title">禅道连接</h3>
    <label class="settings-field">
      <span class="settings-field-label">地址</span>
      <input class="settings-input" type="url" placeholder="http://zentao.example.com/zentao"
        v-model="store.config.zentao.baseUrl" />
    </label>
    <label class="settings-field">
      <span class="settings-field-label">账号</span>
      <input class="settings-input" type="text" placeholder="你的禅道用户名"
        v-model="store.config.zentao.account" />
    </label>
    <label class="settings-field">
      <span class="settings-field-label">密码</span>
      <input class="settings-input" type="password" placeholder="留空表示不修改密钥链中的密码"
        v-model="password" />
    </label>
    <div class="settings-actions">
      <button class="settings-btn" :disabled="state === 'testing'" @click="testConnection">
        {{ state === 'testing' ? '测试中…' : '测试连接' }}
      </button>
      <button class="settings-btn settings-btn-primary" @click="savePassword">
        保存密码到密钥链
      </button>
    </div>
    <p v-if="message" class="settings-msg" :class="`settings-msg-${state}`">{{ message }}</p>
    <p class="settings-section-hint">密码不会写入任何文件，仅保存在系统密钥链中</p>
  </section>
</template>
