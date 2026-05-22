import { basename } from 'path'
import {
  listMyLocalCommitsShared,
  type ListMyLocalCommitsResult,
  type LocalCommit,
  type RangePreset,
} from '../mcp/tencentcode-client.js'
import { aliasesFor, loadBusinessAliases, type BusinessAliases } from '../config/business-aliases.js'
import { loadExcludedBusinessLines } from '../config/excluded-business-lines.js'
import { effortForCommit } from './commit-effort.js'

// ===== 类型 =====

export interface TaskInput {
  id: number | string
  name: string
}

export type MatchType = 'exact' | 'soft'

export interface CommitLink {
  sha: string
  shortSha: string
  title: string
  authoredDate: string
  repoPath: string
  /** 业务线名（rootDir 下第一层子目录，如"物流"、"示例销售线"） */
  businessLine: string
  /** 具体仓库的目录名（如 logistics-web） */
  repoName: string
  matchType: MatchType
  /** 软关联的命中关键词；精确匹配时为空 */
  matchedKeywords?: string[]
  /** 工作量分数；commit 没有 stat 时为 1。用于工时反推。 */
  effort: number
}

export interface TaskCommitLinks {
  taskId: string
  taskName: string
  commits: CommitLink[]
}

export interface CommitLinkResult {
  range: { since: string; until: string; label: string }
  scannedRepos: number
  totalCommits: number
  /** 按任务分组的关联结果 */
  tasks: TaskCommitLinks[]
  /** 没匹配到任何任务的孤儿 commit（按业务线分组） */
  orphanCommits: Array<{ businessLine: string; commits: CommitLink[] }>
}

// ===== 路径解析：从 repoPath 推业务线名 =====

/**
 * 把 Windows 反斜杠归一为正斜杠，去掉末尾斜杠
 */
function normPath(p: string): string {
  return p.replace(/\\/g, '/').replace(/\/+$/, '')
}

/**
 * 从仓库路径里提取"业务线名"。
 *
 * 策略：找到第一个匹配的 rootDir，取相对路径的第一段。
 * - D:/coding + D:/coding/物流/logistics-web    → 物流
 * - D:/coding + D:/coding/示例销售线/example-sale-app  → 示例销售线
 * - D:/coding + D:/coding/deer-flow             → deer-flow（仓库直接挂在 rootDir 下）
 *
 * 如果不在任何 rootDir 下（理论上不会），fallback 到 basename。
 */
export function extractBusinessLine(repoPath: string, rootDirs: string[]): string {
  const np = normPath(repoPath)
  for (const root of rootDirs) {
    const nr = normPath(root)
    if (np === nr) return basename(np)
    if (np.startsWith(nr + '/')) {
      const rel = np.slice(nr.length + 1)
      const seg = rel.split('/').filter(Boolean)[0]
      if (seg) return seg
    }
  }
  return basename(np)
}

// ===== 关键词提取 =====

/** 常见前缀，提取关键词时剥掉以扩大匹配面 */
const TRIM_PREFIXES = ['示例公司', '胜利工贸', '钰海工贸', '通才铁前', '通才', '鸿丰达']

/**
 * 从业务线名提取候选关键词。
 * 例如：
 *   "示例销售线"     → ["示例销售线", "销售"]
 *   "物流"         → ["物流"]
 *   "胜利工贸物流" → ["胜利工贸物流", "物流"]
 *   "钢后mes"      → ["钢后mes", "钢后", "mes"]
 *
 * 如传入 aliases，会再合并该业务线在别名表中的关键词（如"示例业务线"→["门禁","计量"]）。
 */
