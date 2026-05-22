export type EventName =
  | 'task:risk'
  | 'task:update'
  | 'task:deadline'
  | 'agent:message'
  | 'agent:notify'
  | 'agent:thinking'
  | 'agent:idle'
  | 'git:changed'
  | 'git:commit'
  | 'memory:updated'
  | 'action:started'
  | 'action:completed'
  | 'action:failed'
  | 'scheduler:tick'

export interface EventPayload {
  'task:risk': { taskId?: string; level?: 'high' | 'medium' | 'low'; reason?: string; tasks?: unknown[]; message?: string }
  'task:update': { taskId: string; changes: Record<string, unknown> }
  'task:deadline': { taskId: string; deadline: string; hoursLeft: number }
  'agent:message': { type?: 'info' | 'warning' | 'success' | 'error'; content: string; role?: string }
  'agent:notify': { title: string; body: string; priority: 'urgent' | 'normal' | 'low' }
  'agent:thinking': { topic: string }
  'agent:idle': { timestamp: number }
  'git:changed': { branch: string; files: string[] }
  'git:commit': { hash: string; message: string; author: string }
  'memory:updated': { key: string; value: unknown }
  'action:started': { actionId: string; name: string }
  'action:completed': { actionId: string; name: string; result: unknown }
  'action:failed': { actionId: string; name: string; error: string }
  'scheduler:tick': { timestamp: number }
}

type EventHandler<T extends EventName> = (payload: EventPayload[T]) => void | Promise<void>

export class EventBus {
  private listeners = new Map<EventName, Set<EventHandler<any>>>()
  private static instance: EventBus

  static getInstance(): EventBus {
    if (!EventBus.instance) {
      EventBus.instance = new EventBus()
    }
    return EventBus.instance
  }

  on<T extends EventName>(event: T, handler: EventHandler<T>): () => void {
    if (!this.listeners.has(event)) {
      this.listeners.set(event, new Set())
    }
    this.listeners.get(event)!.add(handler)

    // 返回取消订阅函数
    return () => {
      this.listeners.get(event)?.delete(handler)
    }
  }

  once<T extends EventName>(event: T, handler: EventHandler<T>): void {
    const unsubscribe = this.on(event, (payload) => {
      unsubscribe()
      handler(payload)
    })
  }

  emit<T extends EventName>(event: T, payload: EventPayload[T]): void {
    const handlers = this.listeners.get(event)
    if (!handlers) return

    handlers.forEach(handler => {
      try {
        const result = handler(payload)
        if (result instanceof Promise) {
          result.catch(err => console.error(`Event handler error for ${event}:`, err))
        }
      } catch (err) {
        console.error(`Event handler error for ${event}:`, err)
      }
    })
  }

  off<T extends EventName>(event: T, handler?: EventHandler<T>): void {
    if (!handler) {
      this.listeners.delete(event)
      return
    }
    this.listeners.get(event)?.delete(handler)
  }

  count(event: EventName): number {
    return this.listeners.get(event)?.size ?? 0
  }
}

export const eventBus = EventBus.getInstance()
