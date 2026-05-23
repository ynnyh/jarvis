import type { CommitLink, CommitLinkResult } from './commit-link-service.js'
import { cleanCommitTitle } from './clean-commit-title.js'

export interface ReviewTaskInfo {
  id: string | number
  name: string
  status: 'wait' | 'doing' | 'done' | 'closed' | 'cancel' | string
}

export interface AdvancedTask {
  taskId: string
  taskName: string
  status: string
  commitCount: number
  commits: CommitLink[]
  /** 业务线 */
  businessLine: string
  /** 工作量分数（该任务下所有 commit effort 之和，已去重 commit-sha） */
  effort: number
  /** 建议工时（小时，四舍五入到 0.5）：业务线工时 / 该业务线任务数 */
  suggestedHours?: number
}

export interface BusinessLineGroup {
  businessLine: string
  commits: CommitLink[]
  /** 这条业务线下涉及的任务（去重） */
  tasks: Array<{ taskId: string; taskName: string }>
  /** 该业务线去重 commit 的 effort 总和；用于在多业务线之间按工作量分配工时 */
  effort: number
  /** 按 effort 比例估算的建议工时（小时，四舍五入到 0.5） */
  suggestedHours?: number
}

export interface NeedsStatusUpdate {
  taskId: string
  taskName: string
  commitCount: number
  reason: string
}

export interface DailyReview {
  date: string
  range: { since: string; until: string; label: string }
  summary: {
    totalCommits: number
    businessLineCount: number
    tasksAdvancedCount: number
    orphanCommitCount: number
  }
  /** 今天有 commit 关联的任务，按 commit 数倒序 */
  advancedTasks: AdvancedTask[]
  /** 按业务线分组的 commit + 建议工时 */
  byBusinessLine: BusinessLineGroup[]
  /** 有 commit 关联但任务状态仍是"未开始"——提醒去禅道更新 */
  needsStatusUpdate: NeedsStatusUpdate[]
  /**
   * 没匹配到任务的 commit。也是真实工作量，进日报。
   * effort 是这组 commit 的代码量分（≈ 影响行数），suggestedHours 按全天工时
   * 比例分配，让用户填写禅道的"杂项工时"或在日报里口头补一段。
   */
  orphanCommits: Array<{
    businessLine: string
    commits: CommitLink[]
    effort: number
    suggestedHours?: number
  }>
  /** 用于按业务线分配工时的总工时基数（若未传 hoursPerWorkDay，则为 0） */
  totalHoursForEstimate: number
  /** 纯文本日报草稿（无 Markdown 符号，可直接复制粘贴） */
  plainText: string
}

function todayStr(now = new Date()): string {
  const y = now.getFullYear()
  const m = String(now.getMonth() + 1).padStart(2, '0')
  const d = String(now.getDate()).padStart(2, '0')
  return `${y}-${m}-${d}`
}

/** 四舍五入到 0.5 */
function roundHalf(x: number): number {
  return Math.round(x * 2) / 2
}

/**
 * 把业务线的 N 小时按 0.5h 量化分配到任务上。
 *
 * 规则：
 *   - 业务线工时 H → 槽位数 slots = round(H × 2)
 *   - 任务按 commit 数倒序优先分配
 *   - 若 slots ≥ 任务数：每个任务至少 0.5h，余数从前面任务往后补
 *   - 若 slots < 任务数：仅前 slots 个任务各 0.5h，其余 0（不显示工时建议）
 *
 * 这样保证最小粒度 0.5h，避免出现 0.2h / 0.3h 等无效精度。
 */
function allocateHoursBySlots(businessHours: number, taskCount: number): number[] {
  const slots = Math.round(businessHours * 2)
  if (slots <= 0 || taskCount <= 0) return new Array(taskCount).fill(0)
  if (slots >= taskCount) {
    const base = Math.floor(slots / taskCount)
    const extra = slots - base * taskCount
    return Array.from({ length: taskCount }, (_, i) => (base + (i < extra ? 1 : 0)) * 0.5)
  }
  return Array.from({ length: taskCount }, (_, i) => (i < slots ? 0.5 : 0))
}

