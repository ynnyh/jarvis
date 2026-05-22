import { promises as fs } from 'fs'
import { homedir } from 'os'
import { join } from 'path'

/**
 * 业务线别名配置。
 *
 * Key   = `D:/coding/` 下的目录名（业务线名，如"示例业务线"）
 * Value = 该目录下"实际包含的系统名"列表，用于补充关键词匹配
 *
 * 例：`示例业务线` 目录下其实是"门禁"和"计量"两个系统的代码，但禅道任务
 * 的描述只会写"门禁系统"或"计量"，不会写"无人值守"。所以需要别名表把
 * 这些系统关键词补充到该目录的关键词集里。
 */
export type BusinessAliases = Record<string, string[]>

const FILE_PATH = join(homedir(), '.jarvis', 'business-aliases.json')

/**
 * 默认别名表。文件不存在时用它写一份初始版本，方便用户后续直接编辑。
 */
const DEFAULT_ALIASES: BusinessAliases = {
  示例业务线: ['门禁', '计量'],
}

let cached: BusinessAliases | null = null

async function ensureFileExists(): Promise<void> {
  try {
    await fs.access(FILE_PATH)
  } catch {
    const dir = join(homedir(), '.jarvis')
    await fs.mkdir(dir, { recursive: true })
    await fs.writeFile(FILE_PATH, JSON.stringify(DEFAULT_ALIASES, null, 2), 'utf-8')
  }
}

export async function loadBusinessAliases(): Promise<BusinessAliases> {
  if (cached) return cached
  try {
    await ensureFileExists()
    const content = await fs.readFile(FILE_PATH, 'utf-8')
    const parsed = JSON.parse(content)
    if (typeof parsed !== 'object' || parsed === null) {
      cached = {}
      return cached
    }
    // 清洗：value 必须是字符串数组
    const clean: BusinessAliases = {}
    for (const [k, v] of Object.entries(parsed)) {
      if (Array.isArray(v)) {
        clean[k] = v.filter(x => typeof x === 'string' && x.trim().length > 0)
      }
    }
    cached = clean
    return clean
  } catch {
    cached = {}
    return cached
  }
}

/** 给某个业务线名取出所有补充关键词（不含自身） */
export function aliasesFor(businessLine: string, aliases: BusinessAliases): string[] {
  return aliases[businessLine] ?? []
}

/** 测试用：清缓存 */
export function _clearCache() {
  cached = null
}

/** 配置文件路径（供 UI 编辑或日志显示） */
export function aliasesFilePath(): string {
  return FILE_PATH
}
