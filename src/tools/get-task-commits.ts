import { z } from 'zod'
import { toolRegistry } from '../core/tool-registry.js'
import type { Tool } from '../core/tool-registry.js'
import { linkTasksWithCommits, type LinkCommitsOptions } from '../services/commit-link-service.js'
import { TaskService } from '../services/task-service.js'
import { getZentaoCredentials, getRepoRoots } from '../config/settings.js'

const rangeEnum = z.enum([
  'today', 'yesterday', 'thisWeek', 'lastWeek', 'last7days', 'last30days', 'thisMonth', 'all',
])

const inputSchema = z.object({
  range: rangeEnum.optional(),
  since: z.string().optional(),
  until: z.string().optional(),
  rootDir: z.union([z.string(), z.array(z.string())]).optional(),
  includeBody: z.boolean().optional(),
  /** 只关联这些任务 ID（不传则关联当前用户的全部任务） */
  taskIds: z.array(z.union([z.string(), z.number()])).optional(),
})

async function execute(input: z.infer<typeof inputSchema>) {
  // 1. 拉任务（默认：当前用户的全部任务）
  const { ZenTaoProvider } = await import('../providers/zentao-provider.js')
  const { baseUrl, account, password } = getZentaoCredentials()
  const provider = new ZenTaoProvider({ baseUrl, username: account, password })
  const service = new TaskService(provider)

  const allTasks = await service.getMyTasks()

  const filtered = input.taskIds && input.taskIds.length > 0
    ? allTasks.filter(t => input.taskIds!.some(id => String(id) === String((t as any).id)))
    : allTasks

  const taskInputs = filtered.map((t: any) => ({
    id: t.id,
    name: t.name ?? t.title ?? '',
  }))

  // 2. 关联（rootDir 入参优先；未传则用 settings.repoRoots）
  const options: LinkCommitsOptions = {
    range: input.range,
    since: input.since,
    until: input.until,
    rootDir: input.rootDir ?? getRepoRoots(),
    includeBody: input.includeBody ?? true,
  }
  return linkTasksWithCommits(taskInputs, options)
}

export const getTaskCommitsTool: Tool = {
  metadata: {
    name: 'get_task_commits',
    description: '关联禅道任务与本地 git 提交。通过 MCP 调用 tencentcode-mcp 拉取近期 commit，按仓库名关键词与任务名匹配做软关联，并识别 commit message 中的 #任务号 做精确关联。',
    category: 'task',
    version: '1.0.0',
    inputSchema,
    examples: [
      { input: {}, description: '关联今天我的所有任务与今天的 commit' },
      { input: { range: 'thisWeek' }, description: '关联本周' },
      { input: { range: 'last7days', taskIds: [10238] }, description: '只看某一个任务近 7 天的相关 commit' },
    ],
  },
  execute: execute as any,
}

toolRegistry.register(getTaskCommitsTool)
