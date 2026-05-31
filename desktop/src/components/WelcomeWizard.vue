<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { getCurrentWindow, LogicalSize, LogicalPosition, currentMonitor } from '@tauri-apps/api/window'
import { useConfigStore, type WorkStyle } from '../stores/config'
import { normalizeZentaoBaseUrl } from '../composables/zentaoUrl'

const emit = defineEmits<{
  (e: 'done'): void
}>()

const store = useConfigStore()

const WIZARD_W = 500
const WIZARD_H = 620
let savedSize: { width: number; height: number } | null = null
let savedPos: { x: number; y: number } | null = null

async function enterWizardMode() {
  const win = getCurrentWindow()
  try {
    const scale = await win.scaleFactor()
    const outerSize = await win.outerSize()
    const outerPos = await win.outerPosition()
    savedSize = {
      width: Math.round(outerSize.width / scale),
      height: Math.round(outerSize.height / scale),
    }
    savedPos = {
      x: Math.round(outerPos.x / scale),
      y: Math.round(outerPos.y / scale),
    }
    await win.setSize(new LogicalSize(WIZARD_W, WIZARD_H))
    const monitor = await currentMonitor()
    if (monitor) {
      const monW = monitor.size.width / scale
      const monH = monitor.size.height / scale
      const cx = Math.round((monW - WIZARD_W) / 2)
      const cy = Math.round((monH - WIZARD_H) / 2)
      await win.setPosition(new LogicalPosition(cx, cy))
    }
  } catch (error) {
    console.error('[wizard] 放大窗口失败:', error)
  }
}

async function exitWizardMode() {
  if (!savedSize) return
  const win = getCurrentWindow()
  try {
    await win.setSize(new LogicalSize(savedSize.width, savedSize.height))
    if (savedPos) await win.setPosition(new LogicalPosition(savedPos.x, savedPos.y))
  } catch (error) {
    console.error('[wizard] 恢复窗口失败:', error)
  } finally {
    savedSize = null
    savedPos = null
  }
}

onMounted(enterWizardMode)
onUnmounted(exitWizardMode)

const step = ref(1)
const totalSteps = 6

const baseUrl = ref(store.config.zentao.baseUrl || '')
const account = ref(store.config.zentao.account || '')
const password = ref('')
const repoRoots = ref<string[]>([...store.config.repoRoots])

const workStyle = ref<WorkStyle>(store.config.workStyle ?? 'balanced')
const WORK_STYLE_OPTIONS: Array<{ value: WorkStyle; title: string; desc: string }> = [
  { value: 'focused', title: '专注模式', desc: '盯着少量固定项目，大部分时间都在持续推进主线任务。' },
  { value: 'multi', title: '并行模式', desc: '手里的项目和任务比较多，经常要在不同上下文之间切换。' },
  { value: 'transactional', title: '事务模式', desc: '部署、值班、排障、开会、沟通这类事情占比更高。' },
  { value: 'balanced', title: '平衡模式', desc: '代码推进和事务处理都会有，整体比较均衡。' },
]
const workStyleTitle = computed(
  () => WORK_STYLE_OPTIONS.find(option => option.value === workStyle.value)?.title ?? '平衡模式',
)

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
  const cleaned = normalizeZentaoBaseUrl(baseUrl.value)
  if (cleaned !== baseUrl.value) baseUrl.value = cleaned

  testState.value = 'testing'
  testMessage.value = ''
  try {
    const result = await invoke<{ ok: boolean; message: string }>('zentao_test_connection', {
      req: { baseUrl: baseUrl.value, account: account.value, password: password.value },
    })
    testState.value = result.ok ? 'ok' : 'fail'
    testMessage.value = result.message
  } catch (error: any) {
    testState.value = 'fail'
    testMessage.value = String(error?.message ?? error)
  }
}

async function pickFolder() {
  const picked = await invoke<string | null>('pick_directory', {
    title: '选择本地代码根目录',
  })
  if (!picked) return
  if (!repoRoots.value.includes(picked)) repoRoots.value.push(picked)
}

