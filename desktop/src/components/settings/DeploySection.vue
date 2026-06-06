<script setup lang="ts">
import { onMounted, reactive, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'

// ============================================================================
// 后端契约（src-tauri/src/commands/deploy_config.rs，三个 Tauri 命令已实现并提交）
// ============================================================================
//   deploy_config_get -> { jenkinsMcpPath, serverConfigured, connections[], projects }
//   deploy_config_save({ input: { jenkinsMcpPath?, connections[], projects } }) -> void
//   deploy_test_connection({ name }) -> { ok: true, detail }
//
// 数据模型铁律（避免 envName/jenkinsEnvironment 对不上）：
//   连接 name（小写字母数字/-，如 test/dev/prod）= 项目里 environment 的 map key
//   = 该环境的 jenkinsEnvironment 值。一个项目环境 = 选一个连接 + 该环境的
//   job + 分支 + 其它参数。保存时连接名同时用作 environments 的 key 和 jenkinsEnvironment。
//   branch 单独做成输入框，其余参数走通用键值对编辑器；保存时 branch 合回 params。
//
//   token：进来只有 hasToken 布尔（后端绝不回明文）；保存时仅当用户实际输入新值才放进
//   payload，省略/留空 = 保留已有密钥不覆盖。

// ---- 后端返回的只读视图 ----
interface ConnectionView {
  name: string
  url: string
  username: string
  hasToken: boolean
}
interface EnvPreset {
  job: string
  jenkinsEnvironment: string
  params: Record<string, string>
}
interface ProjectPreset {
  environments: Record<string, EnvPreset>
}
interface DeployConfigView {
  jenkinsMcpPath: string
  serverConfigured: boolean
  connections: ConnectionView[]
  projects: Record<string, ProjectPreset>
}

// ---- 前端编辑态（把扁平的键值参数拆成 branch + 其它键值对，便于编辑） ----
interface ConnRow {
  name: string
  url: string
  username: string
  /** 是否已存有密钥（来自后端 hasToken）；用于 placeholder 提示 */
  hasToken: boolean
  /** 用户本次新输入的 token；为空 = 不修改已有密钥 */
  token: string
  testState: 'idle' | 'testing' | 'ok' | 'fail'
  testMessage: string
}
interface ParamKv {
  key: string
  value: string
}
interface EnvRow {
  /** 选用的连接名（= environments 的 key = jenkinsEnvironment） */
  connection: string
  job: string
  branch: string
  /** branch 之外的参数 */
  params: ParamKv[]
}
interface ProjectRow {
  name: string
  environments: EnvRow[]
}

const loading = ref(true)
const saving = ref(false)
const saveState = ref<'idle' | 'ok' | 'fail'>('idle')
const saveMessage = ref('')
const showAdvanced = ref(false)

const jenkinsMcpPath = ref('')
const serverConfigured = ref(false)
const connections = reactive<ConnRow[]>([])
const projects = reactive<ProjectRow[]>([])

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
    serverConfigured.value = !!view.serverConfigured
    connections.splice(0, connections.length, ...(view.connections ?? []).map(toConnRow))
    projects.splice(0, projects.length, ...projectsToRows(view.projects ?? {}))
  } catch (e) {
    saveState.value = 'fail'
    saveMessage.value = `加载发版配置失败：${errText(e)}`
  } finally {
    loading.value = false
  }
}

function toConnRow(c: ConnectionView): ConnRow {
  return {
    name: c.name ?? '',
    url: c.url ?? '',
    username: c.username ?? '',
    hasToken: !!c.hasToken,
    token: '',
    testState: 'idle',
    testMessage: '',
  }
}

function projectsToRows(projectMap: Record<string, ProjectPreset>): ProjectRow[] {
  return Object.entries(projectMap).map(([name, proj]) => ({
    name,
    environments: Object.entries(proj.environments ?? {}).map(([envKey, env]) => {
      const params = env.params ?? {}
      // branch 拆出来单独编辑，其余进键值对编辑器。
      const { branch, ...rest } = params
      return {
        // environments 的 map key 就是连接名（= jenkinsEnvironment），优先用 key。
        connection: envKey || env.jenkinsEnvironment || '',
        job: env.job ?? '',
        branch: branch ?? '',
        params: Object.entries(rest).map(([key, value]) => ({ key, value: String(value) })),
      }
    }),
  }))
}

