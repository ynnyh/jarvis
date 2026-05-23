import { z } from 'zod'
import { TaskService } from '../services/task-service.js'
import { toolRegistry } from '../core/tool-registry.js'
import type { Tool } from '../core/tool-registry.js'
import type { Task, RiskAnalysis } from '../shared/types.js'
import { getZentaoCredentials } from '../config/settings.js'

const inputSchema = z.object({})

async function execute(_input: z.infer<typeof inputSchema>): Promise<RiskAnalysis> {
  const { ZenTaoProvider } = await import('../providers/zentao-provider.js')
  const { baseUrl, account, password } = getZentaoCredentials()
  const provider = new ZenTaoProvider({ baseUrl, username: account, password })
  const service = new TaskService(provider)

  const tasks = await service.getMyTasks()
  const now = new Date()
  const threeDaysLater = new Date(now.getTime() + 3 * 24 * 60 * 60 * 1000)

  const overdueTasks = tasks.filter(t => {
    if (!t.deadline || t.status === 'done' || t.status === 'closed') return false
    const deadline = new Date(t.deadline)
    return deadline < now
  })

  const nearDeadlineTasks = tasks.filter(t => {
    if (!t.deadline || t.status === 'done' || t.status === 'closed') return false
    const deadline = new Date(t.deadline)
    return deadline >= now && deadline <= threeDaysLater
  })

  const highPriorityTasks = tasks.filter(t => {
    return (t.priority === 'urgent' || t.priority === 'high') &&
           t.status !== 'done' && t.status !== 'closed'
  })

  const dependencyRisks = analyzeDependencies(tasks)

  const summary = buildSummary(overdueTasks, nearDeadlineTasks, highPriorityTasks, dependencyRisks)

  return {
    overdueTasks: [...overdueTasks, ...nearDeadlineTasks],
    highPriorityTasks,
    dependencyRisks,
    summary,
  }
}

function analyzeDependencies(tasks: Task[]) {
  const taskMap = new Map(tasks.map(t => [t.id, t]))
  const risks = []

  for (const task of tasks) {
    if (!task.dependencies || task.dependencies.length === 0) continue
    if (task.status === 'done' || task.status === 'closed') continue

    const missingDeps = task.dependencies.filter(depId => {
      const dep = taskMap.get(depId)
      return !dep || (dep.status !== 'done' && dep.status !== 'closed')
    })

    if (missingDeps.length > 0) {
      risks.push({
        taskId: task.id,
        taskTitle: task.title,
        missingDependencies: missingDeps,
        reason: `依赖任务 ${missingDeps.join(', ')} 尚未完成`,
      })
    }
  }

  return risks
}

function buildSummary(
  overdue: Task[],
  nearDeadline: Task[],
  highPriority: Task[],
  depRisks: RiskAnalysis['dependencyRisks']
): string {
  const lines: string[] = []

  if (overdue.length > 0) {
    lines.push(`发现 ${overdue.length} 个已延期任务，需要立即处理。`)
  }
  if (nearDeadline.length > 0) {
    lines.push(`发现 ${nearDeadline.length} 个即将到期任务（3天内），请密切关注。`)
  }
  if (highPriority.length > 0) {
    lines.push(`有 ${highPriority.length} 个高优先级任务待处理。`)
  }
  if (depRisks.length > 0) {
    lines.push(`发现 ${depRisks.length} 个任务存在依赖风险。`)
  }

  if (lines.length === 0) {
    lines.push('当前任务状态良好，未发现明显风险。')
  }

  return lines.join('\n')
}

export const analyzeRiskTool: Tool = {
  metadata: {
    name: 'analyze_risk',
    description: 'AI 分析所有任务的风险，包括延期风险、高优先级任务和依赖风险',
    category: 'analysis',
    version: '1.0.0',
    inputSchema,
    examples: [
      {
        input: {},
        description: '分析当前所有任务的风险',
      },
    ],
  },
  execute: execute as any,
}

toolRegistry.register(analyzeRiskTool)

export async function analyzeRisk(service: TaskService): Promise<RiskAnalysis> {
  return execute({}) as Promise<RiskAnalysis>
}
