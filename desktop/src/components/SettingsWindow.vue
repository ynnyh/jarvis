<script setup lang="ts">
import { computed, ref, onMounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useConfigStore } from '../stores/config'
import { normalizeZentaoBaseUrl } from '../composables/zentaoUrl'

const store = useConfigStore()

const DAYS = [
  { value: 1, label: '一' },
  { value: 2, label: '二' },
  { value: 3, label: '三' },
  { value: 4, label: '四' },
  { value: 5, label: '五' },
  { value: 6, label: '六' },
  { value: 0, label: '日' },
]

const phaseLabel = computed(() => {
  switch (store.phase) {
    case 'working': return '工作中'
    case 'lunch': return '午休'
    case 'before-work': return '尚未上班'
    case 'after-work': return '已下班'
    case 'weekend': return '周末'
    case 'dayoff': return '今天休假'
    case 'overtime': return '加班模式'
    default: return ''
  }
})

function toggleWorkDay(day: number) {
  const days = store.config.workSchedule.workDays
  const i = days.indexOf(day)
  if (i >= 0) days.splice(i, 1)
  else { days.push(day); days.sort() }
}

// ===== 禅道连接 =====
const zentaoPassword = ref('')          // 用户输入；只在按"保存"时写到 keychain
const zentaoTestState = ref<'idle' | 'testing' | 'ok' | 'fail'>('idle')
const zentaoTestMessage = ref('')

async function testZentao() {
  // 同 WelcomeWizard：测试前规范化，并把清洗后的值写回 store（store.watch 会持久化）
  const cleaned = normalizeZentaoBaseUrl(store.config.zentao.baseUrl)
  if (cleaned !== store.config.zentao.baseUrl) store.config.zentao.baseUrl = cleaned

  zentaoTestState.value = 'testing'
  zentaoTestMessage.value = ''
  try {
    const r = await invoke<{ ok: boolean; message: string }>('zentao_test_connection', {
      req: {
        baseUrl: store.config.zentao.baseUrl,
        account: store.config.zentao.account,
        password: zentaoPassword.value,
      },
    })
    zentaoTestState.value = r.ok ? 'ok' : 'fail'
    zentaoTestMessage.value = r.message
  } catch (e: any) {
    zentaoTestState.value = 'fail'
    zentaoTestMessage.value = String(e?.message ?? e)
  }
}

async function saveZentaoPassword() {
  if (!store.config.zentao.account.trim()) {
    zentaoTestMessage.value = '请先填写禅道账号'
    zentaoTestState.value = 'fail'
    return
  }
  if (!zentaoPassword.value) {
    zentaoTestMessage.value = '请输入密码'
    zentaoTestState.value = 'fail'
    return
  }
  try {
    await invoke('credentials_set', {
      account: store.config.zentao.account,
      password: zentaoPassword.value,
    })
    // 重启 daemon 拿新密码 —— 不重启的话它仍用旧 env 里的密码调禅道认证。
    try { await invoke('daemon_restart') } catch (e) {
      console.warn('daemon 重启失败（密码已保存）:', e)
    }
    zentaoTestState.value = 'ok'
    zentaoTestMessage.value = '密码已加密保存到系统密钥链，daemon 已重启使用新凭证'
    zentaoPassword.value = ''
  } catch (e: any) {
    zentaoTestState.value = 'fail'
    zentaoTestMessage.value = '保存密码失败：' + String(e?.message ?? e)
  }
}