// ============================================================================
// 连接列表交互
// ============================================================================

const NAME_RE = /^[A-Za-z0-9-]+$/

function connNameError(name: string): string {
  const n = name.trim()
  if (!n) return '连接名不能为空'
  if (!NAME_RE.test(n)) return '连接名只能包含字母、数字和连字符(-)'
  return ''
}

function addConnection() {
  connections.push({
    name: '',
    url: '',
    username: '',
    hasToken: false,
    token: '',
    testState: 'idle',
    testMessage: '',
  })
}

function removeConnection(i: number) {
  connections.splice(i, 1)
}

async function testConnection(row: ConnRow) {
  const err = connNameError(row.name)
  if (err) {
    row.testState = 'fail'
    row.testMessage = err
    return
  }
  // 后端按已保存的配置测试连接：先保存再测更可靠。
  if (!serverConfigured.value || row.hasToken === false) {
    row.testState = 'fail'
    row.testMessage = '请先保存配置（含 token），再测试连接'
    return
  }
  row.testState = 'testing'
  row.testMessage = ''
  try {
    const r = await invoke<{ ok: boolean; detail: string }>('deploy_test_connection', {
      name: row.name.trim(),
    })
    row.testState = 'ok'
    row.testMessage = r.detail || '连接成功'
  } catch (e) {
    row.testState = 'fail'
    row.testMessage = errText(e)
  }
}

// ============================================================================
// 项目列表交互
// ============================================================================

function addProject() {
  projects.push({ name: '', environments: [] })
}

function removeProject(i: number) {
  projects.splice(i, 1)
}

function addEnvironment(project: ProjectRow) {
  project.environments.push({
    // 默认选第一个连接（若有），否则留空待用户选。
    connection: connections[0]?.name?.trim() ?? '',
    job: '',
    branch: '',
    params: [],
  })
}

function removeEnvironment(project: ProjectRow, i: number) {
  project.environments.splice(i, 1)
}

function addParam(env: EnvRow) {
  env.params.push({ key: '', value: '' })
}

function removeParam(env: EnvRow, i: number) {
  env.params.splice(i, 1)
}

// ============================================================================
// 保存：组装 payload -> deploy_config_save
// ============================================================================

async function save() {
  saveState.value = 'idle'
  saveMessage.value = ''

  // 前端先做连接名校验，给即时反馈（后端也会再校验）。
  for (const c of connections) {
    const err = connNameError(c.name)
    if (err) {
      saveState.value = 'fail'
      saveMessage.value = `连接「${c.name || '(未命名)'}」：${err}`
      return
    }
  }

  saving.value = true
  try {
    await invoke('deploy_config_save', { input: buildPayload() })
    saveState.value = 'ok'
    saveMessage.value = '已保存并重启发版服务'
    // 重新拉取：刷新 hasToken（新存的 token 落库后清空本地输入）、serverConfigured。
    await reloadAfterSave()
  } catch (e) {
    // reject 两种：纯校验失败（没写盘）或 配置已写盘但 jenkins-mcp 重启失败。
    // 前端无法精确区分，统一显示错误并提示「配置可能已保存，修正后重存即可」。
    saveState.value = 'fail'
    saveMessage.value = `${errText(e)}\n（若是连接 token / 路径问题，配置已保存，修正后重新保存即可。）`
  } finally {
    saving.value = false
  }
}

function buildPayload() {
  const conns = connections.map(c => {
    const base: { name: string; url: string; username: string; token?: string } = {
      name: c.name.trim(),
      url: c.url.trim(),
      username: c.username.trim(),
    }
    // token 仅在用户本次实际输入了新值时才带；留空 = 保留已有密钥不覆盖。
    const token = c.token.trim()
    if (token) base.token = token
    return base
  })

  const projectMap: Record<string, ProjectPreset> = {}
  for (const p of projects) {
    const name = p.name.trim()
    if (!name) continue
    const environments: Record<string, EnvPreset> = {}
    for (const env of p.environments) {
      const conn = env.connection.trim()
      if (!conn) continue
      const params: Record<string, string> = {}
      for (const kv of env.params) {
        const key = kv.key.trim()
        if (!key) continue
        params[key] = kv.value
      }
      // branch 合回 params（仅当非空）。
      const branch = env.branch.trim()
      if (branch) params.branch = branch
      // 连接名同时用作 environments 的 key 和 jenkinsEnvironment（铁律）。
      environments[conn] = {
        job: env.job.trim(),
        jenkinsEnvironment: conn,
        params,
      }
    }
    projectMap[name] = { environments }
  }

  return {
    jenkinsMcpPath: jenkinsMcpPath.value.trim() || undefined,
    connections: conns,
    projects: projectMap,
  }
}

