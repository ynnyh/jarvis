<script setup lang="ts">
// 首启欢迎引导。检测到 settings.zentao.baseUrl 空 OR settings.repoRoots 空时展示。
//
// 流程：
//   1. 欢迎介绍
//   2. 禅道地址 + 账号
//   3. 密码 + 测试连接
//   4. 选代码文件夹（可多选）
//   5. 完成（写 settings + 保存密码到密钥链）

import { ref, computed } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useConfigStore } from '../stores/config'
import { normalizeZentaoBaseUrl } from '../composables/zentaoUrl'

const emit = defineEmits<{
  (e: 'done'): void
}>()

const store = useConfigStore()

const step = ref(1)
const totalSteps = 5

// 暂存输入，最后一步才写 store/keychain
const baseUrl = ref(store.config.zentao.baseUrl || '')
const account = ref(store.config.zentao.account || '')
const password = ref('')

const repoRoots = ref<string[]>([...store.config.repoRoots])

const testState = ref<'idle' | 'testing' | 'ok' | 'fail'>('idle')
const testMessage = ref('')
const finishing = ref(false)

const canNext = computed(() => {
  if (step.value === 2) return baseUrl.value.trim().length > 0 && account.value.trim().length > 0
  if (step.value === 3) return testState.value === 'ok'
  if (step.value === 4) return repoRoots.value.length > 0
  return true
})

async function testConnection() {
  // 用户多半会从浏览器地址栏直接复制；先规范化再调，并把清洗后的值写回输入框
  // 让用户看到我们实际用的是什么 URL
  const cleaned = normalizeZentaoBaseUrl(baseUrl.value)
  if (cleaned !== baseUrl.value) baseUrl.value = cleaned

  testState.value = 'testing'
  testMessage.value = ''
  try {
    const r = await invoke<{ ok: boolean; message: string }>('zentao_test_connection', {
      req: { baseUrl: baseUrl.value, account: account.value, password: password.value },
    })
    testState.value = r.ok ? 'ok' : 'fail'
    testMessage.value = r.message
  } catch (e: any) {
    testState.value = 'fail'
    testMessage.value = String(e?.message ?? e)
  }
}

async function pickFolder() {
  const picked = await invoke<string | null>('pick_directory', {
    title: '选择本地代码根目录',
  })
  if (!picked) return
  if (!repoRoots.value.includes(picked)) repoRoots.value.push(picked)
}

function removeRoot(i: number) {
  repoRoots.value.splice(i, 1)
}

async function finish() {
  finishing.value = true
  try {
    // 1. 密码进密钥链
    await invoke('credentials_set', { account: account.value, password: password.value })
    // 2. settings 写回（baseUrl + account + repoRoots）
    store.config.zentao.baseUrl = normalizeZentaoBaseUrl(baseUrl.value)
    store.config.zentao.account = account.value.trim()
    store.config.repoRoots = [...repoRoots.value]
    // 3. 让 store 的 watch 把 config 写盘（save 是 250ms 防抖）
    await new Promise(r => setTimeout(r, 350))
    // 4. 重启 daemon —— 它启动时通过 env 拿 ZENTAO_PASSWORD，不重启拿不到新密码。
    //    旧 daemon 用启动时的（可能为空 / 旧密码）凭证调禅道会认证失败，UI 一进
    //    主界面就报 "ZenTao 认证失败"。
    try { await invoke('daemon_restart') } catch (e) {
      console.warn('daemon 重启失败（不影响 wizard 完成）:', e)
    }
    emit('done')
  } catch (e: any) {
    testState.value = 'fail'
    testMessage.value = '完成失败：' + String(e?.message ?? e)
  } finally {
    finishing.value = false
  }
}

function next() { if (canNext.value && step.value < totalSteps) step.value++ }
function prev() { if (step.value > 1) step.value-- }
</script>

