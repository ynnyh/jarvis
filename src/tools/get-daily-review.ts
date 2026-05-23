import { z } from 'zod'
import { toolRegistry } from '../core/tool-registry.js'
import type { Tool } from '../core/tool-registry.js'
import { linkTasksWithCommits } from '../services/commit-link-service.js'
import { buildDailyReview } from '../services/daily-review-service.js'
import { TaskService } from '../services/task-service.js'
import { getZentaoCredentials, getRepoRoots } from '../config/settings.js'

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
})

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
    },
  )

  // 3. 合成日报
  return buildDailyReview(linkResult, taskInputs, {
    date: input.date,
    hoursPerWorkDay: input.hoursPerWorkDay,
  })
}

export const getDailyReviewTool: Tool = {
  metadata: {
    name: 'get_daily_review',
    description: '生成今日工作复盘：基于本地 commit 与禅道任务关联，输出推进任务、业务线分布、需要更新状态的任务，并附带纯文本日报草稿（可选按 commit 数比例反推建议工时分配）。',
    category: 'task',
    version: '1.0.0',
    inputSchema,
    examples: [
      { input: {}, description: '生成今天的复盘（无工时建议）' },
      { input: { hoursPerWorkDay: 8 }, description: '生成今天的复盘 + 按 8h 反推工时分配' },
      { input: { range: 'thisWeek' }, description: '生成本周复盘' },
    ],
  },
  execute: execute as any,
}

toolRegistry.register(getDailyReviewTool)