function removeRoot(index: number) {
  repoRoots.value.splice(index, 1)
}

async function finish() {
  finishing.value = true
  try {
    await invoke('credentials_set', { account: account.value, password: password.value })
    store.config.zentao.baseUrl = normalizeZentaoBaseUrl(baseUrl.value)
    store.config.zentao.account = account.value.trim()
    store.config.repoRoots = [...repoRoots.value]
    store.config.workStyle = workStyle.value
    await store.save()
    emit('done')
  } catch (error: any) {
    testState.value = 'fail'
    testMessage.value = `完成失败：${String(error?.message ?? error)}`
  } finally {
    finishing.value = false
  }
}

function next() {
  if (canNext.value && step.value < totalSteps) step.value++
}

function prev() {
  if (step.value > 1) step.value--
}
</script>

<template>
  <div class="wizard-overlay pointer-target">
    <div class="wizard">
      <header class="wizard-header">
        <div class="wizard-title">
          <span class="wizard-icon">✨</span>
          <span>欢迎使用 {{ store.config.assistantName }}</span>
        </div>
        <div class="wizard-progress">
          <span v-for="i in totalSteps" :key="i" class="dot" :class="{ active: i <= step }" />
          <span class="step-text">{{ step }} / {{ totalSteps }}</span>
        </div>
      </header>

      <div class="wizard-body">
        <section v-if="step === 1" class="step">
          <h2>你好，先把基础信息接好</h2>
          <p>{{ store.config.assistantName }} 是你的个人工作助手，会同步禅道任务、扫描本地提交，并在下班前提醒你收尾和补工时。</p>
          <p>开始前需要配置几项基本信息：</p>
          <ul class="prep-list">
            <li>路 禅道地址、账号和密码</li>
            <li>路 本地代码所在的文件夹</li>
          </ul>
          <p class="hint">大概一分钟。密码会加密保存在系统密钥链里，不会写入任何配置文件。</p>
        </section>

        <section v-if="step === 2" class="step">
          <h2>先告诉我禅道在哪</h2>
          <label class="form-field">
            <span class="form-label">禅道地址</span>
            <input
              v-model="baseUrl"
              class="form-input"
              type="url"
              placeholder="例如 http://zentao.example.com/zentao"
              autofocus
            />
          </label>
          <label class="form-field">
            <span class="form-label">账号</span>
            <input
              v-model="account"
              class="form-input"
              type="text"
              placeholder="你的禅道用户名"
            />
          </label>
          <p class="hint">这个账号也会用来过滤“我的任务”，所以尽量填你平时登录禅道用的那个。</p>
        </section>

        <section v-if="step === 3" class="step">
          <h2>再测一下连接是否正常</h2>
          <label class="form-field">
            <span class="form-label">密码</span>
            <input
              v-model="password"
              class="form-input"
              type="password"
              placeholder="禅道登录密码"
              @keydown.enter="testConnection"
            />
          </label>
          <button class="test-btn" :disabled="!password || testState === 'testing'" @click="testConnection">
            {{ testState === 'testing' ? '测试中...' : '测试连接' }}
          </button>
          <p v-if="testMessage" class="test-msg" :class="`msg-${testState}`">{{ testMessage }}</p>
          <p class="hint">密码不会进任何本地文件，只会保存在 Windows / macOS 的系统密钥链里。</p>
        </section>

        <section v-if="step === 4" class="step">
          <h2>你的代码放在哪些目录</h2>
          <p>选一个或多个本地代码根目录，{{ store.config.assistantName }} 会扫描里面的 git 仓库，把提交和禅道任务关联起来。</p>
          <ul class="path-list">
            <li v-for="(path, index) in repoRoots" :key="index" class="path-item">
              <span class="path-text">{{ path }}</span>
              <button class="path-remove" title="移除" @click="removeRoot(index)">×</button>
            </li>
            <li v-if="repoRoots.length === 0" class="path-empty">还没有添加文件夹</li>
          </ul>
          <button class="test-btn" @click="pickFolder">+ 添加文件夹</button>
          <p class="hint">目录的第一层子文件夹会被当作业务线，常见结构例如 `D:/coding/采购系统/web-app`。</p>
        </section>

        <section v-if="step === 5" class="step">
          <h2>你平时更像哪种工作状态</h2>
          <p>选一个最接近你的模式，后面今日计划、复盘和工时推荐都会按这个方向帮你收敛候选。</p>
          <div class="style-list">
            <button
              v-for="option in WORK_STYLE_OPTIONS"
              :key="option.value"
              type="button"
              class="style-card"
              :class="{ active: workStyle === option.value }"
              @click="workStyle = option.value"
            >
              <span class="style-radio" />
              <span class="style-main">
                <strong>{{ option.title }}</strong>
                <small>{{ option.desc }}</small>
              </span>
            </button>
          </div>
        </section>

        <section v-if="step === 6" class="step">
          <h2>准备就绪</h2>
          <p>确认一下，接下来会保存这些配置：</p>
          <ul class="confirm-list">
            <li>路 禅道地址：<b>{{ baseUrl }}</b></li>
            <li>路 禅道账号：<b>{{ account }}</b></li>
            <li>路 密码：<b>保存到系统密钥链</b></li>
            <li>路 代码目录（{{ repoRoots.length }} 个）：<b>{{ repoRoots.join('  路  ') }}</b></li>
            <li>路 工作模式：<b>{{ workStyleTitle }}</b></li>
          </ul>
          <p class="hint">这些后面都可以在设置里继续改，不会锁死。</p>
        </section>
      </div>

      <footer class="wizard-footer">
        <button class="step-btn" :disabled="step === 1" @click="prev">上一步</button>
        <button v-if="step < totalSteps" class="step-btn primary" :disabled="!canNext" @click="next">下一步</button>
        <button v-else class="step-btn primary" :disabled="finishing" @click="finish">
          {{ finishing ? '保存中...' : '开始使用' }}
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
  width: 7px;
  height: 7px;
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

