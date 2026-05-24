import { basename } from 'path'
import {
  listMyLocalCommits,
  type ListMyLocalCommitsResult,
  type LocalCommit,
  type RangePreset,
} from './local-git/index.js'
import { aliasesFor, loadBusinessAliases, type BusinessAliases } from '../config/business-aliases.js'
import { loadExcludedBusinessLines } from '../config/excluded-business-lines.js'
import { effortForCommit } from './commit-effort.js'
import { getLlmClient } from '../llm/client.js'

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
  /** LLM 评分的置信度 0~1。仅 soft + useLlm 时填充。 */
  confidence?: number
  /** LLM 给出的一句话理由。仅 soft + useLlm 时填充。 */
  reason?: string
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
  /** 对 soft 匹配跑 LLM 评分；低于 minConfidence 的会被丢掉。失败回退到规则结果。 */
  useLlm?: boolean
  /** LLM 评分阈值，默认 0.4。低于阈值的 soft 匹配会被丢弃。 */
  minConfidence?: number
}

export async function linkTasksWithCommits(
  tasks: TaskInput[],
  options: LinkCommitsOptions = {},
): Promise<CommitLinkResult> {
  const range: RangePreset = options.range ?? 'today'

  const [raw, aliases, excluded] = await Promise.all([
    listMyLocalCommits({
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

  // ----- 第三遍（可选）：用 LLM 给 soft 匹配打分，丢掉低置信度的 -----
  if (options.useLlm) {
    const threshold = options.minConfidence ?? 0.4
    try {
      const scoreMap = await scoreSoftMatchesWithLlm(tasks, taskLinks)
      for (const [taskId, links] of taskLinks) {
        const kept: CommitLink[] = []
        for (const link of links) {
          if (link.matchType !== 'soft') {
            kept.push(link)
            continue
          }
          const score = scoreMap.get(`${taskId}|${link.sha}`)
          // LLM 没给到分数：保守保留（不删），但不写 confidence
          if (!score) { kept.push(link); continue }
          if (score.confidence < threshold) continue  // 丢弃
          kept.push({ ...link, confidence: score.confidence, reason: score.reason })
        }
        taskLinks.set(taskId, kept)
      }
    } catch {
      // LLM 失败：保留规则结果，调用方拿不到 confidence 字段就知道没评分
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

// ===== LLM 评分 =====

/**
 * 给 soft 匹配打分。
 *
 * 输入：每个 (taskId, taskName) 候选的 commit 列表（只看 soft 类型）。
 * 输出：Map<"taskId|sha", {confidence, reason}>。
 *
 * Prompt 策略：一次性把所有候选丢给 LLM 评分，避免每个 commit 一次请求。
 * 要求严格输出 JSON 数组——不行则 throw，上层回退到规则结果。
 * 温度 0.1——评分要稳定，不要发散。
 */
async function scoreSoftMatchesWithLlm(
  tasks: TaskInput[],
  taskLinks: Map<string, CommitLink[]>,
): Promise<Map<string, { confidence: number; reason: string }>> {
  const taskNameById = new Map(tasks.map(t => [String(t.id), t.name]))

  type Candidate = { taskId: string; taskName: string; sha: string; title: string; businessLine: string; keywords?: string[] }
  const candidates: Candidate[] = []
  for (const [taskId, links] of taskLinks) {
    const taskName = taskNameById.get(taskId) ?? ''
    for (const link of links) {
      if (link.matchType !== 'soft') continue
      candidates.push({
        taskId,
        taskName,
        sha: link.sha,
        title: link.title,
        businessLine: link.businessLine,
        keywords: link.matchedKeywords,
      })
    }
  }

  const out = new Map<string, { confidence: number; reason: string }>()
  if (candidates.length === 0) return out

  const client = getLlmClient()
  const res = await client.chat({
    temperature: 0.1,
    maxTokens: Math.min(4000, 200 + candidates.length * 60),
    messages: [
      {
        role: 'system',
        content:
          '你是一个代码提交与任务关联的评分助手。\n' +
          '给定一组 (任务, commit) 候选对，判断这个 commit 是否真的在推进这个任务。\n' +
          '严格按 JSON 输出，每项包含 taskId、sha、confidence (0~1, 两位小数)、reason (一句话中文)。\n' +
          '评分参考：\n' +
          '- 0.8~1.0：commit 标题直接指向任务的功能点\n' +
          '- 0.5~0.79：commit 在同业务线下，且涉及任务相关的模块\n' +
          '- 0.2~0.49：仅业务线匹配，但 commit 在做不相干的事\n' +
          '- 0~0.19：明显无关\n' +
          '只输出 JSON 数组本身，不要 ```json 包裹，不要解释。',
      },
      {
        role: 'user',
        content:
          '候选对：\n```json\n' + JSON.stringify(candidates, null, 2) + '\n```\n' +
          '请返回形如 [{"taskId":"123","sha":"abc","confidence":0.8,"reason":"..."}, ...] 的 JSON 数组。',
      },
    ],
  })

  const parsed = parseLlmJsonArray(res.text)
  for (const row of parsed) {
    const taskId = String(row?.taskId ?? '')
    const sha = String(row?.sha ?? '')
    const confidence = Number(row?.confidence)
    const reason = String(row?.reason ?? '')
    if (!taskId || !sha || !Number.isFinite(confidence)) continue
    out.set(`${taskId}|${sha}`, {
      confidence: Math.max(0, Math.min(1, confidence)),
      reason,
    })
  }
  return out
}

/**
 * 从 LLM 输出里抠出 JSON 数组。
 * 容忍 ```json 围栏、前后多余文字。失败抛错让上层回退。
 */
function parseLlmJsonArray(text: string): any[] {
  let s = text.trim()
  // 剥 ```json ... ``` 围栏
  const fence = s.match(/```(?:json)?\s*([\s\S]*?)```/i)
  if (fence) s = fence[1].trim()
  // 抠出第一个 [...] 块
  const start = s.indexOf('[')
  const end = s.lastIndexOf(']')
  if (start < 0 || end < 0 || end <= start) {
    throw new Error(`LLM 输出不含 JSON 数组: ${text.slice(0, 200)}`)
  }
  const slice = s.slice(start, end + 1)
  const parsed = JSON.parse(slice)
  if (!Array.isArray(parsed)) throw new Error('LLM JSON 不是数组')
  return parsed
}
