// Mock 版本 - 用于测试前端 UI 而不需要 Tauri IPC
// 直接调用 Node.js 核心

import type { Task, RiskAnalysis } from '../stores/app'

export interface ToolResult {
  success: boolean
  data?: any
  error?: string
}

// 模拟 IPC 调用 - 实际通过 HTTP 或直接调用
export const ToolRegistry = {
  async executeTool(name: string, input?: Record<string, any>): Promise<ToolResult> {
    console.log(`[Mock] Executing tool: ${name}`, input)
    // 返回模拟数据
    return { success: true, data: { message: 'Mock result' } }
  },

  async listTools(): Promise<any[]> {
    return [
      { name: 'get_tasks', description: '获取任务列表' },
      { name: 'get_today_tasks', description: '获取今日任务' },
      { name: 'analyze_risk', description: '分析风险' },
    ]
  },

  async getTasks(): Promise<Task[]> {
    return []
  },

  async getTodayTasks(): Promise<Task[]> {
    // 返回模拟今日任务
    return [
      {
        id: '102',
        title: '订单列表页面优化',
        description: '优化订单列表加载性能，支持分页和筛选。',
        status: 'wait',
        priority: 'urgent',
        estimatedHours: 8,
        consumedHours: 0,
        deadline: '2026-05-14',
        assignee: '张三',
      },
      {
        id: '103',
        title: '修复支付回调漏洞',
        description: '修复安全漏洞',
        status: 'doing',
        priority: 'urgent',
        estimatedHours: 4,
        consumedHours: 2,
        deadline: '2026-05-13',
        assignee: '张三',
      },
    ]
  },

  async getTaskDetail(id: string): Promise<Task | null> {
    return null
  },

  async analyzeRisk(): Promise<RiskAnalysis | null> {
    return {
      overdueTasks: [
        {
          id: '103',
          title: '修复支付回调漏洞',
          description: '修复安全漏洞',
          status: 'doing',
          priority: 'urgent',
          estimatedHours: 4,
          consumedHours: 2,
          deadline: '2026-05-13',
          assignee: '张三',
        },
      ],
      highPriorityTasks: [
        {
          id: '102',
          title: '订单列表页面优化',
          description: '优化订单列表加载性能',
          status: 'wait',
          priority: 'urgent',
          estimatedHours: 8,
          consumedHours: 0,
          deadline: '2026-05-14',
          assignee: '张三',
        },
        {
          id: '103',
          title: '修复支付回调漏洞',
          description: '修复安全漏洞',
          status: 'doing',
          priority: 'urgent',
          estimatedHours: 4,
          consumedHours: 2,
          deadline: '2026-05-13',
          assignee: '张三',
        },
      ],
      dependencyRisks: [],
      summary: '发现 1 个已延期任务，需要立即处理。\n有 2 个高优先级任务待处理。',
    }
  },

  async executeAction(id: string): Promise<ToolResult> {
    console.log(`[Mock] Executing action: ${id}`)
    // 模拟延迟
    await new Promise(resolve => setTimeout(resolve, 1000))
    return { success: true, data: { action: id, completed: true } }
  },

  async listActions(): Promise<any[]> {
    return [
      { id: 'start_today_work', description: '开始今日工作' },
      { id: 'periodic_risk_check', description: '定期检查风险' },
    ]
  },

  async startTodayWork(): Promise<ToolResult> {
    console.log('[Mock] Starting today work...')
    await new Promise(resolve => setTimeout(resolve, 1500))
    return { success: true, data: { tasks: 2, risks: 1 } }
  },

  async periodicRiskCheck(): Promise<ToolResult> {
    return { success: true, data: {} }
  },

  async generateDailyReport(): Promise<ToolResult> {
    return { success: true, data: { report: '日报内容' } }
  },

  async addMemory(
    type: 'project' | 'risk' | 'habit' | 'analysis' | 'preference',
    content: string,
    tags: string[],
    importance: number
  ): Promise<any> {
    return {
      id: `mem_${Date.now()}`,
      type,
      content,
      tags,
      importance,
      created_at: new Date().toISOString(),
    }
  },

  async listMemories(): Promise<any[]> {
    return [
      { id: '1', type: 'risk', content: 'workflow.ts 风险较高', tags: ['workflow', 'risk'], importance: 8 },
    ]
  },

  async getAgentState(): Promise<{ state: string; duration: number; historyCount: number }> {
    return { state: 'idle', duration: 0, historyCount: 5 }
  },

  async startScheduler(): Promise<void> {
    console.log('[Mock] Scheduler started')
  },

  async getSchedulerStatus(): Promise<any> {
    return { running: true, taskCount: 1 }
  },

  async buildContext(): Promise<string> {
    return '# AI 上下文\n\n## 当前时间\n2026-05-14\n\n## 相关记忆\n- workflow.ts 风险较高\n\n## 可用工具\n- get_tasks\n- analyze_risk'
  },

  async getGitInfo(): Promise<any> {
    return { branch: 'main', commitCount: 10 }
  },
}