.prep-list,
.confirm-list {
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

.test-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.test-msg {
  margin-top: 8px !important;
  padding: 6px 10px;
  font-size: 11.5px !important;
  border-radius: 4px;
  white-space: pre-line;
  word-break: break-all;
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
  width: 22px;
  height: 22px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  font-size: 15px;
  color: rgba(255, 255, 255, 0.5);
  background: transparent;
  border: none;
  border-radius: 4px;
  cursor: pointer;
}

.path-remove:hover {
  color: rgba(239, 68, 68, 0.95);
  background: rgba(239, 68, 68, 0.12);
}

.path-empty {
  padding: 8px;
  font-size: 11.5px;
  color: rgba(255, 255, 255, 0.35);
  text-align: center;
}

.style-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
  margin-top: 12px;
}

.style-card {
  display: flex;
  align-items: flex-start;
  gap: 10px;
  width: 100%;
  padding: 10px 12px;
  text-align: left;
  color: rgba(255, 255, 255, 0.9);
  background: rgba(255, 255, 255, 0.04);
  border: 1px solid rgba(255, 255, 255, 0.1);
  border-radius: 8px;
  cursor: pointer;
}

.style-card:hover { background: rgba(0, 212, 255, 0.06); }

.style-card.active {
  border-color: rgba(0, 212, 255, 0.55);
  background: rgba(0, 212, 255, 0.1);
}

.style-radio {
  flex-shrink: 0;
  width: 14px;
  height: 14px;
  margin-top: 2px;
  border-radius: 50%;
  border: 2px solid rgba(255, 255, 255, 0.3);
}

.style-card.active .style-radio {
  border-color: rgba(0, 212, 255, 0.9);
  background: radial-gradient(circle, rgba(0, 212, 255, 0.95) 0 4px, transparent 5px);
}

.style-main {
  display: flex;
  flex-direction: column;
  gap: 3px;
  min-width: 0;
}

.style-main strong { font-size: 13px; color: rgba(255, 255, 255, 0.96); }
.style-main small { font-size: 11.5px; line-height: 1.4; color: rgba(255, 255, 255, 0.6); }

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
