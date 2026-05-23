import { z } from 'zod'
import { TaskService } from '../services/task-service.js'
import { toolRegistry } from '../core/tool-registry.js'
import type { Tool } from '../core/tool-registry.js'
import { getZentaoCredentials } from '../config/settings.js'

const inputSchema = z.object({
  id: z.string().describe('任务 ID'),
})

async function execute(input: z.infer<typeof inputSchema>) {
  const { ZenTaoProvider } = await import('../providers/zentao-provider.js')
  const { baseUrl, account, password } = getZentaoCredentials()
  const provider = new ZenTaoProvider({ baseUrl, username: account, password })
  const service = new TaskService(provider)
  return service.getTaskDetail(input.id)
}

export const getTaskDetailTool: Tool = {
  metadata: {
    name: 'get_task_detail',
    description: '获取单个任务的详细信息',
    category: 'task',
    version: '1.0.0',
    inputSchema,
    examples: [
      {
        input: { id: '101' },
        description: '获取任务 101 的详情',
      },
    ],
  },
  execute: execute as any,
}

toolRegistry.register(getTaskDetailTool)

export async function getTaskDetail(service: TaskService, id: string) {
  return service.getTaskDetail(id)
}