/**
 * 合成日报。
 *
 * 调用方负责传入 commit-link 结果和任务列表，本服务做组合 + 生成纯文本草稿。
 */
export function buildDailyReview(
  linkResult: CommitLinkResult,
  tasks: ReviewTaskInfo[],
  options: { date?: string; hoursPerWorkDay?: number } = {},
): DailyReview {
  const date = options.date ?? todayStr()
  const hoursPerWorkDay = options.hoursPerWorkDay ?? 0

  const taskById = new Map<string, ReviewTaskInfo>()
  for (const t of tasks) taskById.set(String(t.id), t)

  // ---- 推进的任务 ----
  // 一个任务的 commits 可能横跨多个业务线（任务名同时命中多条业务线关键词），
  // 这里把"主业务线"定为该任务下 commit 数最多的那条；effort 只算主业务线内的
  // commit，避免跨业务线累加把这种任务挤到 effort 排行榜前列。
  const advancedTasks: AdvancedTask[] = linkResult.tasks
    .filter(t => t.commits.length > 0)
    .map(t => {
      const taskInfo = taskById.get(t.taskId)
      const blCount = new Map<string, number>()
      for (const c of t.commits) blCount.set(c.businessLine, (blCount.get(c.businessLine) ?? 0) + 1)
      const businessLine =
        Array.from(blCount.entries()).sort((a, b) => b[1] - a[1])[0]?.[0] ?? ''
      const effort = t.commits
        .filter(c => c.businessLine === businessLine)
        .reduce((s, c) => s + (c.effort ?? 1), 0)
      return {
        taskId: t.taskId,
        taskName: t.taskName,
        status: taskInfo?.status ?? 'unknown',
        commitCount: t.commits.length,
        commits: t.commits,
        businessLine,
        effort,
      }
    })
    .sort((a, b) => b.effort - a.effort)

  // ---- 按业务线分组 ----
  const byLineMap = new Map<string, BusinessLineGroup>()
  const collectCommit = (bl: string, c: CommitLink) => {
    if (!byLineMap.has(bl)) byLineMap.set(bl, { businessLine: bl, commits: [], tasks: [], effort: 0 })
    const g = byLineMap.get(bl)!
    if (!g.commits.find(x => x.sha === c.sha && x.repoPath === c.repoPath)) {
      g.commits.push(c)
      g.effort += c.effort ?? 1
    }
  }
  const collectTask = (bl: string, taskId: string, taskName: string) => {
    if (!byLineMap.has(bl)) byLineMap.set(bl, { businessLine: bl, commits: [], tasks: [], effort: 0 })
    const g = byLineMap.get(bl)!
    if (!g.tasks.find(x => x.taskId === taskId)) {
      g.tasks.push({ taskId, taskName })
    }
  }
  for (const t of linkResult.tasks) {
    for (const c of t.commits) {
      collectCommit(c.businessLine, c)
      collectTask(c.businessLine, t.taskId, t.taskName)
    }
  }
  for (const o of linkResult.orphanCommits) {
    for (const c of o.commits) collectCommit(o.businessLine, c)
  }
  const byBusinessLine = Array.from(byLineMap.values())
    .map(g => ({
      ...g,
      commits: g.commits.sort((a, b) => b.authoredDate.localeCompare(a.authoredDate)),
    }))
    .sort((a, b) => b.effort - a.effort)

  // ---- 按业务线分配建议工时（按 effort 比例，不再按 commit 数）----
  // 所有业务线（含纯孤儿业务线）都按 effort 比例分工时 —— 用户的真实工作量。
  // 历史上这里曾排除"没任务的业务线"防 my-mcp-servers 一类工具仓占工时，
  // 现在这类仓应该通过 ~/.jarvis/excluded-business-lines.json 提前过滤，
  // 走到这里的就是用户认可的真实工作。日报要全部呈现 / 全部分工时，让
  // 用户在禅道用「杂项任务」之类的方式补登也好，至少别让代码量凭空消失。
  const totalEffort = byBusinessLine.reduce((s, g) => s + g.effort, 0)
  if (hoursPerWorkDay > 0 && totalEffort > 0) {
    for (const g of byBusinessLine) {
      const raw = (g.effort / totalEffort) * hoursPerWorkDay
      g.suggestedHours = roundHalf(raw)
    }
    // 业务线工时按 0.5h 粒度分到任务，优先 effort 高的（孤儿业务线没任务，
    // 这一步直接跳过，整条业务线的 suggestedHours 留给 orphan 段呈现）
    for (const line of byBusinessLine.filter(g => g.tasks.length > 0)) {
      if (!line.suggestedHours) continue
      const lineTasks = advancedTasks
        .filter(t => t.businessLine === line.businessLine)
        .sort((a, b) => b.effort - a.effort)
      const allocations = allocateHoursBySlots(line.suggestedHours, lineTasks.length)
      lineTasks.forEach((t, i) => {
        t.suggestedHours = allocations[i]
      })
    }
  }

  // ---- 应该更新状态的任务（有 commit 但状态还是 wait） ----
  const needsStatusUpdate: NeedsStatusUpdate[] = advancedTasks
    .filter(t => t.status === 'wait')
    .map(t => ({
      taskId: t.taskId,
      taskName: t.taskName,
      commitCount: t.commitCount,
      reason: `本地已有 ${t.commitCount} 个 commit，但禅道状态仍是"未开始"`,
    }))

  // ---- 概况 ----
  const summary = {
    totalCommits: linkResult.totalCommits,
    businessLineCount: byBusinessLine.length,
    tasksAdvancedCount: advancedTasks.length,
    orphanCommitCount: linkResult.orphanCommits.reduce((s, o) => s + o.commits.length, 0),
  }

  // ---- 孤儿 commit 也估个工时 ----
  // 没匹配到禅道任务的提交也是真实工作。按这组 commit 的 effort 占全天总
  // effort 的比例切一份工时出来，让用户能在日报里补一段或拿去禅道找杂项
  // 任务填工时。
  const orphanCommitGroups = linkResult.orphanCommits.map(o => {
    const effort = o.commits.reduce((s, c) => s + (c.effort ?? 1), 0)
    const suggestedHours = hoursPerWorkDay > 0 && totalEffort > 0
      ? roundHalf((effort / totalEffort) * hoursPerWorkDay)
      : undefined
    return { businessLine: o.businessLine, commits: o.commits, effort, suggestedHours }
  })

  // ---- 纯文本草稿 ----
  const plainText = renderPlainText(date, summary, advancedTasks, byBusinessLine, needsStatusUpdate, orphanCommitGroups, hoursPerWorkDay)

  return {
    date,
    range: linkResult.range,
    summary,
    advancedTasks,
    byBusinessLine,
    needsStatusUpdate,
    orphanCommits: orphanCommitGroups,
    totalHoursForEstimate: hoursPerWorkDay,
    plainText,
  }
}

