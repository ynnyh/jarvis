import { z } from 'zod'
import { TaskService } from '../services/task-service.js'
import { toolRegistry } from '../core/tool-registry.js'
import type { Tool } from '../core/tool-registry.js'
import type { Task, RiskAnalysis } from '../shared/types.js'
import { getZentaoCredentials } from '../config/settings.js'
import { getLlmClient } from '../llm/client.js'

const inputSchema = z.object({
  /** 用 LLM 改写 summary 段落。失败回退 heuristic（保留计数行） */
  useLlm: z.boolean().optional(),
})

async function execute(input: z.infer<typeof inputSchema>): Promise<RiskAnalysis & { llmUsed?: boolean; llmError?: string; summaryHeuristic?: string }> {
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

  const heuristicSummary = buildSummary(overdueTasks, nearDeadlineTasks, highPriorityTasks, dependencyRisks)

  const base: RiskAnalysis = {
    overdueTasks: [...overdueTasks, ...nearDeadlineTasks],
    highPriorityTasks,
    dependencyRisks,
    summary: heuristicSummary,
  }

  if (input.useLlm) {
    try {
      const llmSummary = await summarizeWithLlm(overdueTasks, nearDeadlineTasks, highPriorityTasks, dependencyRisks)
      return { ...base, summary: llmSummary, summaryHeuristic: heuristicSummary, llmUsed: true }
    } catch (e: any) {
      return { ...base, llmUsed: false, llmError: e instanceof Error ? e.message : String(e) }
    }
  }

  return base
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

/**
 * 用 LLM 把"计数"升级为"建议"。
 *
 * Prompt 策略：给 LLM 看具体的 overdue / nearDeadline / highPriority 任务标题
 * 和截止日期，让它给出"今天应该优先处理什么、为什么"。不要让它编没在数据里
 * 的事项。温度 0.3——风险提示要稳，不要发散。
 */
async function summarizeWithLlm(
  overdue: Task[],
  nearDeadline: Task[],
  highPriority: Task[],
  depRisks: RiskAnalysis['dependencyRisks'],
): Promise<string> {
  const briefTask = (t: Task) => ({
    id: t.id, title: t.title, status: t.status,
    priority: t.priority, deadline: t.deadline || null,
  })

  const payload = {
    overdue: overdue.map(briefTask),
    nearDeadline: nearDeadline.map(briefTask),
    highPriority: highPriority.map(briefTask),
    dependencyRisks: depRisks,
    today: new Date().toISOString().slice(0, 10),
  }

  const client = getLlmClient()
  const res = await client.chat({
    temperature: 0.3,
    maxTokens: 800,
    messages: [
      {
        role: 'system',
        content:
          '你是一个简短直接的任务风险提示助手。基于结构化的风险数据，告诉用户今天应该优先关注什么。\n' +
          '约束：\n' +
          '1. 不堆砌"发现 N 个..."这种计数语，要给出具体建议（哪些任务先做、为什么）\n' +
          '2. 只能基于输入数据，不要编没有的任务名或事项\n' +
          '3. 中文，纯文本，3~6 句话以内\n' +
          '4. 如果数据里没有风险，直接说"今天没有明显风险"',
      },
      {
        role: 'user',
        content: '```json\n' + JSON.stringify(payload, null, 2) + '\n```',
      },
    ],
  })
  return res.text.trim()
}

export const analyzeRiskTool: Tool = {
  metadata: {
    name: 'analyze_risk',
    description: '分析所有任务的风险（延期、高优先级、依赖）。useLlm=true 时用 LLM 把计数提示升级为具体建议。',
    category: 'analysis',
    version: '1.1.0',
    inputSchema,
    examples: [
      { input: {}, description: '生成 heuristic 风险摘要' },
      { input: { useLlm: true }, description: 'LLM 生成具体建议' },
    ],
  },
  execute: execute as any,
}

toolRegistry.register(analyzeRiskTool)

export async function analyzeRisk(_service: TaskService): Promise<RiskAnalysis> {
  return execute({}) as Promise<RiskAnalysis>
}
