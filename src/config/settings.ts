// Node 端 settings 读取层。
//
// 单一数据源：~/.jarvis/config.json，由 Rust 端 config_save 写入。Node 端只读，
// 不写——避免双写竞争。
//
// 缓存策略：首次访问时同步加载，调 reloadSettings() 清缓存（daemon 的
// /settings/reload 端点会触发这个）。
//
// 密码不在这里，task #12 实现 OS 密钥链单独读。

import { readFileSync } from 'fs'
import { homedir } from 'os'
import { join } from 'path'

export interface ZentaoSettings {
  baseUrl: string
  account: string
}

export interface WorkPeriod {
  start: string
  end: string
  label?: string
}

export interface JarvisSettings {
  zentao: ZentaoSettings
  repoRoots: string[]
  workSchedule: {
    workDays: number[]
    periods: WorkPeriod[]
  }
  notifications: {
    quietDuringLunch: boolean
    quietAfterWork: boolean
    quietOnWeekends: boolean
    morningGreeting: boolean
    eveningSummary: boolean
    eveningSummaryMinutesBefore: number
  }
  override: {
    todayMode: 'normal' | 'overtime' | 'dayoff'
    todayModeSetOn: string
  }
}

const FILE_PATH = join(homedir(), '.jarvis', 'config.json')

const DEFAULTS: JarvisSettings = {
  zentao: { baseUrl: '', account: '' },
  repoRoots: [],
  workSchedule: {
    workDays: [1, 2, 3, 4, 5],
    periods: [
      { start: '08:00', end: '12:00', label: '上午' },
      { start: '14:00', end: '18:00', label: '下午' },
    ],
  },
  notifications: {
    quietDuringLunch: true,
    quietAfterWork: true,
    quietOnWeekends: true,
    morningGreeting: true,
    eveningSummary: true,
    eveningSummaryMinutesBefore: 30,
  },
  override: { todayMode: 'normal', todayModeSetOn: '' },
}

let cached: JarvisSettings | null = null

function isObject(x: unknown): x is Record<string, unknown> {
  return typeof x === 'object' && x !== null && !Array.isArray(x)
}

/**
 * 深度合并：用户没设的字段用默认值填，保证 settings 永远是完整结构。
 * 与 Rust 端 merge_defaults 行为对齐。
 */
function mergeDefaults<T>(user: unknown, defaults: T): T {
  if (!isObject(user) || !isObject(defaults)) return (user as T) ?? defaults
  const out: Record<string, unknown> = { ...defaults }
  for (const k of Object.keys(defaults)) {
    if (k in user) {
      const dv = (defaults as any)[k]
      if (isObject(dv) && isObject(user[k])) {
        out[k] = mergeDefaults(user[k], dv)
      } else {
        out[k] = user[k]
      }
    }
  }
  return out as T
}

function loadFromDisk(): JarvisSettings {
  try {
    const raw = readFileSync(FILE_PATH, 'utf-8')
    const parsed = JSON.parse(raw)
    return mergeDefaults(parsed, DEFAULTS)
  } catch {
    return { ...DEFAULTS }
  }
}

export function getSettings(): JarvisSettings {
  if (!cached) cached = loadFromDisk()
  return cached
}

export function reloadSettings(): JarvisSettings {
  cached = loadFromDisk()
  return cached
}

/** 配置文件路径（供日志显示） */
export function settingsFilePath(): string {
  return FILE_PATH
}

// ===== 禅道凭证 =====
//
// 密码暂时还从 env 拿（task #12 会接 OS 密钥链）。
// baseUrl 和 account 已经走 settings，但保留 env 回退以兼容当前 dev 环境。

export interface ZentaoCredentials {
  baseUrl: string
  account: string
  password: string
}

export function getZentaoCredentials(): ZentaoCredentials {
  const s = getSettings()
  return {
    baseUrl: s.zentao.baseUrl
      || process.env.ZENTAO_BASE_URL
      || process.env.ZENTAO_URL
      || '',
    account: s.zentao.account
      || process.env.ZENTAO_ACCOUNT
      || process.env.ZENTAO_USER
      || '',
    password: process.env.ZENTAO_PASSWORD || process.env.ZENTAO_PASS || '',
  }
}

export function getRepoRoots(): string[] {
  const s = getSettings()
  if (s.repoRoots && s.repoRoots.length > 0) return s.repoRoots
  // 兼容旧 env
  const raw = process.env.TENCENT_CODE_LOCAL_ROOTS
  if (raw) {
    return raw.split(/[;,]/).map(x => x.trim()).filter(Boolean)
  }
  return []
}
