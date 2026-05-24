import { z } from 'zod'
import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { homedir } from 'node:os'
import { toolRegistry } from '../core/tool-registry.js'
import type { Tool } from '../core/tool-registry.js'

/**
 * 从 CC Switch（~/.cc-switch/）一键拉取当前激活的 Codex（OpenAI 兼容）provider 配置。
 *
 * 数据布局：
 *   ~/.cc-switch/settings.json        → 含 currentProviderCodex（uuid）
 *   ~/.cc-switch/cc-switch.db         → SQLite，表 providers 的 settings_config 字段是 JSON
 *
 * settings_config 形态（codex 类型）：
 *   {
 *     "auth": { "OPENAI_API_KEY": "..." },
 *     "config": "...Codex CLI 风格的 TOML 文本..."
 *   }
 *
 * config 是 TOML，但只有几个字段我们关心，正则抠出来即可（避免引一个 TOML 解析器）：
 *   - 顶层 model = "..."
 *   - 顶层 model_provider = "..."（指向 [model_providers.xxx] section）
 *   - 对应 section 下 base_url = "..."
 *
 * 注意：CC Switch 的 Codex provider 用的是 responses API（/v1/responses），
 * 我们的 LLM client 拼的是 /v1/chat/completions。导入后能不能直接跑取决于
 * 上游是否也开了 chat completions 端点。这点由 UI 提示用户。
 */

const inputSchema = z.object({})

interface ImportResult {
  found: boolean
  /** 失败原因。found=false 时填 */
  reason?: string
  /** 成功时填 */
  provider?: {
    name: string
    apiKey: string
    baseUrl: string
    model: string
    /** 上游用的 wire api 协议提示。'responses' 表示 Codex CLI 协议（不能直接对接），'chat' 兼容 */
    wireApi?: string
  }
}

async function execute(): Promise<ImportResult> {
  const ccDir = join(homedir(), '.cc-switch')
  const settingsPath = join(ccDir, 'settings.json')
  const dbPath = join(ccDir, 'cc-switch.db')

  if (!existsSync(settingsPath) || !existsSync(dbPath)) {
    return { found: false, reason: '未检测到 CC Switch（~/.cc-switch/ 目录不完整）' }
  }

  let currentId: string | null = null
  try {
    const raw = readFileSync(settingsPath, 'utf-8')
    const json = JSON.parse(raw)
    currentId = typeof json.currentProviderCodex === 'string' ? json.currentProviderCodex : null
  } catch (e: any) {
    return { found: false, reason: `CC Switch settings.json 解析失败: ${e?.message || e}` }
  }
  if (!currentId) {
    return { found: false, reason: 'CC Switch 没有选定的 Codex（OpenAI）provider，请先在 CC Switch 里切换到一个' }
  }

  // node 内置 sqlite（node 22+ 稳定；20+ experimental）。读 readOnly 模式不会和 CC Switch 写冲突。
  // 旧 node 没这个，捕获 ImportError 给清晰提示。
  let DatabaseSync: any
  try {
    ;({ DatabaseSync } = await import('node:sqlite'))
  } catch (e: any) {
    return { found: false, reason: `当前 Node 版本不支持 node:sqlite，无法读取 CC Switch 数据库: ${e?.message || e}` }
  }

  let db: any
  try {
    db = new DatabaseSync(dbPath, { readOnly: true })
  } catch (e: any) {
    return { found: false, reason: `打开 CC Switch 数据库失败: ${e?.message || e}` }
  }

  try {
    const row = db
      .prepare('SELECT id, name, settings_config FROM providers WHERE id = ? AND app_type = ?')
      .get(currentId, 'codex') as { id: string; name: string; settings_config: string } | undefined

    if (!row) {
      return { found: false, reason: `在 CC Switch 数据库里找不到当前 Codex provider (id=${currentId})` }
    }

    let config: any
    try {
      config = JSON.parse(row.settings_config || '{}')
    } catch (e: any) {
      return { found: false, reason: `CC Switch provider 的 settings_config 不是合法 JSON: ${e?.message || e}` }
    }

    const apiKey = config?.auth?.OPENAI_API_KEY
    if (typeof apiKey !== 'string' || !apiKey.trim()) {
      return { found: false, reason: `CC Switch provider 「${row.name}」未配置 OPENAI_API_KEY` }
    }

    const tomlText: string = typeof config?.config === 'string' ? config.config : ''
    const parsed = parseCodexToml(tomlText)
    if (!parsed.baseUrl) {
      return { found: false, reason: `CC Switch provider 「${row.name}」的 base_url 解析失败（找不到 [model_providers.${parsed.providerName || '*'}] 段的 base_url）` }
    }

    return {
      found: true,
      provider: {
        name: row.name,
        apiKey: apiKey.trim(),
        baseUrl: parsed.baseUrl,
        model: parsed.model || 'gpt-4o-mini',
        wireApi: parsed.wireApi,
      },
    }
  } finally {
    try { db.close() } catch { /* noop */ }
  }
}

/**
 * 从 Codex CLI 风格的 TOML 文本里抠出 model / model_provider / 对应 section 的 base_url。
 *
 * 只识别 `key = "value"` 简单形式。CC Switch 写出来的内容确定是这种格式，
 * 不引入完整 TOML 解析器是为了少依赖。
 */
function parseCodexToml(text: string): { model?: string; baseUrl?: string; providerName?: string; wireApi?: string } {
  // 顶层 model = "..."（要在第一个 [section] 之前）
  const topBlock = text.split(/^\s*\[[^\]]+\]\s*$/m, 1)[0]
  const model = matchString(topBlock, /^\s*model\s*=\s*"([^"]+)"/m)
  const providerName = matchString(topBlock, /^\s*model_provider\s*=\s*"([^"]+)"/m)

  let baseUrl: string | undefined
  let wireApi: string | undefined
  if (providerName) {
    // 找 [model_providers.{providerName}] section 的内容。
    // 这里不要用 m 标志，否则 `$` 会被解读为"行尾"，让非贪婪捕获在第一行就停。
    // 用 \n[ 显式锚定下一个 section 头，或文本结束 ($) 即字符串结束。
    const sectionRe = new RegExp(
      `\\[model_providers\\.${escapeRegex(providerName)}\\]([\\s\\S]*?)(?=\\n\\[|$)`,
    )
    const m = text.match(sectionRe)
    const section = m ? m[1] : ''
    baseUrl = matchString(section, /^\s*base_url\s*=\s*"([^"]+)"/m)
    wireApi = matchString(section, /^\s*wire_api\s*=\s*"([^"]+)"/m)
  }

  // baseUrl 兜底：直接全文找第一个 base_url = "..."
  if (!baseUrl) {
    baseUrl = matchString(text, /^\s*base_url\s*=\s*"([^"]+)"/m)
  }

  return { model, baseUrl, providerName, wireApi }
}

function matchString(s: string, re: RegExp): string | undefined {
  const m = s.match(re)
  return m ? m[1] : undefined
}

function escapeRegex(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
}

export const ccSwitchImportTool: Tool = {
  metadata: {
    name: 'cc_switch_import',
    description: '从 CC Switch（~/.cc-switch/）一键拉取当前激活的 Codex（OpenAI 兼容）provider 配置：apiKey + baseUrl + model + name。',
    category: 'config',
    version: '1.0.0',
    inputSchema,
    examples: [{ input: {}, description: '导入当前激活的 Codex provider' }],
  },
  execute: execute as any,
}

toolRegistry.register(ccSwitchImportTool)