<template>
  <div class="wizard-overlay">
    <div class="wizard">
      <header class="wizard-header">
        <div class="wizard-title">
          <span class="wizard-icon">🤖</span>
          <span>欢迎使用 Jarvis</span>
        </div>
        <div class="wizard-progress">
          <span v-for="i in totalSteps" :key="i" class="dot" :class="{ active: i <= step }" />
          <span class="step-text">{{ step }} / {{ totalSteps }}</span>
        </div>
      </header>

      <div class="wizard-body">
        <!-- Step 1：欢迎 -->
        <section v-if="step === 1" class="step">
          <h2>你好 👋</h2>
          <p>Jarvis 是你的个人任务助手，会自动同步禅道任务、追踪本地代码提交，并在下班前提醒你写日报。</p>
          <p>开始之前，需要配置几项基本信息：</p>
          <ul class="prep-list">
            <li>· 禅道地址、账号和密码</li>
            <li>· 本地代码所在的文件夹</li>
          </ul>
          <p class="hint">大概一分钟。密码会加密存到系统密钥链，绝不写入任何文件。</p>
        </section>

        <!-- Step 2：禅道地址 + 账号 -->
        <section v-if="step === 2" class="step">
          <h2>禅道在哪？</h2>
          <label class="form-field">
            <span class="form-label">禅道地址</span>
            <input class="form-input" type="url" placeholder="例如 http://zentao.example.com/zentao"
              v-model="baseUrl" autofocus />
          </label>
          <label class="form-field">
            <span class="form-label">你的账号</span>
            <input class="form-input" type="text" placeholder="禅道用户名"
              v-model="account" />
          </label>
          <p class="hint">这个账号同时也是 Jarvis 过滤"我的任务"的标识。</p>
        </section>

        <!-- Step 3：密码 + 测试 -->
        <section v-if="step === 3" class="step">
          <h2>密码 + 测试连接</h2>
          <label class="form-field">
            <span class="form-label">密码</span>
            <input class="form-input" type="password" placeholder="禅道登录密码"
              v-model="password" @keydown.enter="testConnection" />
          </label>
          <button class="test-btn" :disabled="!password || testState === 'testing'" @click="testConnection">
            {{ testState === 'testing' ? '测试中…' : '测试连接' }}
          </button>
          <p v-if="testMessage" class="test-msg" :class="`msg-${testState}`">{{ testMessage }}</p>
          <p class="hint">密码不会写入任何文件，只会存到 Windows / macOS 系统密钥链。</p>
        </section>

        <!-- Step 4：代码文件夹 -->
        <section v-if="step === 4" class="step">
          <h2>代码在哪？</h2>
          <p>选择一个或多个本地代码根目录，Jarvis 会扫描里面的 git 仓库，把 commit 关联到禅道任务。</p>
          <ul class="path-list">
            <li v-for="(p, i) in repoRoots" :key="i" class="path-item">
              <span class="path-text">{{ p }}</span>
              <button class="path-remove" @click="removeRoot(i)" title="移除">×</button>
            </li>
            <li v-if="repoRoots.length === 0" class="path-empty">还没有添加文件夹</li>
          </ul>
          <button class="test-btn" @click="pickFolder">+ 添加文件夹</button>
          <p class="hint">目录的第一层子文件夹会被识别为"业务线"。常见结构如 D:/coding/&lt;业务&gt;/&lt;仓库&gt;。</p>
        </section>

        <!-- Step 5：完成 -->
        <section v-if="step === 5" class="step">
          <h2>准备就绪</h2>
          <p>即将完成以下操作：</p>
          <ul class="confirm-list">
            <li>· 禅道地址：<b>{{ baseUrl }}</b></li>
            <li>· 禅道账号：<b>{{ account }}</b></li>
            <li>· 密码：<b>保存到系统密钥链</b></li>
            <li>· 代码文件夹（{{ repoRoots.length }} 个）：<b>{{ repoRoots.join('  ·  ') }}</b></li>
          </ul>
          <p class="hint">这些都可以以后在"设置"里随时修改。</p>
        </section>
      </div>

      <footer class="wizard-footer">
        <button class="step-btn" :disabled="step === 1" @click="prev">上一步</button>
        <button v-if="step < totalSteps" class="step-btn primary" :disabled="!canNext" @click="next">下一步</button>
        <button v-else class="step-btn primary" :disabled="finishing" @click="finish">
          {{ finishing ? '保存中…' : '开始使用' }}
        </button>
      </footer>
    </div>
  </div>
</template>

<style scoped>
.wizard-overlay {
  position: fixed;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  background: rgba(8, 14, 28, 0.85);
  backdrop-filter: blur(6px);
  z-index: 200;
  padding: 20px;
}
.wizard {
  width: 100%;
  max-width: 480px;
  display: flex;
  flex-direction: column;
  background: linear-gradient(135deg, rgba(20, 30, 56, 0.99), rgba(15, 23, 42, 0.99));
  border: 1px solid rgba(0, 212, 255, 0.25);
  border-radius: 16px;
  box-shadow: 0 18px 48px rgba(0, 0, 0, 0.6);
  color: rgba(255, 255, 255, 0.92);
  overflow: hidden;
  max-height: 88vh;
}

.wizard-header {
  padding: 14px 18px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  background: rgba(0, 0, 0, 0.2);
  border-bottom: 1px solid rgba(255, 255, 255, 0.06);
}
.wizard-title {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 14px;
  font-weight: 600;
}
.wizard-icon { font-size: 18px; }
.wizard-progress {
  display: flex;
  align-items: center;
  gap: 4px;
}
.dot {
  width: 7px; height: 7px;
  border-radius: 50%;
  background: rgba(255, 255, 255, 0.15);
}
.dot.active { background: rgba(0, 212, 255, 0.9); }
.step-text {
  margin-left: 6px;
  font-size: 10.5px;
  color: rgba(255, 255, 255, 0.45);
}

