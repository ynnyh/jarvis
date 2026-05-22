import { eventBus } from '../events/event-bus.js'
import { actionEngine } from '../actions/action-engine.js'
import { memoryStore } from '../memory/memory-store.js'

export interface ScheduledTask {
  id: string
  name: string
  actionId: string
  cron: string // 简化版 cron: "*/10 * * * *" 每10分钟
  enabled: boolean
  lastRun?: string
  nextRun?: string
  runCount: number
}

export class AgentScheduler {
  private tasks = new Map<string, ScheduledTask>()
  private timers = new Map<string, ReturnType<typeof setInterval>>()
  private running = false
  private static instance: AgentScheduler

  static getInstance(): AgentScheduler {
    if (!AgentScheduler.instance) {
      AgentScheduler.instance = new AgentScheduler()
    }
    return AgentScheduler.instance
  }

  // 简化版 cron 解析，支持: */N * * * * (每N分钟)
  private parseCron(cron: string): number {
    const parts = cron.split(' ')
    if (parts[0].startsWith('*/')) {
      const minutes = parseInt(parts[0].slice(2))
      return minutes * 60 * 1000
    }
    // 默认 10 分钟
    return 600000
  }

  register(task: Omit<ScheduledTask, 'lastRun' | 'nextRun' | 'runCount'>): void {
    const fullTask: ScheduledTask = {
      ...task,
      runCount: 0,
    }
    this.tasks.set(task.id, fullTask)

    if (task.enabled) {
      this.startTask(task.id)
    }
  }

  private startTask(taskId: string): void {
    const task = this.tasks.get(taskId)
    if (!task || !task.enabled) return

    const interval = this.parseCron(task.cron)

    // 立即执行一次
    this.runTask(taskId)

    // 设置定时器
    const timer = setInterval(() => {
      this.runTask(taskId)
    }, interval)

    this.timers.set(taskId, timer)
  }

  private async runTask(taskId: string): Promise<void> {
    const task = this.tasks.get(taskId)
    if (!task) return

    const now = new Date().toISOString()
    task.lastRun = now
    task.runCount++

    // 计算下次运行时间
    const interval = this.parseCron(task.cron)
    task.nextRun = new Date(Date.now() + interval).toISOString()

    eventBus.emit('scheduler:tick', { timestamp: Date.now() })

    try {
      eventBus.emit('agent:thinking', { topic: task.name })

      await actionEngine.execute(task.actionId)

      // 记录到 memory
      memoryStore.add({
        type: 'habit',
        content: `定时任务执行: ${task.name}`,
        tags: ['scheduler', 'auto'],
        importance: 3,
      })
    } catch (error) {
      const errorMessage = error instanceof Error ? error.message : String(error)
      eventBus.emit('agent:message', {
        type: 'error',
        content: `定时任务 ${task.name} 执行失败: ${errorMessage}`,
      })
    } finally {
      eventBus.emit('agent:idle', { timestamp: Date.now() })
    }
  }

  start(): void {
    if (this.running) return
    this.running = true

    for (const [id, task] of this.tasks) {
      if (task.enabled) {
        this.startTask(id)
      }
    }

    eventBus.emit('agent:message', {
      type: 'info',
      content: 'Agent 调度器已启动',
    })
  }

  stop(): void {
    this.running = false
    for (const [id, timer] of this.timers) {
      clearInterval(timer)
      this.timers.delete(id)
    }

    eventBus.emit('agent:message', {
      type: 'info',
      content: 'Agent 调度器已停止',
    })
  }

  enable(taskId: string): void {
    const task = this.tasks.get(taskId)
    if (!task) return
    task.enabled = true
    this.startTask(taskId)
  }

  disable(taskId: string): void {
    const task = this.tasks.get(taskId)
    if (!task) return
    task.enabled = false
    const timer = this.timers.get(taskId)
    if (timer) {
      clearInterval(timer)
      this.timers.delete(taskId)
    }
  }

  list(): ScheduledTask[] {
    return Array.from(this.tasks.values())
  }

  getStatus(): { running: boolean; taskCount: number; activeTasks: number } {
    return {
      running: this.running,
      taskCount: this.tasks.size,
      activeTasks: Array.from(this.tasks.values()).filter(t => t.enabled).length,
    }
  }
}

export const agentScheduler = AgentScheduler.getInstance()