// ===== LLM =====
const llmShowKey = ref(false)
const llmTestState = ref<'idle' | 'testing' | 'ok' | 'fail'>('idle')
const llmTestMessage = ref('')
const ccImportState = ref<'idle' | 'importing' | 'ok' | 'fail'>('idle')
const ccImportMessage = ref('')

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
    const data = r.data as { found: boolean; reason?: string; provider?: { name: string; apiKey: string; baseUrl: string; model: string; wireApi?: string } }
    if (!data.found || !data.provider) {
      ccImportState.value = 'fail'
      ccImportMessage.value = data.reason || '未找到 CC Switch 配置'
      return
    }
    // 覆盖 LLM 字段。provider 切到 custom（CC Switch 的 baseUrl 一般不是 OpenAI/DeepSeek）
    store.config.llm.provider = 'custom'
    store.config.llm.apiKey = data.provider.apiKey
    store.config.llm.baseUrl = data.provider.baseUrl
    store.config.llm.model = data.provider.model
    ccImportState.value = 'ok'
    let msg = `已导入「${data.provider.name}」：${data.provider.model}`
    if (data.provider.wireApi === 'responses') {
      msg += '\n⚠ 该 provider 用的是 Codex responses API（/v1/responses），咱们的客户端走 /v1/chat/completions。如果连通失败，需要换条 baseUrl 或上游开 chat completions 端点。'
    }
    ccImportMessage.value = msg
  } catch (e: any) {
    ccImportState.value = 'fail'
    ccImportMessage.value = String(e?.message ?? e)
  }
}

// 切换 provider 时把 baseUrl/model 顺手填成厂商默认值，避免用户手动配
const LLM_PRESETS: Record<string, { baseUrl: string; model: string }> = {
  deepseek: { baseUrl: 'https://api.deepseek.com', model: 'deepseek-chat' },
  openai: { baseUrl: 'https://api.openai.com', model: 'gpt-4o-mini' },
  custom: { baseUrl: '', model: '' },
}
function onLlmProviderChange(next: string) {
  const preset = LLM_PRESETS[next]
  if (!preset) return
  // custom 不覆盖已有内容；其余情况只在用户没改过时才填默认（避免抹掉用户的自定义值）
  if (next === 'custom') return
  store.config.llm.baseUrl = preset.baseUrl
  store.config.llm.model = preset.model
}

async function testLlm() {
  llmTestState.value = 'testing'
  llmTestMessage.value = ''
  // 先等一次保存（store watcher 250ms 防抖），保证 daemon 拿到的是最新 apiKey
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
      llmTestState.value = 'ok'
      llmTestMessage.value = `连通：${r.data.model ?? store.config.llm.model} → “${String(r.data.text).slice(0, 40)}”`
    } else {
      llmTestState.value = 'fail'
      llmTestMessage.value = r.error || '调用失败：无文本返回'
    }
  } catch (e: any) {
    llmTestState.value = 'fail'
    llmTestMessage.value = String(e?.message ?? e)
  }
}

// ===== 代码文件夹（repoRoots） =====
async function addRepoRoot() {
  const picked = await invoke<string | null>('pick_directory', {
    title: '选择本地代码根目录（如 D:/coding）',
  })
  if (!picked) return
  if (store.config.repoRoots.includes(picked)) return
  store.config.repoRoots.push(picked)
}

function removeRepoRoot(i: number) {
  store.config.repoRoots.splice(i, 1)
}

// ===== 忽略的业务线 =====
const excludedLines = ref<string[]>([])
const newExcludedInput = ref('')

async function loadExcluded() {
  try {
    excludedLines.value = await invoke<string[]>('excluded_business_lines_load')
  } catch {
    excludedLines.value = []
  }
}

async function saveExcluded() {
  try {
    await invoke('excluded_business_lines_save', { lines: excludedLines.value })
  } catch (e) {
    console.error('保存排除业务线失败:', e)
  }
}

function addExcluded() {
  const v = newExcludedInput.value.trim()
  if (!v) return
  if (excludedLines.value.includes(v)) {
    newExcludedInput.value = ''
    return
  }
  excludedLines.value.push(v)
  newExcludedInput.value = ''
  saveExcluded()
}

function removeExcluded(i: number) {
  excludedLines.value.splice(i, 1)
  saveExcluded()
}

onMounted(loadExcluded)
</script>

