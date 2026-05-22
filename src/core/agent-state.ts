import { eventBus } from '../events/event-bus.js'

export type AgentState =
  | 'idle'           // 空闲
  | 'thinking'       // 思考中
  | 'analyzing'      // 分析中
  | 'working'        // 工作中
  | 'notifying'      // 通知中
  | 'error'          // 错误

export interface StateTransition {
  from: AgentState
  to: AgentState
  reason: string
  timestamp: number
}

export class AgentStateMachine {
  private currentState: AgentState = 'idle'
  private stateHistory: StateTransition[] = []
  private stateStartTime: number = Date.now()
  private listeners = new Set<(state: AgentState, prev: AgentState) => void>()
  private static instance: AgentStateMachine

  static getInstance(): AgentStateMachine {
    if (!AgentStateMachine.instance) {
      AgentStateMachine.instance = new AgentStateMachine()
    }
    return AgentStateMachine.instance
  }

  constructor() {
    // 监听事件自动转换状态
    eventBus.on('agent:thinking', () => { this.transition('thinking', '开始思考') })
    eventBus.on('agent:idle', () => { this.transition('idle', '进入空闲') })
    eventBus.on('action:started', () => { this.transition('working', '开始执行 Action') })
    eventBus.on('action:completed', () => { this.transition('idle', 'Action 完成') })
    eventBus.on('action:failed', () => { this.transition('error', 'Action 失败') })
    eventBus.on('agent:notify', () => { this.transition('notifying', '发送通知') })
  }

  getState(): AgentState {
    return this.currentState
  }

  getStateDuration(): number {
    return Date.now() - this.stateStartTime
  }

  transition(to: AgentState, reason: string): boolean {
    const from = this.currentState
    if (from === to) return false

    // 记录历史
    const transition: StateTransition = {
      from,
      to,
      reason,
      timestamp: Date.now(),
    }
    this.stateHistory.push(transition)

    // 限制历史记录长度
    if (this.stateHistory.length > 100) {
      this.stateHistory = this.stateHistory.slice(-50)
    }

    // 更新状态
    this.currentState = to
    this.stateStartTime = Date.now()

    // 通知监听器
    this.listeners.forEach(listener => listener(to, from))

    // 发送事件
    eventBus.emit('agent:message', {
      type: 'info',
      content: `状态: ${this.formatState(from)} → ${this.formatState(to)} (${reason})`,
    })

    return true
  }

  onStateChange(listener: (state: AgentState, prev: AgentState) => void): () => void {
    this.listeners.add(listener)
    return () => this.listeners.delete(listener)
  }

  getHistory(limit: number = 20): StateTransition[] {
    return this.stateHistory.slice(-limit)
  }

  getStats(): {
    current: AgentState
    duration: number
    historyCount: number
    stateCounts: Record<AgentState, number>
  } {
    const stateCounts: Record<string, number> = {}
    for (const t of this.stateHistory) {
      stateCounts[t.to] = (stateCounts[t.to] || 0) + 1
    }

    return {
      current: this.currentState,
      duration: this.getStateDuration(),
      historyCount: this.stateHistory.length,
      stateCounts: stateCounts as Record<AgentState, number>,
    }
  }

  private formatState(state: AgentState): string {
    const map: Record<AgentState, string> = {
      idle: '空闲',
      thinking: '思考',
      analyzing: '分析',
      working: '工作',
      notifying: '通知',
      error: '错误',
    }
    return map[state] || state
  }
}

export const agentState = AgentStateMachine.getInstance()