.wizard-body {
  flex: 1;
  overflow-y: auto;
  padding: 18px;
}
.step h2 {
  margin: 0 0 10px;
  font-size: 17px;
  font-weight: 600;
  color: rgba(255, 255, 255, 0.98);
}
.step p {
  margin: 6px 0;
  font-size: 12.5px;
  line-height: 1.6;
  color: rgba(255, 255, 255, 0.78);
}
.hint {
  margin-top: 10px !important;
  font-size: 11px !important;
  color: rgba(255, 255, 255, 0.45) !important;
}
.prep-list, .confirm-list {
  margin: 8px 0;
  padding: 8px 12px;
  list-style: none;
  background: rgba(0, 212, 255, 0.04);
  border-left: 2px solid rgba(0, 212, 255, 0.35);
  border-radius: 4px;
  font-size: 12px;
  line-height: 1.8;
  color: rgba(255, 255, 255, 0.85);
}
.confirm-list b { color: rgba(0, 212, 255, 0.92); font-weight: 600; }

/* form */
.form-field {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-top: 10px;
}
.form-label {
  width: 70px;
  font-size: 12px;
  color: rgba(255, 255, 255, 0.6);
  flex-shrink: 0;
}
.form-input {
  flex: 1;
  padding: 7px 10px;
  font-size: 12.5px;
  font-family: inherit;
  color: rgba(255, 255, 255, 0.95);
  background: rgba(255, 255, 255, 0.06);
  border: 1px solid rgba(255, 255, 255, 0.1);
  border-radius: 5px;
}
.form-input:focus {
  outline: none;
  border-color: rgba(0, 212, 255, 0.5);
  background: rgba(0, 212, 255, 0.05);
}

.test-btn {
  margin-top: 12px;
  padding: 7px 14px;
  font-size: 12px;
  color: rgba(255, 255, 255, 0.9);
  background: rgba(255, 255, 255, 0.06);
  border: 1px solid rgba(255, 255, 255, 0.12);
  border-radius: 5px;
  cursor: pointer;
}
.test-btn:hover:not(:disabled) {
  background: rgba(255, 255, 255, 0.1);
}
.test-btn:disabled { opacity: 0.5; cursor: not-allowed; }
.test-msg {
  margin-top: 8px !important;
  padding: 6px 10px;
  font-size: 11.5px !important;
  border-radius: 4px;
  white-space: pre-line;       /* 后端消息里的 \n 直接换行 */
  word-break: break-all;       /* 长 URL 强制换行，避免横向溢出 */
  line-height: 1.5;
}
.msg-ok { color: rgba(134, 239, 172, 0.95) !important; background: rgba(34, 197, 94, 0.12); }
.msg-fail { color: rgba(252, 165, 165, 0.95) !important; background: rgba(239, 68, 68, 0.12); }
.msg-testing { color: rgba(147, 197, 253, 0.95) !important; background: rgba(59, 130, 246, 0.12); }

.path-list {
  list-style: none;
  margin: 10px 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.path-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 10px;
  background: rgba(255, 255, 255, 0.03);
  border: 1px solid rgba(255, 255, 255, 0.06);
  border-radius: 5px;
  font-size: 12px;
}
.path-text {
  flex: 1;
  font-family: ui-monospace, monospace;
  word-break: break-all;
  color: rgba(255, 255, 255, 0.88);
}
.path-remove {
  width: 22px; height: 22px;
  display: inline-flex; align-items: center; justify-content: center;
  font-size: 15px;
  color: rgba(255, 255, 255, 0.5);
  background: transparent;
  border: none;
  border-radius: 4px;
  cursor: pointer;
}
.path-remove:hover { color: rgba(239, 68, 68, 0.95); background: rgba(239, 68, 68, 0.12); }
.path-empty {
  padding: 8px;
  font-size: 11.5px;
  color: rgba(255, 255, 255, 0.35);
  text-align: center;
}

.wizard-footer {
  padding: 12px 18px;
  display: flex;
  justify-content: space-between;
  gap: 8px;
  background: rgba(0, 0, 0, 0.2);
  border-top: 1px solid rgba(255, 255, 255, 0.06);
}
.step-btn {
  padding: 7px 18px;
  font-size: 12.5px;
  color: rgba(255, 255, 255, 0.8);
  background: rgba(255, 255, 255, 0.05);
  border: 1px solid rgba(255, 255, 255, 0.1);
  border-radius: 6px;
  cursor: pointer;
}
.step-btn:hover:not(:disabled) {
  background: rgba(255, 255, 255, 0.1);
}
.step-btn:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}
.step-btn.primary {
  color: rgba(0, 212, 255, 0.98);
  background: rgba(0, 212, 255, 0.16);
  border-color: rgba(0, 212, 255, 0.45);
}
.step-btn.primary:hover:not(:disabled) {
  background: rgba(0, 212, 255, 0.24);
}
</style>