/** 保存成功后重新拉取视图，但不覆盖用户刚填的报错状态（save 已设 ok）。 */
async function reloadAfterSave() {
  try {
    const view = await invoke<DeployConfigView>('deploy_config_get')
    serverConfigured.value = !!view.serverConfigured
    // 刷新连接的 hasToken 并清掉本地 token 输入（已落密钥链）。
    const byName = new Map((view.connections ?? []).map(c => [c.name, c]))
    for (const row of connections) {
      const fresh = byName.get(row.name.trim())
      if (fresh) {
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
    <p class="settings-section-hint">
      配置 Jenkins 连接与项目预设后，就能对机器人说「给某项目测试环境发个版」。发版属高危写操作，会先回显项目、环境、分支、参数等你确认，确认后才真正触发。token 加密存进系统密钥链，不写入明文配置。
    </p>

    <p v-if="loading" class="settings-section-hint" style="text-align:center;padding:16px 0;">
      正在加载发版配置…
    </p>

    <template v-else>
      <!-- ============ 块 1：Jenkins 连接列表 ============ -->
      <div class="deploy-block">
        <div class="deploy-block-title">Jenkins 连接</div>
        <p class="settings-section-hint">
          每个连接 = 一个账号 + 一个环境。连接名（如 <code>test</code> / <code>prod</code>，仅字母数字和连字符）会作为该环境的标识，在下方项目里被引用。
        </p>

        <div v-if="connections.length === 0" class="deploy-empty">还没有连接，点下方按钮添加</div>

        <div v-for="(c, ci) in connections" :key="ci" class="deploy-card">
          <div class="deploy-card-head">
            <input
              v-model="c.name"
              class="settings-input deploy-name-input"
              type="text"
              placeholder="连接名，如 test / prod"
              :class="{ 'deploy-input-bad': !!connNameError(c.name) && c.name.length > 0 }"
            />
            <button class="deploy-del-btn" title="删除连接" @click="removeConnection(ci)">×</button>
          </div>
          <label class="settings-field">
            <span class="settings-field-label">地址</span>
            <input v-model="c.url" class="settings-input" type="url" placeholder="https://jenkins.example.com" />
          </label>
          <label class="settings-field">
            <span class="settings-field-label">账号</span>
            <input v-model="c.username" class="settings-input" type="text" placeholder="Jenkins 用户名" />
          </label>
          <label class="settings-field">
            <span class="settings-field-label">token</span>
            <input
              v-model="c.token"
              class="settings-input"
              type="password"
              :placeholder="c.hasToken ? '已保存，留空则不修改' : 'Jenkins API Token'"
            />
          </label>
          <div class="settings-actions">
            <button class="settings-btn" :disabled="c.testState === 'testing'" @click="testConnection(c)">
              {{ c.testState === 'testing' ? '测试中…' : '测试连接' }}
            </button>
          </div>
          <p v-if="c.testMessage" class="settings-msg" :class="`settings-msg-${c.testState === 'testing' ? 'testing' : c.testState}`">
            {{ c.testMessage }}
          </p>
        </div>

        <button class="settings-btn deploy-add-btn" @click="addConnection">+ 新增连接</button>
      </div>

      <!-- ============ 块 2：项目列表 ============ -->
      <div class="deploy-block">
        <div class="deploy-block-title">项目预设</div>
        <p class="settings-section-hint">
          每个项目可配多个环境。每个环境 = 选一个上面的连接 + 该环境的 job + 分支 + 其它构建参数。所选连接名同时作为发版时的环境标识，杜绝默认环境误发。
        </p>

        <div v-if="projects.length === 0" class="deploy-empty">还没有项目，点下方按钮添加</div>

        <div v-for="(p, pi) in projects" :key="pi" class="deploy-card">
          <div class="deploy-card-head">
            <input
              v-model="p.name"
              class="settings-input deploy-name-input"
              type="text"
              placeholder="项目名（别名），如 人资管理端"
            />
            <button class="deploy-del-btn" title="删除项目" @click="removeProject(pi)">×</button>
          </div>

          <div v-if="p.environments.length === 0" class="deploy-empty deploy-empty-inner">
            还没有环境，点下方按钮添加
          </div>

          <div v-for="(env, ei) in p.environments" :key="ei" class="deploy-env">
            <div class="deploy-env-head">
              <span class="deploy-env-tag">环境</span>
              <button class="deploy-del-btn" title="删除环境" @click="removeEnvironment(p, ei)">×</button>
            </div>
            <label class="settings-field">
              <span class="settings-field-label">连接</span>
              <select v-model="env.connection" class="settings-input">
                <option value="" disabled>选择一个连接</option>
                <option v-for="c in connections" :key="c.name" :value="c.name.trim()">
                  {{ c.name || '(未命名)' }}
                </option>
              </select>
            </label>
            <label class="settings-field">
              <span class="settings-field-label">job</span>
              <input v-model="env.job" class="settings-input" type="text" placeholder="Jenkins job 名，如 example-access-web" />
            </label>
            <label class="settings-field">
              <span class="settings-field-label">分支</span>
              <input v-model="env.branch" class="settings-input" type="text" placeholder="如 dev / prod" />
            </label>

            <div class="deploy-params">
              <div class="deploy-params-label">其它参数</div>
              <div v-for="(kv, ki) in env.params" :key="ki" class="deploy-param-row">
                <input v-model="kv.key" class="settings-input deploy-param-key" type="text" placeholder="键，如 node_version" />
                <input v-model="kv.value" class="settings-input deploy-param-val" type="text" placeholder="值，如 nodejs-18.14.2" />
                <button class="deploy-del-btn" title="删除参数" @click="removeParam(env, ki)">×</button>
              </div>
              <button class="settings-btn deploy-add-btn deploy-add-sm" @click="addParam(env)">+ 参数</button>
            </div>
          </div>

          <button class="settings-btn deploy-add-btn deploy-add-sm" @click="addEnvironment(p)">+ 新增环境</button>
        </div>

        <button class="settings-btn deploy-add-btn" @click="addProject">+ 新增项目</button>
      </div>

      <!-- ============ 块 3：jenkins-mcp 路径（高级，折叠） ============ -->
      <div class="deploy-block">
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

      <!-- ============ 保存 ============ -->
      <div class="settings-actions">
        <button class="settings-btn settings-btn-primary" :disabled="saving" @click="save">
          {{ saving ? '保存中…' : '保存' }}
        </button>
      </div>
      <p v-if="saveMessage" class="settings-msg" :class="`settings-msg-${saveState === 'idle' ? 'testing' : saveState}`">
        {{ saveMessage }}
      </p>
    </template>
  </section>
</template>

<style scoped>
.deploy-block {
  display: flex;
  flex-direction: column;
  gap: 5px;
  padding: 10px;
  background: var(--surface);
  border: var(--divider);
  border-radius: 6px;
}
.deploy-block-title {
  font-size: 12px;
  font-weight: 600;
  color: var(--text);
}
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
  gap: 6px;
}
.deploy-name-input {
  flex: 1;
  font-weight: 600;
}
.deploy-input-bad {
  border-color: var(--danger);
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
.deploy-env {
  display: flex;
  flex-direction: column;
  gap: 3px;
  margin-top: 4px;
  padding: 7px;
  background: var(--surface);
  border: var(--divider-soft);
  border-radius: 5px;
}
.deploy-env-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
}
.deploy-env-tag {
  font-size: 10px;
  color: var(--text-dim);
  text-transform: uppercase;
  letter-spacing: 0.5px;
}
.deploy-params {
  display: flex;
  flex-direction: column;
  gap: 3px;
  margin-top: 4px;
}
.deploy-params-label {
  font-size: 10.5px;
  color: var(--text-dim);
}
.deploy-param-row {
  display: flex;
  align-items: center;
  gap: 6px;
}
.deploy-param-key {
  flex: 0 0 38%;
}
.deploy-param-val {
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
