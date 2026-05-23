// 本地 git 仓库扫描 + commit 查询。
//
// 这是原 tencentcode-mcp/src/local-git.ts 的直接搬迁。设计原则：
// - 不引入第三方依赖（只用 child_process / fs / path）
// - 抛 Error 而不是返回 isError 结果（MCP 协议层那种）
// - 类型字段保持和 MCP 版本一致，方便上层零改动
//
// 调用入口在 ./index.ts，那里包了 listMyLocalCommits / getLocalCommitDiff
// 两个高级 API。

import { spawn } from 'child_process'
import { promises as fs } from 'fs'
import { join } from 'path'

// ===== 类型 =====

export interface LocalCommit {
  sha: string
  shortSha: string
  authorName: string
  authorEmail: string
  authoredDate: string
  committerName: string
  committerEmail: string
  committedDate: string
  title: string
  body?: string
  stat?: CommitStat
}

export interface CommitStat {
  filesChanged: number
  insertions: number
  deletions: number
  files: Array<{ path: string; insertions: number; deletions: number; binary?: boolean }>
}

export interface RepoCommits {
  repoPath: string
  commits: LocalCommit[]
}

export interface DateRange {
  since: string
  until: string
  label: string
}

export type MatchDimension = 'author' | 'committer' | 'any'
export type RangePreset =
  | 'today'
  | 'yesterday'
  | 'thisWeek'
  | 'lastWeek'
  | 'last7days'
  | 'last30days'
  | 'thisMonth'
  | 'all'

export interface CommitDiff {
  sha: string
  authorName: string
  authorEmail: string
  authoredDate: string
  title: string
  body: string
  stat: CommitStat
  patch: string
}

// ===== 并发工具 =====

export async function runWithConcurrency<T, R>(
  items: T[],
  limit: number,
  worker: (item: T, index: number) => Promise<R>,
): Promise<R[]> {
  const results: R[] = new Array(items.length)
  let cursor = 0
  const runners = Array.from(
    { length: Math.min(limit, items.length) },
    async () => {
      while (true) {
        const i = cursor++
        if (i >= items.length) return
        results[i] = await worker(items[i], i)
      }
    },
  )
  await Promise.all(runners)
  return results
}

// ===== 日期范围预设（本地时区） =====

function pad(n: number): string {
  return String(n).padStart(2, '0')
}