<template>
  <Transition name="panel">
    <div v-if="store.showSettingsWindow" class="settings-panel pointer-target">
      <header class="panel-header">
        <div class="panel-title">
          <span class="title-icon">⚙️</span>
          <span class="title-text">设置</span>
        </div>
        <button class="icon-btn" title="关闭" @click="store.showSettingsWindow = false">×</button>
      </header>

      <!-- 当前状态条 -->
      <div class="phase-bar" :class="`phase-${store.phase}`">
        <span class="phase-dot" />
        <span>当前：{{ phaseLabel }}</span>
        <span v-if="store.isQuietHours" class="phase-meta">静默中</span>
      </div>

      <div class="panel-body">
        <!-- 助手名字 -->
        <section class="section">
          <h3 class="section-title">助手名字</h3>
          <label class="field">
            <span class="field-label">名字</span>
            <input class="text-input" type="text" maxlength="16" placeholder="Jarvis"
              v-model="store.config.assistantName" />
          </label>
          <p class="section-hint">影响菜单、问候、通知标题等所有显示文案，重启或切换面板即时生效</p>
        </section>

        <!-- 禅道连接 -->
        <section class="section">
          <h3 class="section-title">禅道连接</h3>
          <label class="field">
            <span class="field-label">地址</span>
            <input class="text-input" type="url" placeholder="http://zentao.example.com/zentao"
              v-model="store.config.zentao.baseUrl" />
          </label>
          <label class="field">
            <span class="field-label">账号</span>
            <input class="text-input" type="text" placeholder="你的禅道用户名"
              v-model="store.config.zentao.account" />
          </label>
          <label class="field">
            <span class="field-label">密码</span>
            <input class="text-input" type="password" placeholder="留空表示不修改密钥链中的密码"
              v-model="zentaoPassword" />
          </label>
          <div class="zentao-actions">
            <button class="action-btn" :disabled="zentaoTestState === 'testing'" @click="testZentao">
              {{ zentaoTestState === 'testing' ? '测试中…' : '测试连接' }}
            </button>
            <button class="action-btn primary" @click="saveZentaoPassword">
              保存密码到密钥链
            </button>
          </div>
          <p v-if="zentaoTestMessage" class="zentao-msg" :class="`msg-${zentaoTestState}`">
            {{ zentaoTestMessage }}
          </p>
          <p class="section-hint">密码不会写入任何文件，仅保存在系统密钥链中</p>
        </section>

        <!-- LLM 接入 -->
        <section class="section">
          <h3 class="section-title">LLM 接入</h3>
          <p class="section-hint">日报、风险摘要、commit↔任务评分可选调用。apiKey 明文存 config.json，不写密钥链</p>
          <label class="field">
            <span class="field-label">服务商</span>
            <select class="text-input"
              :value="store.config.llm.provider"
              @change="(e) => { const v = (e.target as HTMLSelectElement).value as any; store.config.llm.provider = v; onLlmProviderChange(v) }">
              <option value="deepseek">DeepSeek</option>
              <option value="openai">OpenAI</option>
              <option value="custom">自定义（OpenAI 兼容）</option>
            </select>
          </label>
          <label class="field">
            <span class="field-label">地址</span>
            <input class="text-input" type="url" placeholder="https://api.deepseek.com"
              v-model="store.config.llm.baseUrl" />
          </label>
          <label class="field">
            <span class="field-label">模型</span>
            <input class="text-input" type="text" placeholder="deepseek-chat"
              v-model="store.config.llm.model" />
          </label>
          <label class="field">
            <span class="field-label">apiKey</span>
            <input class="text-input" :type="llmShowKey ? 'text' : 'password'"
              placeholder="sk-..." v-model="store.config.llm.apiKey" />
            <button class="action-btn" style="margin-left:6px;padding:4px 8px;"
              @click="llmShowKey = !llmShowKey">
              {{ llmShowKey ? '隐藏' : '显示' }}
            </button>
          </label>
          <div class="zentao-actions">
            <button class="action-btn primary"
              :disabled="llmTestState === 'testing' || !store.config.llm.apiKey"
              @click="testLlm">
              {{ llmTestState === 'testing' ? '测试中…' : '测试连接' }}
            </button>
            <button class="action-btn"
              :disabled="ccImportState === 'importing'"
              @click="importFromCcSwitch"
              title="从 ~/.cc-switch/ 读取当前激活的 Codex（OpenAI）provider 一键填入">
              {{ ccImportState === 'importing' ? '导入中…' : '🔄 从 CC Switch 导入' }}
            </button>
          </div>
          <p v-if="llmTestMessage" class="zentao-msg" :class="`msg-${llmTestState}`">
            {{ llmTestMessage }}
          </p>
          <p v-if="ccImportMessage" class="zentao-msg" :class="`msg-${ccImportState === 'importing' ? 'testing' : ccImportState}`">
            {{ ccImportMessage }}
          </p>
        </section>

        <!-- 代码文件夹（repoRoots） -->
        <section class="section">
          <h3 class="section-title">本地代码文件夹</h3>
          <p class="section-hint">{{ store.config.assistantName }} 会扫描这些目录下的 git 仓库，关联到禅道任务以生成日报。每个目录第一层子文件夹的名字会被当作"业务线"</p>
          <ul class="path-list">
            <li v-for="(p, i) in store.config.repoRoots" :key="i" class="path-item">
              <span class="path-text">{{ p }}</span>
              <button class="path-remove" @click="removeRepoRoot(i)" title="移除">×</button>
            </li>
            <li v-if="store.config.repoRoots.length === 0" class="path-empty">还没有添加</li>
          </ul>
          <button class="action-btn" @click="addRepoRoot">+ 添加文件夹</button>
        </section>

        <!-- 忽略的业务线 -->
        <section class="section">
          <h3 class="section-title">忽略的文件夹（业务线）</h3>
          <p class="section-hint">这些业务线下的 commit 不会进入工时统计和日报。常用于个人项目、试验仓库等</p>
          <ul class="path-list">
            <li v-for="(name, i) in excludedLines" :key="name" class="path-item">
              <span class="path-text">{{ name }}</span>
              <button class="path-remove" @click="removeExcluded(i)" title="移除">×</button>
            </li>
            <li v-if="excludedLines.length === 0" class="path-empty">没有忽略项</li>
          </ul>
          <div class="excl-add-row">
            <input class="text-input excl-input" type="text"
              placeholder="业务线名（如 my-mcp-servers）"
              v-model="newExcludedInput"
              @keydown.enter="addExcluded" />
            <button class="action-btn" @click="addExcluded">添加</button>
          </div>
        </section>

        <!-- 工作日 -->
        <section class="section">
          <h3 class="section-title">工作日</h3>
          <div class="weekday-row">
            <button
              v-for="d in DAYS"
              :key="d.value"
              class="weekday-btn"
              :class="{ active: store.config.workSchedule.workDays.includes(d.value) }"
              @click="toggleWorkDay(d.value)"
            >{{ d.label }}</button>
          </div>
        </section>

        <!-- 工作时段 -->
        <section class="section">
          <h3 class="section-title">工作时段</h3>
          <div class="periods">
            <div
              v-for="(p, i) in store.config.workSchedule.periods"
              :key="i"
              class="period-row"
            >
              <input class="period-label" v-model="p.label" placeholder="名称" />
              <input class="time-input" type="time" v-model="p.start" />
              <span class="dash">—</span>
              <input class="time-input" type="time" v-model="p.end" />
            </div>
          </div>
          <p class="section-hint">改完即时生效，自动保存</p>
        </section>

        <!-- 静默规则 -->
        <section class="section">
          <h3 class="section-title">静默规则</h3>
          <label class="toggle-row">
            <input type="checkbox" v-model="store.config.notifications.quietDuringLunch" />
            <span>午休时段静默（不弹通知）</span>
          </label>
          <label class="toggle-row">
            <input type="checkbox" v-model="store.config.notifications.quietAfterWork" />
            <span>下班后 / 上班前静默</span>
          </label>
          <label class="toggle-row">
            <input type="checkbox" v-model="store.config.notifications.quietOnWeekends" />
            <span>周末整天静默</span>
          </label>
        </section>

        <!-- 仪式感 -->
        <section class="section">
          <h3 class="section-title">早晚仪式</h3>
          <label class="toggle-row">
            <input type="checkbox" v-model="store.config.notifications.morningGreeting" />
            <span>上班时弹早安卡片 + 今日速览</span>
          </label>
          <label class="toggle-row">
            <input type="checkbox" v-model="store.config.notifications.eveningSummary" />
            <span>下班前
              <input
                class="inline-num"
                type="number"
                min="5" max="120" step="5"
                v-model.number="store.config.notifications.eveningSummaryMinutesBefore"
              />
              分钟弹日终总结
            </span>
          </label>
        </section>

        <!-- 工作时段小提示 -->
        <section class="section">
          <h3 class="section-title">工作时段小提示</h3>
          <label class="toggle-row">
            <input type="checkbox" v-model="store.config.notifications.workdayNudges" />
            <span>上班时段定时弹提示（喝水 / 起身 / 午饭 / 下班）</span>
          </label>
          <label class="toggle-row">
            <span>喝水 / 起身 每
              <input
                class="inline-num"
                type="number"
                min="30" max="240" step="15"
                v-model.number="store.config.notifications.nudgeIntervalMinutes"
                :disabled="!store.config.notifications.workdayNudges"
              />
              分钟轮一次
            </span>
          </label>
          <p class="section-hint">午饭前 10 分钟、下班前 10 分钟为时间锚点，自动一次。所有提示静默时段不弹。</p>
        </section>

        <!-- 今日临时覆盖 -->
        <section class="section">
          <h3 class="section-title">今日临时覆盖</h3>
          <div class="mode-row">
            <button
              class="mode-btn"
              :class="{ active: store.config.override.todayMode === 'normal' }"
              @click="store.setTodayMode('normal')"
            >正常</button>
            <button
              class="mode-btn"
              :class="{ active: store.config.override.todayMode === 'overtime' }"
              @click="store.setTodayMode('overtime')"
            >今晚加班</button>
            <button
              class="mode-btn"
              :class="{ active: store.config.override.todayMode === 'dayoff' }"
              @click="store.setTodayMode('dayoff')"
            >今天休假</button>
          </div>
          <p class="section-hint">仅当天有效，次日自动恢复正常</p>
        </section>
      </div>
    </div>
  </Transition>
