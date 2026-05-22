import { z } from 'zod'
import { TaskService } from '../services/task-service.js'
import { toolRegistry } from '../core/tool-registry.js'
import type { Tool } from '../core/tool-registry.js'

const inputSchema = z.object({})

async function execute(_input: z.infer<typeof inputSchema>) {
  const { ZenTaoProvider } = await import('../providers/zentao-provider.js')
  const baseUrl = process.env.ZENTAO_BASE_URL || process.env.ZENTAO_URL || ''
  const username = process.env.ZENTAO_ACCOUNT || process.env.ZENTAO_USER || ''
  const password = process.env.ZENTAO_PASSWORD || process.env.ZENTAO_PASS || ''
  const provider = new ZenTaoProvider({ baseUrl, username, password })
  const service = new TaskService(provider)
  return service.getTodayTasks()
}

export const getTodayTasksTool: Tool = {
  metadata: {
    name: 'get_today_tasks',
    description: '获取今天截止的任务列表',
    category: 'task',
    version: '1.0.0',
    inputSchema,
    examples: [
      {
        input: {},
        description: '获取今天截止的所有任务',
      },
    ],
  },
  execute: execute as any,
}

toolRegistry.register(getTodayTasksTool)

export async function getTodayTasks(service: TaskService) {
  return service.getTodayTasks()
}
