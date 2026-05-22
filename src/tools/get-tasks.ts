import { z } from 'zod'
import { TaskService } from '../services/task-service.js'
import { toolRegistry } from '../core/tool-registry.js'
import type { Tool } from '../core/tool-registry.js'

const inputSchema = z.object({
  status: z.array(z.enum(['wait', 'doing', 'done', 'closed', 'cancel'])).optional(),
  assignee: z.string().optional(),
})

async function execute(input: z.infer<typeof inputSchema>) {
  const { ZenTaoProvider } = await import('../providers/zentao-provider.js')
  const baseUrl = process.env.ZENTAO_BASE_URL || process.env.ZENTAO_URL || ''
  const username = process.env.ZENTAO_ACCOUNT || process.env.ZENTAO_USER || ''
  const password = process.env.ZENTAO_PASSWORD || process.env.ZENTAO_PASS || ''
  const provider = new ZenTaoProvider({ baseUrl, username, password })
  const service = new TaskService(provider)
  return service.getMyTasks()
}

export const getTasksTool: Tool = {
  metadata: {
    name: 'get_tasks',
    description: '获取全部任务列表，支持按状态和负责人筛选',
    category: 'task',
    version: '1.0.0',
    inputSchema,
    examples: [
      {
        input: {},
        description: '获取所有任务',
      },
      {
        input: { status: ['doing', 'wait'] },
        description: '获取进行中和未开始的任务',
      },
    ],
  },
  execute: execute as any,
}

toolRegistry.register(getTasksTool)

// 兼容旧接口
export async function getTasks(service: TaskService, filter?: { status?: Array<'wait' | 'doing' | 'done' | 'closed' | 'cancel'>; assignee?: string }) {
  return service.getMyTasks()
}
