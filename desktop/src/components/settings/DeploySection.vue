<script setup lang="ts">
import { onMounted, reactive, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'

// ============================================================================
// 后端契约（src-tauri/src/commands/deploy_config.rs）
// ============================================================================
//   deploy_config_get -> { jenkinsMcpPath, jenkinsUrl, credentials[] }
//   deploy_config_save({ input: { jenkinsMcpPath?, jenkinsUrl, credentials[] } }) -> void
//   deploy_test_connection({ name, url, username, token? }) -> { ok, detail, jenkinsUrl }
//     用**当前表单值**直接打 Jenkins /api/json 验证凭据，不必先保存；token 留空则后端
//     回退取 keychain 里该账号(name)已存的 token。
//
// credentials[].name 是「账号」的**内部 id**：自动生成、对用户隐藏，仅用于派生 keychain
// account（jenkins-<id>-token）与 jenkins-mcp 的环境名。用户只填 用户名 / token / 项目，
// 不感知也不编辑 id。token 进来只有 hasToken 布尔（后端绝不回明文）；保存时仅当用户输入了
// 新值才带，留空 = 保留已有密钥不覆盖。

// ---- 后端返回的只读视图 ----
interface ProjectEntry {
  job: string
  alias: string
}
interface CredentialView {
  name: string // 内部 id（隐藏）
  username: string
  hasToken: boolean
  projects: ProjectEntry[]
}
interface DeployConfigView {
  jenkinsMcpPath: string
  jenkinsUrl: string
  credentials: CredentialView[]
}

// ---- 前端编辑态（一个账号 = 一个 Jenkins 登录） ----
interface AccountRow {
  /** 内部 id（隐藏）：加载时来自后端，新建时本地生成，保存后原样回传以保持稳定。 */
  id: string
  username: string
  hasToken: boolean
  projects: ProjectEntry[]
  /** 用户本次新输入的 token；为空 = 不修改已有密钥 */
  token: string
  testState: 'idle' | 'testing' | 'ok' | 'fail'
  testMessage: string
}

const loading = ref(true)
const saving = ref(false)
const saveState = ref<'idle' | 'ok' | 'fail'>('idle')
const saveMessage = ref('')
const showAdvanced = ref(false)

const jenkinsMcpPath = ref('')
const jenkinsUrl = ref('')
const accounts = reactive<AccountRow[]>([])

// ============================================================================
// 加载：deploy_config_get -> 填表
// ============================================================================

onMounted(load)

async function load() {
  loading.value = true
  saveState.value = 'idle'
  saveMessage.value = ''
  try {
    const view = await invoke<DeployConfigView>('deploy_config_get')
    jenkinsMcpPath.value = view.jenkinsMcpPath ?? ''
    jenkinsUrl.value = view.jenkinsUrl ?? ''
    accounts.splice(0, accounts.length, ...(view.credentials ?? []).map(toAccountRow))
  } catch (e) {
    saveState.value = 'fail'
    saveMessage.value = `加载发版配置失败：${errText(e)}`
  } finally {
    loading.value = false
  }
}

function toAccountRow(c: CredentialView): AccountRow {
  return {
    id: c.name ?? '',
    username: c.username ?? '',
    hasToken: !!c.hasToken,
    projects: (c.projects ?? []).map(p => ({ job: p.job ?? '', alias: p.alias ?? '' })),
    token: '',
    testState: 'idle',
    testMessage: '',
  }
}

// ============================================================================
// 账号列表交互
// ============================================================================

/** 生成账号内部 id：`acct-<base36>`，对用户隐藏；满足后端 [A-Za-z0-9-] 校验。 */
function genAccountId(): string {
  return 'acct-' + Date.now().toString(36) + Math.random().toString(36).slice(2, 6)
}

function addAccount() {
  accounts.push({
    id: genAccountId(),
    username: '',
    hasToken: false,
    projects: [],
    token: '',
    testState: 'idle',
    testMessage: '',
  })
}

function removeAccount(i: number) {
  accounts.splice(i, 1)
}

async function testConnection(acct: AccountRow) {
  if (!jenkinsUrl.value.trim()) {
    acct.testState = 'fail'
    acct.testMessage = '请先填写 Jenkins 地址'
    return
  }
  if (!acct.username.trim()) {
    acct.testState = 'fail'
    acct.testMessage = '请先填写用户名'
    return
  }
  // 测当前**填写**的值，不必先保存：token 填了就用填的，留空则后端回退已保存的。
  acct.testState = 'testing'
  acct.testMessage = ''
  try {
    const r = await invoke<{ ok: boolean; detail: string; jenkinsUrl: string }>('deploy_test_connection', {
      name: acct.id,
      url: jenkinsUrl.value.trim(),
      username: acct.username.trim(),
      token: acct.token.trim() || undefined,
    })
    acct.testState = r.ok ? 'ok' : 'fail'
    acct.testMessage = r.detail || (r.ok ? '连接成功 ✓' : '连接失败')
  } catch (e) {
    acct.testState = 'fail'
    acct.testMessage = errText(e)
  }
}

// ============================================================================
// 项目列表交互
// ============================================================================

function addProject(acct: AccountRow) {
  acct.projects.push({ job: '', alias: '' })
}

function removeProject(acct: AccountRow, i: number) {
  acct.projects.splice(i, 1)
}

// ============================================================================
// 保存：组装 payload -> deploy_config_save
// ============================================================================

async function save() {
  saveState.value = 'idle'
  saveMessage.value = ''

  for (let i = 0; i < accounts.length; i++) {
    if (!accounts[i].username.trim()) {
      saveState.value = 'fail'
      saveMessage.value = `账号 ${i + 1}：用户名不能为空（Jenkins 鉴权必需）`
      return
    }
    // 别名必填：凡填了 job 的项目都必须有别名（与 buildPayload 的 job 过滤口径一致）。
    for (const p of accounts[i].projects) {
      if (p.job.trim() && !p.alias.trim()) {
        saveState.value = 'fail'
        saveMessage.value = `账号 ${i + 1} 的项目「${p.job.trim()}」：别名不能为空`
        return
      }
    }
  }

  saving.value = true
  try {
    await invoke('deploy_config_save', { input: buildPayload() })
    saveState.value = 'ok'
    saveMessage.value = '已保存'
    await reloadAfterSave()
  } catch (e) {
    saveState.value = 'fail'
    saveMessage.value = errText(e)
  } finally {
    saving.value = false
  }
}

function buildPayload() {
  const creds = accounts.map(a => {
    const base: { name: string; username: string; token?: string; projects: ProjectEntry[] } = {
      name: a.id, // 内部 id 作为后端 credential name
      username: a.username.trim(),
      projects: a.projects
        .filter(p => p.job.trim())
        .map(p => ({ job: p.job.trim(), alias: p.alias.trim() })),
    }
    const token = a.token.trim()
    if (token) base.token = token
    return base
  })

  return {
    jenkinsMcpPath: jenkinsMcpPath.value.trim() || undefined,
    jenkinsUrl: jenkinsUrl.value.trim(),
    credentials: creds,
  }
}

/** 保存成功后重新拉取视图，按内部 id 刷新 hasToken / username 并清空本地 token 输入。 */
async function reloadAfterSave() {
  try {
    const view = await invoke<DeployConfigView>('deploy_config_get')
    jenkinsUrl.value = view.jenkinsUrl ?? ''
    const byId = new Map((view.credentials ?? []).map(c => [c.name, c]))
    for (const row of accounts) {
      const fresh = byId.get(row.id)
      if (fresh) {
        row.username = fresh.username ?? row.username
        row.hasToken = !!fresh.hasToken
        row.token = ''
      }
    }
  } catch {
    // 重拉失败不影响「已保存」结论，忽略。
  }
}

// ============================================================================
// 工具
// ============================================================================

function errText(e: unknown): string {
  if (e instanceof Error) return e.message
  return String(e)
}
</script>

<template>
  <section class="settings-section">
    <h3 class="settings-section-title">对话式发版</h3>

    <p v-if="loading" class="settings-section-hint" style="text-align:center;padding:16px 0;">
      正在加载发版配置…
    </p>

    <template v-else>
      <!-- ============ Jenkins 地址 ============ -->
      <label class="settings-field">
        <span class="settings-field-label">Jenkins 地址</span>
        <input v-model="jenkinsUrl" class="settings-input" type="url" placeholder="http://jenkins.example.com" />
      </label>

      <!-- ============ 账号卡片 ============ -->
      <div v-if="accounts.length === 0" class="deploy-empty">还没有账号，点下方按钮添加</div>

      <div v-for="(acct, ai) in accounts" :key="acct.id" class="deploy-card">
        <!-- 头部：账号序号 + 删除 -->
        <div class="deploy-card-head">
          <span class="deploy-card-title">账号 {{ ai + 1 }}</span>
          <button class="deploy-del-btn" title="删除账号" @click="removeAccount(ai)">×</button>
        </div>

        <!-- 用户名 -->
        <label class="settings-field">
          <span class="settings-field-label">用户名</span>
          <input
            v-model="acct.username"
            class="settings-input"
            type="text"
            placeholder="Jenkins 用户名（User ID）"
          />
        </label>
        <p class="deploy-field-hint">
          = Jenkins User ID（不是显示名）。在 Jenkins 点右上角进个人页，地址栏 <code>…/user/xxx/</code> 里的 <code>xxx</code> 就是。
        </p>

        <!-- token -->
        <label class="settings-field">
          <span class="settings-field-label">token</span>
          <input
            v-model="acct.token"
            class="settings-input"
            type="password"
            :placeholder="acct.hasToken ? '已保存，留空则不修改' : 'Jenkins API Token'"
          />
        </label>

        <!-- 测试连接 -->
        <div class="settings-actions">
          <button class="settings-btn" :disabled="acct.testState === 'testing'" @click="testConnection(acct)">
            {{ acct.testState === 'testing' ? '测试中…' : '测试连接' }}
          </button>
        </div>
        <p v-if="!acct.testMessage" class="deploy-field-hint">测试用当前填写的信息，不必先保存；测通后再点底部「保存」。</p>
        <p v-if="acct.testMessage" class="settings-msg" :class="`settings-msg-${acct.testState === 'testing' ? 'testing' : acct.testState}`">
          {{ acct.testMessage }}
        </p>

        <!-- 项目列表 -->
        <div class="deploy-projects">
          <div class="deploy-projects-label">项目</div>

          <div v-if="acct.projects.length === 0" class="deploy-empty deploy-empty-inner">
            还没有项目，点下方按钮添加
          </div>

          <div v-for="(proj, pi) in acct.projects" :key="pi" class="deploy-project-row">
            <input v-model="proj.job" class="settings-input deploy-project-job" type="text" placeholder="job 名" />
            <input v-model="proj.alias" class="settings-input deploy-project-alias" type="text" placeholder="别名" />
            <button class="deploy-del-btn" title="删除项目" @click="removeProject(acct, pi)">×</button>
          </div>

          <button class="settings-btn deploy-add-btn deploy-add-sm" @click="addProject(acct)">+ 添加项目</button>
        </div>
      </div>

      <!-- ============ 底部操作 ============ -->
      <div class="deploy-bottom-actions">
        <button class="settings-btn deploy-add-btn" @click="addAccount">+ 添加账号</button>
        <button class="settings-btn settings-btn-primary" :disabled="saving" @click="save">
          {{ saving ? '保存中…' : '保存' }}
        </button>
      </div>
      <p v-if="saveMessage" class="settings-msg" :class="`settings-msg-${saveState === 'idle' ? 'testing' : saveState}`">
        {{ saveMessage }}
      </p>

      <!-- ============ 高级：jenkins-mcp 路径 ============ -->
      <div class="deploy-advanced">
        <button class="deploy-advanced-toggle" @click="showAdvanced = !showAdvanced">
          <span>{{ showAdvanced ? '▾' : '▸' }}</span>
          <span>高级：jenkins-mcp 路径</span>
        </button>
        <div v-if="showAdvanced" class="deploy-advanced-body">
          <label class="settings-field">
            <span class="settings-field-label">入口路径</span>
            <input v-model="jenkinsMcpPath" class="settings-input" type="text" placeholder="jenkins-mcp 的 dist/index.js 绝对路径" />
          </label>
          <p class="settings-section-hint">通常无需修改，留空将沿用现有或默认路径。</p>
        </div>
      </div>
    </template>
  </section>
</template>

<style scoped>
.deploy-card {
  display: flex;
  flex-direction: column;
  gap: 3px;
  padding: 8px;
  background: var(--surface-2);
  border: var(--divider-soft);
  border-radius: 6px;
}
.deploy-card-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 6px;
}
.deploy-card-title {
  font-weight: 600;
  font-size: 12px;
  color: var(--text-dim);
}
.deploy-field-hint {
  margin: 0 0 2px;
  font-size: 10.5px;
  line-height: 1.5;
  color: var(--text-dim);
}
.deploy-field-hint code {
  font-size: 10px;
  padding: 0 3px;
  border-radius: 3px;
  background: var(--surface);
  color: var(--text);
}
.deploy-del-btn {
  flex-shrink: 0;
  width: 22px;
  height: 22px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: none;
  border: none;
  color: var(--text-faint);
  cursor: pointer;
  border-radius: 4px;
  font-size: 15px;
  line-height: 1;
}
.deploy-del-btn:hover {
  color: var(--danger);
  background: color-mix(in srgb, var(--danger) 10%, transparent);
}
.deploy-projects {
  display: flex;
  flex-direction: column;
  gap: 3px;
  margin-top: 4px;
  padding: 6px;
  background: var(--surface);
  border: var(--divider-soft);
  border-radius: 5px;
}
.deploy-projects-label {
  font-size: 10.5px;
  color: var(--text-dim);
}
.deploy-project-row {
  display: flex;
  align-items: center;
  gap: 6px;
}
.deploy-project-job {
  flex: 1;
}
.deploy-project-alias {
  flex: 1;
}
.deploy-add-btn {
  font-size: 12px;
  color: var(--accent-text);
  align-self: flex-start;
}
.deploy-add-sm {
  font-size: 11px;
  padding: 4px 10px;
}
.deploy-empty {
  padding: 8px;
  font-size: 11px;
  color: var(--text-dim);
  text-align: center;
}
.deploy-empty-inner {
  padding: 4px;
}
.deploy-bottom-actions {
  display: flex;
  align-items: center;
  justify-content: space-between;
}
.deploy-advanced {
  margin-top: 4px;
}
.deploy-advanced-toggle {
  display: flex;
  align-items: center;
  gap: 5px;
  padding: 0;
  background: none;
  border: none;
  color: var(--text-dim);
  font-size: 11.5px;
  cursor: pointer;
}
.deploy-advanced-toggle:hover {
  color: var(--text);
}
.deploy-advanced-body {
  display: flex;
  flex-direction: column;
  gap: 4px;
  margin-top: 4px;
}
</style>
