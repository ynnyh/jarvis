import { toolRegistry } from '../core/tool-registry.js'
import { eventBus } from '../events/event-bus.js'
import type { EventName } from '../events/event-bus.js'

export interface ActionStep {
  tool: string
  description?: string
  input?: Record<string, unknown>
  condition?: string
  onError?: 'continue' | 'stop' | 'retry'
  retryCount?: number
}

export interface ActionDefinition {
  id: string
  name: string
  description: string
  steps: ActionStep[]
  triggers?: Array<{
    event: EventName
    condition?: (payload: any) => boolean
  }>
  onComplete?: (results: unknown[]) => void
  onError?: (error: Error, stepIndex: number) => void
}

export interface ActionResult {
  actionId: string
  success: boolean
  stepResults: Array<{
    stepIndex: number
    tool: string
    success: boolean
    result?: unknown
    error?: string
    duration: number
  }>
  totalDuration: number
  startedAt: string
  completedAt: string
}

// 步骤间变量上下文
export interface StepContext {
  // 所有步骤的结果，按 stepIndex 存储
  results: Map<number, unknown>
  // 命名变量，供后续步骤引用
  variables: Map<string, unknown>
}

export class ActionEngine {
  private actions = new Map<string, ActionDefinition>()
  private runningActions = new Set<string>()
  private static instance: ActionEngine

  static getInstance(): ActionEngine {
    if (!ActionEngine.instance) {
      ActionEngine.instance = new ActionEngine()
    }
    return ActionEngine.instance
  }

  register(action: ActionDefinition): void {
    this.actions.set(action.id, action)

    if (action.triggers) {
      for (const trigger of action.triggers) {
        eventBus.on(trigger.event, (payload) => {
          if (!trigger.condition || trigger.condition(payload)) {
            this.execute(action.id).catch(console.error)
          }
        })
      }
    }
  }

  async execute(actionId: string, overrideInput?: Record<string, unknown>): Promise<ActionResult> {
    const action = this.actions.get(actionId)
    if (!action) {
      throw new Error(`Action ${actionId} not found`)
    }

    if (this.runningActions.has(actionId)) {
      throw new Error(`Action ${actionId} is already running`)
    }

    this.runningActions.add(actionId)
    eventBus.emit('action:started', { actionId, name: action.name })

    const startedAt = new Date().toISOString()
    const stepResults: ActionResult['stepResults'] = []
    let totalDuration = 0

    // 初始化步骤上下文
    const context: StepContext = {
      results: new Map(),
      variables: new Map(),
    }

    try {
      for (let i = 0; i < action.steps.length; i++) {
        const step = action.steps[i]
        const stepStart = Date.now()

        try {
          // 解析输入，支持变量引用
          const input = this.resolveInput(step.input, context, overrideInput)

          // 执行 Tool
          const result = await toolRegistry.execute(step.tool, input)
          const duration = Date.now() - stepStart
          totalDuration += duration

          // 存储结果到上下文
          context.results.set(i, result)

          // 自动提取命名变量（如果结果是对象）
          if (result && typeof result === 'object') {
            const resultObj = result as Record<string, unknown>
            // 存储关键字段作为变量
            for (const key of ['tasks', 'riskAnalysis', 'task', 'analysis']) {
              if (key in resultObj) {
                context.variables.set(key, resultObj[key])
              }
            }
          }

          stepResults.push({
            stepIndex: i,
            tool: step.tool,
            success: true,
            result,
            duration,
          })

          // 检查条件
          if (step.condition) {
            const shouldContinue = this.evaluateCondition(step.condition, result)
            if (!shouldContinue) {
              break
            }
          }
        } catch (error) {
          const duration = Date.now() - stepStart
          totalDuration += duration

          const errorMessage = error instanceof Error ? error.message : String(error)
          stepResults.push({
            stepIndex: i,
            tool: step.tool,
            success: false,
            error: errorMessage,
            duration,
          })

          const errorStrategy = step.onError || 'stop'
          if (errorStrategy === 'stop') {
            if (action.onError) {
              action.onError(error instanceof Error ? error : new Error(errorMessage), i)
            }
            throw error
          } else if (errorStrategy === 'retry' && step.retryCount && step.retryCount > 0) {
            let retries = 0
            while (retries < step.retryCount) {
              try {
                const input = this.resolveInput(step.input, context, overrideInput)
                const result = await toolRegistry.execute(step.tool, input)
                stepResults[stepResults.length - 1] = {
                  stepIndex: i,
                  tool: step.tool,
                  success: true,
                  result,
                  duration: Date.now() - stepStart,
                }
                break
              } catch {
                retries++
              }
            }
          }
        }
      }

      const completedAt = new Date().toISOString()
      const result: ActionResult = {
        actionId,
        success: stepResults.every(r => r.success),
        stepResults,
        totalDuration,
        startedAt,
        completedAt,
      }

      if (action.onComplete) {
        action.onComplete(stepResults.map(r => r.result))
      }

      eventBus.emit('action:completed', {
        actionId,
        name: action.name,
        result,
      })

      return result
    } catch (error) {
      const errorMessage = error instanceof Error ? error.message : String(error)
      eventBus.emit('action:failed', {
        actionId,
        name: action.name,
        error: errorMessage,
      })
      throw error
    } finally {
      this.runningActions.delete(actionId)
    }
  }