function renderPlainText(
  date: string,
  summary: DailyReview['summary'],
  advancedTasks: AdvancedTask[],
  byLine: BusinessLineGroup[],
  needsUpdate: NeedsStatusUpdate[],
  orphanGroups: DailyReview['orphanCommits'],
  hoursPerWorkDay: number,
): string {
  const lines: string[] = []
  lines.push(`工作日报 ${date}`)
  lines.push('')

  if (summary.totalCommits === 0) {
    lines.push('今天没有本地提交。如有未推送或外部协作，请手动补充。')
    return lines.join('\n')
  }

  lines.push(`今天共提交 ${summary.totalCommits} 个 commit，覆盖 ${summary.businessLineCount} 个业务线，推进 ${summary.tasksAdvancedCount} 个任务。`)
  lines.push('')

  // 完成内容（以业务线为主线，避免同 commit 在多个任务下重复）
  lines.push('【完成内容】')
  lines.push('')
  for (const g of byLine) {
    if (g.commits.length === 0) continue
    // 按清理后的标题去重（剥掉 emoji/feat: 前缀后比较），避免 rebase/cherry-pick
    // 产生的同标题不同 sha 干扰阅读，也避免「✨ feat: xx」和「feat: xx」被当成两条
    const uniqueByTitle: Array<{ commit: CommitLink; cleaned: string }> = []
    const seen = new Set<string>()
    for (const c of g.commits) {
      const cleaned = cleanCommitTitle(c.title)
      if (!cleaned || seen.has(cleaned)) continue
      seen.add(cleaned)
      uniqueByTitle.push({ commit: c, cleaned })
    }
    const dupCount = g.commits.length - uniqueByTitle.length
    const countLabel = dupCount > 0
      ? `${uniqueByTitle.length} 个主题 / 共 ${g.commits.length} 次提交`
      : `${g.commits.length} 个 commit`

    lines.push(`${g.businessLine}（${countLabel}）`)
    for (const { commit: c, cleaned } of uniqueByTitle) {
      lines.push(`  · ${cleaned}  (${c.repoName} · ${c.shortSha})`)
    }
    if (g.tasks.length > 0) {
      lines.push('  推进任务：')
      for (const t of g.tasks) {
        const adv = advancedTasks.find(a => a.taskId === t.taskId)
        const statusMark = adv?.status === 'wait' ? '（未开始）' : ''
        lines.push(`    - #${t.taskId} ${t.taskName}${statusMark}`)
      }
    }
    lines.push('')
  }

  // 建议工时（仅当 hoursPerWorkDay > 0 时输出）
  const linesWithHours = byLine.filter(g => g.suggestedHours !== undefined && g.suggestedHours > 0)
  if (linesWithHours.length > 0 && hoursPerWorkDay > 0) {
    lines.push(`【建议工时分配】（最小粒度 0.5h，仅供禅道填报参考）`)
    for (const g of linesWithHours) {
      const lineTasks = advancedTasks.filter(t => t.businessLine === g.businessLine)
      const tasksWithHours = lineTasks.filter(t => (t.suggestedHours ?? 0) > 0)
      lines.push(`  · ${g.businessLine}：${g.suggestedHours}h（${tasksWithHours.length}/${lineTasks.length} 个主要任务）`)
      for (const t of tasksWithHours) {
        lines.push(`    - #${t.taskId} ${t.taskName}：${t.suggestedHours}h`)
      }
    }
    lines.push('  注：commit 数少的次要任务未分配工时，主要任务工时合计为日总工时。')
    lines.push('')
  }

  // 需要状态更新
  if (needsUpdate.length > 0) {
    lines.push('【需要在禅道更新状态的任务】')
    for (const t of needsUpdate) {
      lines.push(`  · #${t.taskId} ${t.taskName}：${t.reason}`)
    }
    lines.push('')
  }

  // 未关联任务的提交（孤儿）—— 用户实际写了代码但没找到对应禅道任务。
  // 列出来方便用户在日报里口头交代 / 在禅道用杂项任务补登。
  const nonEmptyOrphans = orphanGroups.filter(o => o.commits.length > 0)
  if (nonEmptyOrphans.length > 0) {
    lines.push('【未关联禅道任务的提交】（建议补任务号或在日报中说明）')
    for (const g of nonEmptyOrphans) {
      const seen = new Set<string>()
      const unique: Array<{ commit: CommitLink; cleaned: string }> = []
      for (const c of g.commits) {
        const cleaned = cleanCommitTitle(c.title)
        if (!cleaned || seen.has(cleaned)) continue
        seen.add(cleaned)
        unique.push({ commit: c, cleaned })
      }
      const hoursLabel = g.suggestedHours ? `，建议 ~${g.suggestedHours}h` : ''
      lines.push(`  · ${g.businessLine}（${unique.length} 个主题${hoursLabel}）`)
      for (const { commit: c, cleaned } of unique) {
        lines.push(`    - ${cleaned}  (${c.repoName} · ${c.shortSha})`)
      }
    }
    lines.push('')
  }

  return lines.join('\n').trimEnd()
}
