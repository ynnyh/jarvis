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
  /** 建议工时（小时，四舍五入到 0.5）：业务线工时 / 该业务线任务数 */
  suggestedHours?: number
}

export interface BusinessLineGroup {
  businessLine: string
  commits: CommitLink[]
  /** 这条业务线下涉及的任务（去重） */
  tasks: Array<{ taskId: string; taskName: string }>
  /** 按 commit 比例估算的建议工时（小时，四舍五入到 0.5） */
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
  /** 没匹配到任务的 commit（可能是工具仓库/试验代码） */
  orphanCommits: Array<{ businessLine: string; commits: CommitLink[] }>
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
  const advancedTasks: AdvancedTask[] = linkResult.tasks
    .filter(t => t.commits.length > 0)
    .map(t => {
      const taskInfo = taskById.get(t.taskId)
      const businessLine = t.commits[0]?.businessLine ?? ''
      return {
        taskId: t.taskId,
        taskName: t.taskName,
        status: taskInfo?.status ?? 'unknown',
        commitCount: t.commits.length,
        commits: t.commits,
        businessLine,
      }
    })
    .sort((a, b) => b.commitCount - a.commitCount)

  // ---- 按业务线分组 ----
  const byLineMap = new Map<string, BusinessLineGroup>()
  const collectCommit = (bl: string, c: CommitLink) => {
    if (!byLineMap.has(bl)) byLineMap.set(bl, { businessLine: bl, commits: [], tasks: [] })
    const g = byLineMap.get(bl)!
    if (!g.commits.find(x => x.sha === c.sha && x.repoPath === c.repoPath)) {
      g.commits.push(c)
    }
  }
  const collectTask = (bl: string, taskId: string, taskName: string) => {
    if (!byLineMap.has(bl)) byLineMap.set(bl, { businessLine: bl, commits: [], tasks: [] })
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
    .sort((a, b) => b.commits.length - a.commits.length)

  // ---- 按业务线分配建议工时 ----
  // 排除孤儿业务线（如 my-mcp-servers），它不该占工时
  const linesWithTasks = byBusinessLine.filter(g => g.tasks.length > 0)
  const totalCommitsForEstimate = linesWithTasks.reduce((s, g) => s + g.commits.length, 0)
  if (hoursPerWorkDay > 0 && totalCommitsForEstimate > 0) {
    for (const g of linesWithTasks) {
      const raw = (g.commits.length / totalCommitsForEstimate) * hoursPerWorkDay
      g.suggestedHours = roundHalf(raw)
    }
    // 按业务线把工时按 0.5h 粒度分配给任务（优先 commit 数多的）
    for (const line of linesWithTasks) {
      if (!line.suggestedHours) continue
      // 拿该业务线下的任务（按 commit 数倒序）
      const lineTasks = advancedTasks
        .filter(t => t.businessLine === line.businessLine)
        .sort((a, b) => b.commitCount - a.commitCount)
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

  // ---- 纯文本草稿 ----
  const plainText = renderPlainText(date, summary, advancedTasks, byBusinessLine, needsStatusUpdate, hoursPerWorkDay)

  return {
    date,
    range: linkResult.range,
    summary,
    advancedTasks,
    byBusinessLine,
    needsStatusUpdate,
    orphanCommits: linkResult.orphanCommits,
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

  return lines.join('\n').trimEnd()
}