</template>

<style scoped>
.settings-panel {
  position: fixed;
  inset: 8px 8px 90px 8px;
  display: flex;
  flex-direction: column;
  background: linear-gradient(135deg, rgba(20, 30, 56, 0.97), rgba(15, 23, 42, 0.97));
  border: 1px solid rgba(100, 200, 255, 0.16);
  border-radius: 14px;
  box-shadow: 0 12px 32px rgba(0, 0, 0, 0.5);
  overflow: hidden;
  z-index: 60;
  color: rgba(255, 255, 255, 0.92);
}

.panel-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 8px 10px;
  background: rgba(0, 0, 0, 0.2);
  border-bottom: 1px solid rgba(255, 255, 255, 0.06);
}
.panel-title {
  display: flex; align-items: center; gap: 6px;
  font-size: 13px; font-weight: 600;
}
.title-icon { font-size: 14px; }
.icon-btn {
  width: 22px; height: 22px;
  display: inline-flex; align-items: center; justify-content: center;
  font-size: 16px; line-height: 1;
  color: rgba(255, 255, 255, 0.55);
  background: transparent; border: none; border-radius: 6px;
  cursor: pointer;
}
.icon-btn:hover { color: rgba(255, 255, 255, 0.95); background: rgba(255, 255, 255, 0.08); }

