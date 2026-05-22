// 高级 API：把 scan.ts 的底层函数包成 listMyLocalCommits / getLocalCommitDiff，
// 接口与原来的 tencentcode-mcp 一致，让上游（commit-link-service 等）改动最小。
//
// 与 MCP 版本相比的变化：
// - 不需要任何 spawn / connect / close，纯函数调用
// - rootDir 不传时回退到环境变量 TENCENT_CODE_LOCAL_ROOTS（兼容旧逻辑），
//   后续 task #13 会改为读 settings.repoRoots

import { promises as fs } from 'fs'
import {
  ensureGitAvailable,
  findGitRepos,
  getDateRange,
  getDefaultGitIdentities,
  getCommitDiff,
  getLocalCommits,
  runWithConcurrency,
  type CommitDiff,
  type LocalCommit,
  type MatchDimension,
  type RangePreset,
  type RepoCommits,
} from './scan.js'

export type { LocalCommit, RepoCommits, RangePreset, CommitDiff } from './scan.js'

export interface ListMyLocalCommitsInput {
  rootDir?: string | string[]
  range?: RangePreset
  since?: string
  until?: string
  author?: string
  match?: MatchDimension
  includeBody?: boolean
  includeStat?: boolean
  maxDepth?: number
}

export interface ListMyLocalCommitsResult {
  range: { since: string; until: string; label: string }
  authors: string[]
  rootDirs: string[]
  repos: RepoCommits[]
  totalCommits: number
  scannedRepos: number
  reposWithCommits?: number
}

function resolveRootDirs(input: string | string[] | undefined): string[] {
  const collect = (raw: string | undefined): string[] => {
    if (!raw) return []
    return raw
      .split(/[;,]/)
      .map(s => s.trim())
      .filter(Boolean)
  }

  if (Array.isArray(input)) return input.map(s => s.trim()).filter(Boolean)
  if (typeof input === 'string' && input.trim()) return [input.trim()]
  return collect(process.env.TENCENT_CODE_LOCAL_ROOTS)
}

/**
 * 列出"我"在本地仓库的提交。
 *
 * 不再走 MCP 子进程，直接在当前 Node 进程内 git log。
 */
export async function listMyLocalCommits(
  input: ListMyLocalCommitsInput = {},
): Promise<ListMyLocalCommitsResult> {
  const rootDirs = resolveRootDirs(input.rootDir)
  if (rootDirs.length === 0) {
    throw new Error(
      "缺少 rootDir。请在 settings 里配置代码根目录，或传入 rootDir 参数（如 'D:/coding'）",
    )
  }

  for (const dir of rootDirs) {
    try {
      const stat = await fs.stat(dir)
      if (!stat.isDirectory()) throw new Error(`rootDir 不是目录: ${dir}`)
    } catch {
      throw new Error(`rootDir 不存在或不可访问: ${dir}`)
    }
  }

  await ensureGitAvailable().catch(err => {
    throw new Error(`未找到 git，请确认 git 已安装并在 PATH 中。原始错误: ${err.message}`)
  })

  const rangePreset = input.range ?? 'today'
  const presetRange = getDateRange(rangePreset)
  const since = input.since ?? presetRange.since
  const until = input.until ?? presetRange.until

  let authors: string[]
  if (input.author && input.author.trim()) {
    authors = input.author.split(',').map(s => s.trim()).filter(Boolean)
  } else {
    authors = await getDefaultGitIdentities(rootDirs[0])
  }

  const maxDepth = input.maxDepth ?? 5
  const match: MatchDimension = input.match ?? 'author'
  const includeBody = Boolean(input.includeBody)
  const includeStat = Boolean(input.includeStat)

  const repos = await findGitRepos(rootDirs, maxDepth)
  if (repos.length === 0) {
    return {
      range: { since, until, label: presetRange.label },
      authors,
      rootDirs,
      repos: [],
      totalCommits: 0,
      scannedRepos: 0,
    }
  }

  const perRepo = await runWithConcurrency(repos, 8, async repo => {
    try {
      const commits = await getLocalCommits(repo, {
        authors,
        since,
        until,
        match,
        includeBody,
        includeStat,
      })
      return { repoPath: repo, commits }
    } catch {
      return { repoPath: repo, commits: [] as LocalCommit[] }
    }
  })

  const repoResults: RepoCommits[] = perRepo
    .filter(r => r.commits.length > 0)
    .map(r => ({
      repoPath: r.repoPath,
      commits: r.commits.sort((a, b) => b.authoredDate.localeCompare(a.authoredDate)),
    }))
    .sort((a, b) => {
      const aLast = a.commits[0]?.authoredDate || ''
      const bLast = b.commits[0]?.authoredDate || ''
      return bLast.localeCompare(aLast)
    })

  return {
    range: { since, until, label: presetRange.label },
    authors,
    rootDirs,
    scannedRepos: repos.length,
    reposWithCommits: repoResults.length,
    totalCommits: repoResults.reduce((s, r) => s + r.commits.length, 0),
    repos: repoResults,
  }
}

/**
 * 取单个 commit 的完整 diff。
 */
export async function getLocalCommitDiff(input: {
  repoPath: string
  sha: string
}): Promise<CommitDiff> {
  try {
    const stat = await fs.stat(input.repoPath)
    if (!stat.isDirectory()) throw new Error(`repoPath 不是目录: ${input.repoPath}`)
  } catch {
    throw new Error(`repoPath 不存在或不可访问: ${input.repoPath}`)
  }
  await ensureGitAvailable()
  return getCommitDiff(input.repoPath, input.sha)
}
