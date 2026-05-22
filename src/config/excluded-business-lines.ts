import { promises as fs } from 'fs'
import { homedir } from 'os'
import { join } from 'path'

/**
 * 排除的业务线名单。
 *
 * 出现在这个列表里的业务线（rootDir 下第一层目录名）会被复盘/工时统计
 * 完全忽略：commit 不计入概况、不出现在业务线分组、不分配工时。
 *
 * 典型用例：个人项目、试验仓库、不该算到公司日报里的东西。
 */
export type ExcludedBusinessLines = string[]

const FILE_PATH = join(homedir(), '.jarvis', 'excluded-business-lines.json')

/**
 * 默认排除项。文件不存在时写一份初始版本，方便用户后续编辑。
 * my-mcp-servers 是 REDACTED_ACCOUNT 自己的 MCP 工具仓库，不属于公司工作。
 */
const DEFAULT_EXCLUDED: ExcludedBusinessLines = ['my-mcp-servers']

let cached: Set<string> | null = null

async function ensureFileExists(): Promise<void> {
  try {
    await fs.access(FILE_PATH)
  } catch {
    const dir = join(homedir(), '.jarvis')
    await fs.mkdir(dir, { recursive: true })
    await fs.writeFile(FILE_PATH, JSON.stringify(DEFAULT_EXCLUDED, null, 2), 'utf-8')
  }
}

export async function loadExcludedBusinessLines(): Promise<Set<string>> {
  if (cached) return cached
  try {
    await ensureFileExists()
    const content = await fs.readFile(FILE_PATH, 'utf-8')
    const parsed = JSON.parse(content)
    if (!Array.isArray(parsed)) {
      cached = new Set()
      return cached
    }
    cached = new Set(
      parsed.filter((x): x is string => typeof x === 'string' && x.trim().length > 0),
    )
    return cached
  } catch {
    cached = new Set()
    return cached
  }
}

/** 测试用：清缓存 */
export function _clearCache() {
  cached = null
}

/** 配置文件路径（供 UI 编辑或日志显示） */
export function excludedFilePath(): string {
  return FILE_PATH
}
