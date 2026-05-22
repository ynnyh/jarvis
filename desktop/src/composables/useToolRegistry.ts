import { invoke } from '@tauri-apps/api/core'
import type { Task, RiskAnalysis } from '../stores/app'

// Tool Registry - 通过 Tauri IPC 调用 Rust 后端，再调用 Node.js Phase 3 核心

export interface ToolResult {
  success: boolean
  data?: any
  error?: string
}

export const ToolRegistry = {
  // ===== Tool 调用 =====
  
  async executeTool(name: string, input?: Record<string, any>): Promise<ToolResult> {
    return await invoke('tool_execute', { name, input })
  },

  async listTools(): Promise<any[]> {
    return await invoke('tool_list')
  },

  async getTasks(): Promise<Task[]> {
    const result = await this.executeTool('get_tasks')
    return result.data || []
  },

  async getTodayTasks(): Promise<Task[]> {
    const result = await this.executeTool('get_today_tasks')
    return result.data || []
  },

  async getTaskDetail(id: string): Promise<Task | null> {
    const result = await this.executeTool('get_task_detail', { id })
    return result.data || null
  },

  async analyzeRisk(): Promise<RiskAnalysis | null> {
    const result = await this.executeTool('analyze_risk')
    return result.data || null
  },

  async getTaskCommits(input?: {
    range?: 'today' | 'yesterday' | 'thisWeek' | 'lastWeek' | 'last7days' | 'last30days' | 'thisMonth'
    since?: string
    until?: string
    taskIds?: Array<string | number>
    includeBody?: boolean
  }): Promise<any> {
    const result = await this.executeTool('get_task_commits', input as any)
    return result.data || null
  },

  // ===== Action 调用 =====

  async executeAction(id: string): Promise<ToolResult> {
    return await invoke('action_execute', { id })
  },

  async listActions(): Promise<any[]> {
    return await invoke('action_list')
  },

  async startTodayWork(): Promise<ToolResult> {
    return await this.executeAction('start_today_work')
  },

  async periodicRiskCheck(): Promise<ToolResult> {
    return await this.executeAction('periodic_risk_check')
  },

  async generateDailyReport(): Promise<ToolResult> {
    return await this.executeAction('generate_daily_report')
  },

  // ===== Memory 操作 =====

  async addMemory(
    type: 'project' | 'risk' | 'habit' | 'analysis' | 'preference',
    content: string,
    tags: string[],
    importance: number
  ): Promise<any> {
    return await invoke('memory_add', { type, content, tags, importance })
  },

  async listMemories(): Promise<any[]> {
    return await invoke('memory_list')
  },

  // ===== Agent 状态 =====

  async getAgentState(): Promise<{ state: string; duration: number; historyCount: number }> {
    return await invoke('agent_get_state')
  },

  // ===== Scheduler =====

  async startScheduler(): Promise<void> {
    return await invoke('scheduler_start')
  },

  async getSchedulerStatus(): Promise<any> {
    return await invoke('scheduler_status')
  },

  // ===== Context Builder =====

  async buildContext(): Promise<string> {
    return await invoke('context_build')
  },

  // ===== Git =====

  async getGitInfo(): Promise<any> {
    return await invoke('git_info')
  },
}