export function extractRepoKeywords(businessLine: string, aliases?: BusinessAliases): string[] {
  const out = new Set<string>([businessLine])
  for (const prefix of TRIM_PREFIXES) {
    if (businessLine.startsWith(prefix) && businessLine.length > prefix.length) {
      out.add(businessLine.slice(prefix.length))
    }
  }
  // 末尾常见后缀：mes
  const mesMatch = businessLine.match(/^(.+?)(mes)$/i)
  if (mesMatch) {
    out.add(mesMatch[1])
    out.add(mesMatch[2].toLowerCase())
  }
  // 别名表补充关键词
  if (aliases) {
    for (const alias of aliasesFor(businessLine, aliases)) {
      out.add(alias)
    }
  }
  return Array.from(out).filter(k => k.length >= 2)
}

// ===== 精确匹配：从 commit message 提取任务 ID =====

/**
 * 从 commit 标题/正文里抓任务号。支持以下写法：
 *   #10238   [#10238]   task-10238   task 10238   #10238:
 * 返回所有识别到的任务 ID（去重）。
 */
export function extractTaskIdsFromMessage(commit: LocalCommit): string[] {
  const text = `${commit.title}\n${commit.body ?? ''}`
  const ids = new Set<string>()
  const patterns = [
    /#(\d{3,7})\b/g,
    /\btask[-_\s]?#?(\d{3,7})\b/gi,
    /\bzentao[-_\s]?#?(\d{3,7})\b/gi,
  ]
  for (const re of patterns) {
    let m: RegExpExecArray | null
    while ((m = re.exec(text)) !== null) {
      ids.add(m[1])
    }
  }
  return Array.from(ids)
}

// ===== 软关联 =====

interface BusinessGroup {
  businessLine: string
  keywords: string[]
  /** 这个业务线下所有 commit，附带具体仓库信息 */
  commits: Array<{ commit: LocalCommit; repoPath: string; repoName: string }>
}

function matchTaskAgainstGroup(taskName: string, group: BusinessGroup): string[] {
  const hits: string[] = []
  const lowerName = taskName.toLowerCase()
  for (const kw of group.keywords) {
    if (lowerName.includes(kw.toLowerCase())) {
      hits.push(kw)
    }
  }
  return hits
}

// ===== 主入口 =====

export interface LinkCommitsOptions {
  range?: RangePreset
  since?: string
  until?: string
  rootDir?: string | string[]
  includeBody?: boolean
}

