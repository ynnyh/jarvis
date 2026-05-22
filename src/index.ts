// ===== Core =====
export { toolRegistry, ToolRegistry } from './core/tool-registry.js'
export { agentState, AgentStateMachine } from './core/agent-state.js'

// ===== Events =====
export { eventBus, EventBus } from './events/event-bus.js'
export type { EventName, EventPayload } from './events/event-bus.js'

// ===== Memory =====
export { memoryStore, MemoryStore } from './memory/memory-store.js'
export type { MemoryEntry, MemoryQuery } from './memory/memory-store.js'

// ===== Actions =====
export { actionEngine, ActionEngine } from './actions/action-engine.js'
export type { ActionDefinition, ActionStep, ActionResult } from './actions/action-engine.js'

// ===== Scheduler =====
export { agentScheduler, AgentScheduler } from './scheduler/agent-scheduler.js'
export type { ScheduledTask } from './scheduler/agent-scheduler.js'

// ===== AI =====
export { contextBuilder, ContextBuilder } from './ai/context-builder.js'
export type { ContextOptions as AIContext } from './ai/context-builder.js'

// ===== Providers =====
export { BaseProvider } from './providers/base-provider.js'
export { ZenTaoProvider } from './providers/zentao-provider.js'
export { MockProvider } from './providers/mock-provider.js'
export { GitProvider } from './providers/git/git-provider.js'

// ===== Services =====
export { TaskService } from './services/task-service.js'

// ===== Tools =====
export {
  getTasksTool,
  getTodayTasksTool,
  getTaskDetailTool,
  analyzeRiskTool,
  getTaskCommitsTool,
  getDailyReviewTool,
} from './tools/index.js'

// ===== Shared =====
export type {
  Task,
  TaskStatus,
  Priority,
  Comment,
  TaskFilter,
  RiskAnalysis,
  DependencyRisk,
  ProviderConfig,
} from './shared/types.js'

// ===== 初始化 =====
import './actions/predefined-actions.js'

// 初始化调度器默认任务
import { agentScheduler } from './scheduler/agent-scheduler.js'

agentScheduler.register({
  id: 'default_risk_check',
  name: '默认风险检查',
  actionId: 'periodic_risk_check',
  cron: '*/10 * * * *',
  enabled: true,
})
