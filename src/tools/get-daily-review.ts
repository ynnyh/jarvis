import { z } from 'zod'
import { toolRegistry } from '../core/tool-registry.js'
import type { Tool } from '../core/tool-registry.js'
import { linkTasksWithCommits } from '../services/commit-link-service.js'
import { buildDailyReview } from '../services/daily-review-service.js'
import { TaskService } from '../services/task-service.js'
import { getZentaoCredentials, getRepoRoots } from '../config/settings.js'
import { getLlmClient } from '../llm/client.js'

const inputSchema = z.object({
  /** 默认 today；做"周复盘"可传 thisWeek */
  range: z.enum(['today', 'yesterday', 'thisWeek', 'last7days']).optional(),
  /** 自定义 since/until 时间戳 */
  since: z.string().optional(),
  until: z.string().optional(),
  /** 标在日报标题上的日期（默认今天 YYYY-MM-DD） */
  date: z.string().optional(),
  /** 用户每个工作日的总工时（小时）。传了才会输出"建议工时分配"段落 */
  hoursPerWorkDay: z.number().optional(),
  /** 用 LLM 改写 plainText 段落（heuristic 仍生成作 fallback，调用失败自动回退） */
  useLlm: z.boolean().optional(),
})

/**
 * 用 LLM 改写日报段落。**heuristic plainText 仍保留作 fallback**——
 * LLM 调用失败 / 没配 apiKey 时自动回退，不阻塞用户拿到日报。
 *
 * Prompt 设计要点：
 * - 喂结构化数据（不喂 heuristic plainText），让 LLM 自己组织叙事
 * - 强调"去技术化"——这是用户的明确偏好（见 memory feedback_daily_report_style）
 * - 不要让 LLM 改 commit 标题（已经 clean 过），只让它在外层组织语言
 * - 温度 0.4——比默认 0.3 略放开，但不要发散到瞎编
 */
async function rewriteWithLlm(review: any): Promise<string> {
  const summaryPayload = {
    date: review.date,
    totalCommits: review.summary.totalCommits,
    advancedTasks: review.advancedTasks.map((t: any) => ({
      id: t.taskId,
      name: t.taskName,
      status: t.status,
      businessLine: t.businessLine,
      commitCount: t.commitCount,
      suggestedHours: t.suggestedHours,
    })),
    byBusinessLine: review.byBusinessLine.map((g: any) => ({
      businessLine: g.businessLine,
      commitCount: g.commits.length,
      taskCount: g.tasks.length,
      suggestedHours: g.suggestedHours,
      // 只给 LLM 看 cleaned title，不给 sha/repo 等技术字段
      commitTitles: Array.from(new Set(g.commits.map((c: any) => c.title))).slice(0, 20),
    })),
    needsStatusUpdate: review.needsStatusUpdate.map((n: any) => ({
      id: n.taskId, name: n.taskName, commitCount: n.commitCount,
    })),
    orphanCommitCount: review.summary.orphanCommitCount,
    totalHoursForEstimate: review.totalHoursForEstimate,
  }

  const client = getLlmClient()
  const result = await client.chat({
    temperature: 0.4,
    maxTokens: 1500,
    messages: [
      {
        role: 'system',
        content:
          '你是一个简洁的日报助手。基于结构化的当日工作数据生成自然语言日报。\n' +
          '强约束：\n' +
          '1. 完全去技术化——不出现 commit/sha/repo/PR/branch 等词，commit 标题原样使用（不要加技术修饰）\n' +
          '2. 不要凭空发挥，所有内容必须能从输入数据里找到依据\n' +
          '3. 用项目维度（业务线）+ 任务推进 + 需要补登/更新状态的事项 组织文章\n' +
          '4. 段落清晰，避免大段流水账。提供工时建议时按业务线总览，不必每个任务展开\n' +
          '5. 输出纯文本，不要 Markdown 符号（#、*、- 都不用）',
      },
      {
        role: 'user',
        content:
          `请基于以下结构化数据，写一份 ${summaryPayload.date} 的工作日报（中文，纯文本）：\n\n` +
          '```json\n' + JSON.stringify(summaryPayload, null, 2) + '\n```',
      },
    ],
  })
  return result.text.trim()
}

async function execute(input: z.infer<typeof inputSchema>) {
  // 1. 拉禅道任务
  const { ZenTaoProvider } = await import('../providers/zentao-provider.js')
  const { baseUrl, account, password } = getZentaoCredentials()
  const provider = new ZenTaoProvider({ baseUrl, username: account, password })
  const service = new TaskService(provider)
  const allTasks = await service.getMyTasks()

  const taskInputs = allTasks.map((t: any) => ({
    id: t.id,
    name: t.name ?? t.title ?? '',
    status: t.status ?? 'wait',
  }))

  // 2. 拉 commit + 关联
  const linkResult = await linkTasksWithCommits(
    taskInputs.map(t => ({ id: t.id, name: t.name })),
    {
      range: input.range ?? 'today',
      since: input.since,
      until: input.until,
      rootDir: getRepoRoots(),
      includeBody: true,
      // useLlm 同时驱动两件事：commit↔task soft 匹配评分 + 日报段落改写
      useLlm: input.useLlm,
    },
  )

  // 3. 合成日报（heuristic 结构 + 文本）
  const review = buildDailyReview(linkResult, taskInputs, {
    date: input.date,
    hoursPerWorkDay: input.hoursPerWorkDay,
  })

  // 4. 可选 LLM 改写。失败回退到 heuristic，附带 warning 字段供前端展示
  if (input.useLlm) {
    try {
      const llmText = await rewriteWithLlm(review)
      return { ...review, plainText: llmText, plainTextHeuristic: review.plainText, llmUsed: true }
    } catch (e: any) {
      const message = e instanceof Error ? e.message : String(e)
      return { ...review, llmUsed: false, llmError: message }
    }
  }

  return review
}

export const getDailyReviewTool: Tool = {
  metadata: {
    name: 'get_daily_review',
    description: '生成今日工作复盘：基于本地 commit 与禅道任务关联，输出推进任务、业务线分布、需要更新状态的任务，并附带纯文本日报草稿（可选按 commit 数比例反推建议工时分配；可选 useLlm 调 LLM 改写段落）。',
    category: 'task',
    version: '1.1.0',
    inputSchema,
    examples: [
      { input: {}, description: '生成今天的复盘（无工时建议）' },
      { input: { hoursPerWorkDay: 8 }, description: '生成今天的复盘 + 按 8h 反推工时分配' },
      { input: { range: 'thisWeek' }, description: '生成本周复盘' },
      { input: { useLlm: true, hoursPerWorkDay: 8 }, description: 'LLM 改写日报正文' },
    ],
  },
  execute: execute as any,
}

toolRegistry.register(getDailyReviewTool)