export async function linkTasksWithCommits(
  tasks: TaskInput[],
  options: LinkCommitsOptions = {},
): Promise<CommitLinkResult> {
  const range: RangePreset = options.range ?? 'today'

  const [raw, aliases, excluded] = await Promise.all([
    listMyLocalCommitsShared({
      range,
      since: options.since,
      until: options.until,
      rootDir: options.rootDir,
      includeBody: options.includeBody ?? true,
      // 工时反推需要每文件的行数变更来排除生成/锁文件
      includeStat: true,
    }),
    loadBusinessAliases(),
    loadExcludedBusinessLines(),
  ])

  const rootDirs = raw.rootDirs ?? (Array.isArray(options.rootDir)
    ? options.rootDir
    : options.rootDir ? [options.rootDir] : ['D:/coding'])

  // 按"业务线名"过滤掉用户标记为非公司项目的仓库（~/.jarvis/excluded-business-lines.json）。
  // 这些仓库的 commit 完全不进入后续统计：不计入 totalCommits、不出现在分组、不分工时。
  const rawRepos = (raw.repos ?? []).filter(
    r => !excluded.has(extractBusinessLine(r.repoPath, rootDirs)),
  )

  // 实际计入的 commit 总数（不是 raw.totalCommits，因为可能排除了一些仓库）
  const effectiveTotalCommits = rawRepos.reduce((s, r) => s + r.commits.length, 0)

  // 按业务线聚合
  const groupMap = new Map<string, BusinessGroup>()
  for (const r of rawRepos) {
    const businessLine = extractBusinessLine(r.repoPath, rootDirs)
    if (!groupMap.has(businessLine)) {
      groupMap.set(businessLine, {
        businessLine,
        keywords: extractRepoKeywords(businessLine, aliases),
        commits: [],
      })
    }
    const group = groupMap.get(businessLine)!
    for (const c of r.commits) {
      group.commits.push({
        commit: c,
        repoPath: r.repoPath,
        repoName: basename(r.repoPath),
      })
    }
  }
  const groups = Array.from(groupMap.values())

  // 按 taskId 索引
  const taskById = new Map<string, TaskInput>()
  for (const t of tasks) taskById.set(String(t.id), t)

  const taskLinks = new Map<string, CommitLink[]>()
  const usedCommitKeys = new Set<string>()

  const ensureBucket = (taskId: string) => {
    if (!taskLinks.has(taskId)) taskLinks.set(taskId, [])
    return taskLinks.get(taskId)!
  }

  const toLink = (
    item: BusinessGroup['commits'][number],
    businessLine: string,
    matchType: MatchType,
    matchedKeywords?: string[],
  ): CommitLink => ({
    sha: item.commit.sha,
    shortSha: item.commit.shortSha,
    title: item.commit.title,
    authoredDate: item.commit.authoredDate,
    repoPath: item.repoPath,
    repoName: item.repoName,
    businessLine,
    matchType,
    effort: effortForCommit(item.commit),
    ...(matchedKeywords ? { matchedKeywords } : {}),
  })

  // ----- 第一遍：精确匹配（commit message 含 #任务号） -----
  for (const group of groups) {
    for (const item of group.commits) {
      const ids = extractTaskIdsFromMessage(item.commit)
      if (ids.length === 0) continue
      for (const id of ids) {
        if (!taskById.has(id)) continue
        ensureBucket(id).push(toLink(item, group.businessLine, 'exact'))
        usedCommitKeys.add(`${item.repoPath}:${item.commit.sha}`)
      }
    }
  }

  // ----- 第二遍：软关联（业务线关键词命中任务名） -----
  for (const task of tasks) {
    const taskId = String(task.id)
    for (const group of groups) {
      const hits = matchTaskAgainstGroup(task.name, group)
      if (hits.length === 0) continue
      for (const item of group.commits) {
        const key = `${item.repoPath}:${item.commit.sha}`
        if (usedCommitKeys.has(key)) continue
        ensureBucket(taskId).push(toLink(item, group.businessLine, 'soft', hits))
      }
    }
  }

  // ----- 孤儿 commit -----
  const orphanByLine = new Map<string, CommitLink[]>()
  for (const group of groups) {
    for (const item of group.commits) {
      const key = `${item.repoPath}:${item.commit.sha}`
      if (usedCommitKeys.has(key)) continue
      const claimedBySoft = Array.from(taskLinks.values()).some(links =>
        links.some(l => l.sha === item.commit.sha && l.repoPath === item.repoPath),
      )
      if (claimedBySoft) continue
      if (!orphanByLine.has(group.businessLine)) orphanByLine.set(group.businessLine, [])
      orphanByLine.get(group.businessLine)!.push(
        toLink(item, group.businessLine, 'soft'),
      )
    }
  }

  // ----- 组装返回 -----
  const tasksOut: TaskCommitLinks[] = []
  for (const [taskId, commits] of taskLinks) {
    const task = taskById.get(taskId)
    if (!task) continue
    commits.sort((a, b) => {
      if (a.matchType !== b.matchType) return a.matchType === 'exact' ? -1 : 1
      return b.authoredDate.localeCompare(a.authoredDate)
    })
    tasksOut.push({ taskId, taskName: task.name, commits })
  }

  const orphanCommits = Array.from(orphanByLine.entries()).map(([businessLine, commits]) => ({
    businessLine,
    commits,
  }))

  return {
    range: raw.range,
    scannedRepos: raw.scannedRepos ?? 0,
    totalCommits: effectiveTotalCommits,
    tasks: tasksOut,
    orphanCommits,
  }
}