.phase-bar {
  display: flex; align-items: center; gap: 6px;
  padding: 4px 10px;
  font-size: 10px;
  background: rgba(0, 0, 0, 0.15);
  border-bottom: 1px solid rgba(255, 255, 255, 0.04);
  color: rgba(255, 255, 255, 0.65);
}
.phase-dot { width: 6px; height: 6px; border-radius: 50%; background: rgba(16, 185, 129, 0.9); }
.phase-working .phase-dot { background: rgba(16, 185, 129, 0.95); }
.phase-lunch .phase-dot { background: rgba(167, 139, 250, 0.95); }
.phase-after-work .phase-dot,
.phase-before-work .phase-dot { background: rgba(148, 163, 184, 0.7); }
.phase-weekend .phase-dot,
.phase-dayoff .phase-dot { background: rgba(245, 158, 11, 0.9); }
.phase-meta { margin-left: auto; color: rgba(245, 158, 11, 0.85); }

.panel-body {
  flex: 1;
  overflow-y: auto;
  padding: 10px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.section { display: flex; flex-direction: column; gap: 6px; }
.section-title {
  margin: 0;
  font-size: 11px;
  font-weight: 600;
  color: rgba(0, 212, 255, 0.85);
  letter-spacing: 0.5px;
}
.section-hint {
  margin: 2px 0 0;
  font-size: 9.5px;
  color: rgba(255, 255, 255, 0.35);
}

/* 工作日按钮 */
.weekday-row { display: flex; gap: 4px; }
.weekday-btn {
  flex: 1;
  height: 26px;
  font-size: 11px;
  color: rgba(255, 255, 255, 0.55);
  background: rgba(255, 255, 255, 0.04);
  border: 1px solid rgba(255, 255, 255, 0.06);
  border-radius: 6px;
  cursor: pointer;
  transition: all 0.15s;
}
.weekday-btn.active {
  color: rgba(0, 212, 255, 0.95);
  background: rgba(0, 212, 255, 0.12);
  border-color: rgba(0, 212, 255, 0.4);
}

/* 工作时段 */
.periods { display: flex; flex-direction: column; gap: 4px; }
.period-row {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 4px 0;
}
.period-label {
  width: 50px;
  padding: 3px 6px;
  font-size: 11px;
  color: rgba(255, 255, 255, 0.85);
  background: rgba(255, 255, 255, 0.06);
  border: 1px solid rgba(255, 255, 255, 0.08);
  border-radius: 4px;
}
.time-input {
  padding: 3px 6px;
  font-size: 11px;
  font-family: inherit;
  color: rgba(255, 255, 255, 0.92);
  background: rgba(255, 255, 255, 0.06);
  border: 1px solid rgba(255, 255, 255, 0.08);
  border-radius: 4px;
}
.dash { color: rgba(255, 255, 255, 0.4); font-size: 11px; }

/* 切换开关 */
.toggle-row {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 11.5px;
  color: rgba(255, 255, 255, 0.85);
  cursor: pointer;
  padding: 2px 0;
}

/* 字段 */
.field {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 3px 0;
}
.field-label {
  width: 48px;
  font-size: 11px;
  color: rgba(255, 255, 255, 0.55);
  flex-shrink: 0;
}
.text-input {
  flex: 1;
  padding: 4px 8px;
  font-size: 11.5px;
  font-family: inherit;
  color: rgba(255, 255, 255, 0.92);
  background: rgba(255, 255, 255, 0.06);
  border: 1px solid rgba(255, 255, 255, 0.08);
  border-radius: 4px;
}
.text-input:focus {
  outline: none;
  border-color: rgba(0, 212, 255, 0.5);
  background: rgba(0, 212, 255, 0.05);
}

/* 行动按钮 */
.action-btn {
  padding: 5px 12px;
  font-size: 11px;
  color: rgba(255, 255, 255, 0.85);
  background: rgba(255, 255, 255, 0.05);
  border: 1px solid rgba(255, 255, 255, 0.1);
  border-radius: 5px;
  cursor: pointer;
  transition: all 0.15s;
}
.action-btn:hover:not(:disabled) {
  background: rgba(255, 255, 255, 0.1);
  border-color: rgba(255, 255, 255, 0.18);
}
.action-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
.action-btn.primary {
  color: rgba(0, 212, 255, 0.95);
  background: rgba(0, 212, 255, 0.12);
  border-color: rgba(0, 212, 255, 0.35);
}
.action-btn.primary:hover {
  background: rgba(0, 212, 255, 0.18);
}

.zentao-actions {
  display: flex;
  gap: 6px;
  margin-top: 4px;
}
.zentao-msg {
  margin: 4px 0 0;
  padding: 4px 8px;
  font-size: 11px;
  border-radius: 4px;
  line-height: 1.5;
  white-space: pre-line;       /* 多行诊断信息按 \n 换行 */
  word-break: break-all;       /* 长 URL 强制换行 */
}
.msg-ok { color: rgba(134, 239, 172, 0.95); background: rgba(34, 197, 94, 0.12); }
.msg-fail { color: rgba(252, 165, 165, 0.95); background: rgba(239, 68, 68, 0.12); }
.msg-testing { color: rgba(147, 197, 253, 0.95); background: rgba(59, 130, 246, 0.12); }

/* 路径列表 */
.path-list {
  list-style: none;
  margin: 4px 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 3px;
}
.path-item {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 5px 8px;
  background: rgba(255, 255, 255, 0.03);
  border: 1px solid rgba(255, 255, 255, 0.05);
  border-radius: 4px;
  font-size: 11.5px;
  color: rgba(255, 255, 255, 0.85);
}
.path-text {
  flex: 1;
  font-family: ui-monospace, monospace;
  word-break: break-all;
}
.path-remove {
  width: 20px;
  height: 20px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  font-size: 14px;
  color: rgba(255, 255, 255, 0.5);
  background: transparent;
  border: none;
  border-radius: 4px;
  cursor: pointer;
  flex-shrink: 0;
}
.path-remove:hover {
  color: rgba(239, 68, 68, 0.9);
  background: rgba(239, 68, 68, 0.1);
}
.path-empty {
  padding: 6px;
  font-size: 11px;
  color: rgba(255, 255, 255, 0.35);
  text-align: center;
}
.excl-add-row {
  display: flex;
  gap: 6px;
  margin-top: 6px;
}
.excl-input { flex: 1; }
.toggle-row input[type=checkbox] {
  width: 14px;
  height: 14px;
  accent-color: rgba(0, 212, 255, 0.9);
}
.inline-num {
  width: 42px;
  margin: 0 2px;
  padding: 2px 4px;
  font-size: 11px;
  text-align: center;
  color: rgba(255, 255, 255, 0.92);
  background: rgba(255, 255, 255, 0.06);
  border: 1px solid rgba(255, 255, 255, 0.08);
  border-radius: 4px;
}

/* 模式按钮 */
.mode-row { display: flex; gap: 4px; }
.mode-btn {
  flex: 1;
  padding: 6px 4px;
  font-size: 11px;
  color: rgba(255, 255, 255, 0.65);
  background: rgba(255, 255, 255, 0.04);
  border: 1px solid rgba(255, 255, 255, 0.08);
  border-radius: 6px;
  cursor: pointer;
  transition: all 0.15s;
}
.mode-btn.active {
  color: rgba(245, 158, 11, 0.98);
  background: rgba(245, 158, 11, 0.15);
  border-color: rgba(245, 158, 11, 0.4);
}

.panel-enter-active,
.panel-leave-active {
  transition: opacity 0.2s ease, transform 0.2s ease;
}
.panel-enter-from,
.panel-leave-to {
  opacity: 0;
  transform: translateY(6px) scale(0.98);
}
</style>