function formatDate(d: Date): string {
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`
}

export function getDateRange(preset: RangePreset): DateRange {
  const now = new Date()
  const todayStart = new Date(now.getFullYear(), now.getMonth(), now.getDate())
  const tomorrowStart = new Date(todayStart.getTime() + 24 * 3600 * 1000)
  const ONE_DAY = 24 * 3600 * 1000

  let start: Date
  let end: Date

  switch (preset) {
    case 'today':
      start = todayStart
      end = tomorrowStart
      break
    case 'yesterday':
      end = todayStart
      start = new Date(end.getTime() - ONE_DAY)
      break
    case 'thisWeek': {
      const dow = todayStart.getDay() || 7
      start = new Date(todayStart.getTime() - (dow - 1) * ONE_DAY)
      end = tomorrowStart
      break
    }
    case 'lastWeek': {
      const dow = todayStart.getDay() || 7
      end = new Date(todayStart.getTime() - (dow - 1) * ONE_DAY)
      start = new Date(end.getTime() - 7 * ONE_DAY)
      break
    }
    case 'last7days':
      start = new Date(todayStart.getTime() - 7 * ONE_DAY)
      end = tomorrowStart
      break
    case 'last30days':
      start = new Date(todayStart.getTime() - 30 * ONE_DAY)
      end = tomorrowStart
      break
    case 'thisMonth':
      start = new Date(now.getFullYear(), now.getMonth(), 1)
      end = tomorrowStart
      break
    case 'all':
      // 起点 1970-01-01：足够久远，覆盖任何项目的最早 commit
      start = new Date(0)
      end = tomorrowStart
      break
    default:
      start = todayStart
      end = tomorrowStart
  }

  return {
    since: start.toISOString(),
    until: end.toISOString(),
    label: preset === 'all'
      ? '全部'
      : `${formatDate(start)} ~ ${formatDate(new Date(end.getTime() - 1))}`,
  }
}

// ===== git 子进程 =====

export function runGitCmd(args: string[], cwd?: string, timeoutMs = 15000): Promise<string> {
  return new Promise((resolve, reject) => {
    const proc = spawn('git', args, { cwd, shell: false, windowsHide: true })
    let stdout = ''
    let stderr = ''
    let finished = false
    const timer = setTimeout(() => {
      if (finished) return
      finished = true
      proc.kill()
      reject(new Error(`git ${args.join(' ')} 超时 (${timeoutMs}ms)`))
    }, timeoutMs)

    proc.stdout.on('data', d => (stdout += d.toString('utf8')))
    proc.stderr.on('data', d => (stderr += d.toString('utf8')))
    proc.on('close', code => {
      if (finished) return
      finished = true
      clearTimeout(timer)
      if (code === 0) resolve(stdout)
      else reject(new Error(`git ${args.join(' ')} 退出码 ${code}: ${stderr.trim()}`))
    })
    proc.on('error', err => {
      if (finished) return
      finished = true
      clearTimeout(timer)
      reject(err)
    })
  })
}

export async function ensureGitAvailable(): Promise<void> {
  await runGitCmd(['--version'])
}

// ===== 仓库发现 =====

const SKIP_DIRS = new Set([
  'node_modules',
  'dist',
  'build',
  'out',
  '.next',
  '.cache',
  '.idea',
  '.vscode',
  'target',
  'venv',
  '.venv',
  '__pycache__',
  '.gradle',
])

export async function findGitRepos(roots: string[], maxDepth = 5): Promise<string[]> {
  const found: string[] = []
  const seen = new Set<string>()

  async function walk(dir: string, depth: number): Promise<void> {
    if (depth > maxDepth) return
    let entries: import('fs').Dirent[]
    try {
      entries = await fs.readdir(dir, { withFileTypes: true })
    } catch {
      return
    }

    // .git 可以是目录（普通仓库）或文件（worktree / submodule）
    const isRepo = entries.some(e => e.name === '.git' && (e.isDirectory() || e.isFile()))
    if (isRepo) {
      if (!seen.has(dir)) {
        seen.add(dir)
        found.push(dir)
      }
      return
    }

    for (const e of entries) {
      if (!e.isDirectory()) continue
      if (e.name.startsWith('.')) continue
      if (SKIP_DIRS.has(e.name)) continue
      await walk(join(dir, e.name), depth + 1)
    }
  }

  for (const root of roots) {
    await walk(root, 0)
  }
  return found
}

// ===== 身份解析 =====

const identitiesCache = new Map<string, string[]>()

export async function getDefaultGitIdentities(cwd?: string): Promise<string[]> {
  const cacheKey = cwd || '__global__'
  const cached = identitiesCache.get(cacheKey)
  if (cached) return cached

  const tryRead = async (args: string[]): Promise<string | undefined> => {
    try {
      const out = (await runGitCmd(args, cwd)).trim()
      return out || undefined
    } catch {
      return undefined
    }
  }

  const email =
    (await tryRead(['config', 'user.email'])) ||
    (await tryRead(['config', '--global', 'user.email']))
  const name =
    (await tryRead(['config', 'user.name'])) ||
    (await tryRead(['config', '--global', 'user.name']))

  const identities: string[] = []
  if (email) identities.push(email)
  if (name && name !== email) identities.push(name)

  identitiesCache.set(cacheKey, identities)
  return identities
}

export function clearIdentitiesCache(): void {
  identitiesCache.clear()
}

// ===== 提交查询 =====

const RECORD_SEP = '\x1e'
const FIELD_SEP = '\x1f'

interface GetLocalCommitsOpts {
  authors?: string[]
  since: string
  until: string
  match?: MatchDimension
  includeBody?: boolean
  includeStat?: boolean
}

export async function getLocalCommits(
  repo: string,
  opts: GetLocalCommitsOpts,
): Promise<LocalCommit[]> {
  const fields = ['%H', '%an', '%ae', '%aI', '%cn', '%ce', '%cI', '%s']
  if (opts.includeBody) fields.push('%b')
  const format = fields.join(FIELD_SEP) + RECORD_SEP

  const match = opts.match || 'author'
  const args = [
    'log',
    '--all',
    '--no-merges',
    `--since=${opts.since}`,
    `--until=${opts.until}`,
    `--pretty=format:${format}`,
  ]

  if (match === 'author') {
    for (const a of opts.authors || []) if (a) args.push(`--author=${a}`)
  } else if (match === 'committer') {
    for (const a of opts.authors || []) if (a) args.push(`--committer=${a}`)
  }
  // any 模式不传 --author/--committer，全量后 JS 过滤

  const out = await runGitCmd(args, repo)
  if (!out) return []

  let commits: LocalCommit[] = out
    .split(RECORD_SEP)
    .map(line => line.replace(/^\n/, ''))
    .filter(Boolean)
    .map(line => {
      const parts = line.split(FIELD_SEP)
      const [sha, an, ae, ad, cn, ce, cd, title, ...bodyParts] = parts
      return {
        sha: (sha || '').trim(),
        shortSha: (sha || '').trim().slice(0, 7),
        authorName: an || '',
        authorEmail: ae || '',
        authoredDate: ad || '',
        committerName: cn || '',
        committerEmail: ce || '',
        committedDate: cd || '',
        title: title || '',
        body: opts.includeBody ? bodyParts.join(FIELD_SEP).trim() || undefined : undefined,
      }
    })

  if (match === 'any' && opts.authors && opts.authors.length > 0) {
    const patterns = opts.authors.map(p => p.toLowerCase())
    commits = commits.filter(c => {
      const fields = [c.authorEmail, c.authorName, c.committerEmail, c.committerName].map(s =>
        s.toLowerCase(),
      )
      return patterns.some(p => fields.some(f => f.includes(p)))
    })
  }

  if (opts.includeStat && commits.length > 0) {
    await runWithConcurrency(commits, 4, async c => {
      c.stat = await getCommitStat(repo, c.sha)
    })
  }

  return commits
}

export async function getCommitStat(repo: string, sha: string): Promise<CommitStat> {
  const out = await runGitCmd(['show', '--numstat', '--format=', sha], repo)
  const files: CommitStat['files'] = []
  let insertions = 0
  let deletions = 0
  for (const line of out.split('\n')) {
    const trimmed = line.trim()
    if (!trimmed) continue
    const [addStr, delStr, ...pathParts] = trimmed.split('\t')
    const path = pathParts.join('\t')
    if (!path) continue
    const binary = addStr === '-' && delStr === '-'
    const ins = binary ? 0 : parseInt(addStr, 10) || 0
    const del = binary ? 0 : parseInt(delStr, 10) || 0
    files.push({ path, insertions: ins, deletions: del, binary: binary || undefined })
    insertions += ins
    deletions += del
  }
  return {
    filesChanged: files.length,
    insertions,
    deletions,
    files,
  }
}

export async function getCommitDiff(repo: string, sha: string): Promise<CommitDiff> {
  const metaFormat = ['%H', '%an', '%ae', '%aI', '%s', '%b'].join(FIELD_SEP)
  const metaOut = await runGitCmd(['show', '-s', `--format=${metaFormat}`, sha], repo)
  const [shaOut, an, ae, ad, title, ...bodyParts] = metaOut.replace(/\n$/, '').split(FIELD_SEP)

  const stat = await getCommitStat(repo, sha)
  const patch = await runGitCmd(['show', '--format=', sha], repo)

  return {
    sha: (shaOut || sha).trim(),
    authorName: an || '',
    authorEmail: ae || '',
    authoredDate: ad || '',
    title: title || '',
    body: bodyParts.join(FIELD_SEP).trim(),
    stat,
    patch: patch.replace(/^\n+/, ''),
  }
}