  // 解析输入，支持变量引用
  // 支持: ${step0.tasks}, ${variables.tasks}, ${prev.result}
  private resolveInput(
    input: Record<string, unknown> | undefined,
    context: StepContext,
    overrideInput?: Record<string, unknown>
  ): Record<string, unknown> {
    const resolved: Record<string, unknown> = {}

    // 合并 overrideInput
    if (overrideInput) {
      Object.assign(resolved, overrideInput)
    }

    // 解析 step input
    if (input) {
      for (const [key, value] of Object.entries(input)) {
        if (typeof value === 'string' && value.startsWith('${') && value.endsWith('}')) {
          // 变量引用
          const varPath = value.slice(2, -1)
          resolved[key] = this.resolveVariable(varPath, context)
        } else {
          resolved[key] = value
        }
      }
    }

    return resolved
  }

  // 解析变量路径
  // 支持: step0.tasks, variables.riskAnalysis, prev.result
  private resolveVariable(path: string, context: StepContext): unknown {
    const parts = path.split('.')
    const first = parts[0]

    if (first.startsWith('step')) {
      const stepIndex = parseInt(first.replace('step', ''))
      const result = context.results.get(stepIndex)
      return this.getNestedValue(result, parts.slice(1))
    }

    if (first === 'variables') {
      const varName = parts[1]
      return context.variables.get(varName)
    }

    if (first === 'prev') {
      // 获取上一个步骤的结果
      const maxIndex = Math.max(...Array.from(context.results.keys()), -1)
      const result = context.results.get(maxIndex)
      return this.getNestedValue(result, parts.slice(1))
    }

    return undefined
  }

  // 获取嵌套值
  private getNestedValue(obj: unknown, path: string[]): unknown {
    let current = obj
    for (const key of path) {
      if (current && typeof current === 'object') {
        current = (current as Record<string, unknown>)[key]
      } else {
        return undefined
      }
    }
    return current
  }

  private evaluateCondition(condition: string, result: unknown): boolean {
    try {
      const fn = new Function('result', `return ${condition}`)
      return fn(result)
    } catch {
      return true
    }
  }

  list(): ActionDefinition[] {
    return Array.from(this.actions.values())
  }

  get(id: string): ActionDefinition | undefined {
    return this.actions.get(id)
  }

  isRunning(actionId: string): boolean {
    return this.runningActions.has(actionId)
  }
}

export const actionEngine = ActionEngine.getInstance()
