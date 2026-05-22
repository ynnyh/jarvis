import { actionEngine } from './action-engine.js'
import { eventBus } from '../events/event-bus.js'
import { memoryStore } from '../memory/memory-store.js'

// ===== 开发工作流 Actions =====

// 1. 开始今日工作
// 步骤：获取今日任务 → 分析风险 → 记录到 Memory
actionEngine.register({
  id: 'start_today_work',
  name: '开始今日工作',
  description: '获取今日任务，分析风险，准备开发环境',
  steps: [
    {
      tool: 'get_today_tasks',
      description: '获取今日任务列表',
    },
    {
      tool: 'analyze_risk',
      description: '分析任务风险',
      // 使用上一步的结果作为输入
      input: {
        tasks: '${step0}',
      },
    },
    {
      tool: 'get_task_detail',
      description: '获取高优先级任务详情',
      // 条件：如果有高风险任务，获取详情
      condition: 'result && result.highPriorityTasks && result.highPriorityTasks.length > 0',
      input: {
        id: '${step1.highPriorityTasks.0.id}',
      },
      onError: 'continue', // 获取详情失败不影响整体流程
    },
  ],
  onComplete: (results) => {
    const tasks = results[0] as any[]
    const riskAnalysis = results[1] as any

    // 记录到 Memory
    if (riskAnalysis && riskAnalysis.summary) {
      memoryStore.add({
        type: 'analysis',
        content: `今日风险分析: ${riskAnalysis.summary}`,
        tags: ['daily', 'risk', 'analysis'],
        importance: 7,
      })
    }

    // 记录任务数量
    if (tasks && tasks.length > 0) {
      memoryStore.add({
        type: 'habit',
        content: `今日有 ${tasks.length} 个任务，其中 ${tasks.filter((t: any) => t.priority === 'urgent').length} 个紧急`,
        tags: ['daily', 'task-count'],
        importance: 5,
      })
    }

    // 发送事件
    eventBus.emit('agent:notify', {
      title: '今日工作准备就绪',
      body: `发现 ${tasks?.length || 0} 个任务，${riskAnalysis?.overdueTasks?.length || 0} 个风险`,
      priority: 'normal',
    })
  },
})

// 2. 代码提交前检查
// 步骤：获取 Git 状态 → 分析风险 → 生成提交建议
actionEngine.register({
  id: 'pre_commit_check',
  name: '提交前检查',
  description: '检查 Git 状态，分析代码变更风险',
  steps: [
    {
      tool: 'git_status',
      description: '获取 Git 修改文件',
    },
    {
      tool: 'analyze_risk',
      description: '分析当前任务风险',
      input: {
        context: 'pre-commit',
      },
    },
  ],
  onComplete: (results) => {
    const gitStatus = results[0] as any
    const riskAnalysis = results[1] as any

    const modifiedFiles = gitStatus?.modified || []
    const hasRisk = riskAnalysis?.overdueTasks?.length > 0

    // 记录到 Memory
    if (modifiedFiles.length > 0) {
      memoryStore.add({
        type: 'project',
        content: `提交前检查: ${modifiedFiles.length} 个文件修改，涉及 ${modifiedFiles.join(', ')}`,
        tags: ['git', 'commit', 'pre-check'],
        importance: 6,
      })
    }

    eventBus.emit('agent:notify', {
      title: hasRisk ? '⚠️ 提交前发现风险' : '✅ 提交前检查通过',
      body: hasRisk
        ? `有 ${riskAnalysis.overdueTasks.length} 个任务风险，建议先处理`
        : `修改了 ${modifiedFiles.length} 个文件，可以安全提交`,
      priority: hasRisk ? 'urgent' : 'normal',
    })
  },
})

// 3. 定时风险检查（Scheduler 调用）
actionEngine.register({
  id: 'periodic_risk_check',
  name: '定时风险检查',
  description: '定期检查任务风险并通知',
  steps: [
    {
      tool: 'get_today_tasks',
      description: '获取最新任务',
    },
    {
      tool: 'analyze_risk',
      description: '分析风险',
      input: {
        tasks: '${step0}',
      },
    },
  ],
  triggers: [
    {
      event: 'scheduler:tick',
      condition: (payload) => payload.taskId === 'periodic_risk_check',
    },
  ],
  onComplete: (results) => {
    const riskAnalysis = results[1] as any
    if (riskAnalysis && riskAnalysis.overdueTasks.length > 0) {
      eventBus.emit('task:risk', {
        level: 'high',
        tasks: riskAnalysis.overdueTasks,
        message: `发现 ${riskAnalysis.overdueTasks.length} 个任务已延期`,
      })
    }
  },
})

// 4. 生成日报
actionEngine.register({
  id: 'generate_daily_report',
  name: '生成日报',
  description: '汇总今日任务、风险和 Git 活动',
  steps: [
    {
      tool: 'get_today_tasks',
      description: '获取今日任务',
    },
    {
      tool: 'analyze_risk',
      description: '分析风险',
      input: {
        tasks: '${step0}',
      },
    },
    {
      tool: 'git_status',
      description: '获取 Git 活动',
    },
  ],
  onComplete: (results) => {
    const tasks = results[0] as any[]
    const riskAnalysis = results[1] as any
    const gitStatus = results[2] as any

    const report = {
      date: new Date().toISOString().split('T')[0],
      taskCount: tasks?.length || 0,
      urgentCount: tasks?.filter((t: any) => t.priority === 'urgent').length || 0,
      riskCount: riskAnalysis?.overdueTasks?.length || 0,
      gitFiles: gitStatus?.modified?.length || 0,
    }

    // 记录到 Memory
    memoryStore.add({
      type: 'analysis',
      content: `日报: 完成 ${report.taskCount} 个任务，${report.riskCount} 个风险，修改 ${report.gitFiles} 个文件`,
      tags: ['daily-report', 'summary'],
      importance: 6,
    })

    eventBus.emit('agent:message', {
      role: 'assistant',
      content: `📊 今日日报:\n- 任务: ${report.taskCount} 个 (${report.urgentCount} 紧急)\n- 风险: ${report.riskCount} 个\n- 代码: ${report.gitFiles} 个文件修改`,
    })
  },
})

// 5. 任务切换助手
// 当用户切换任务时，自动获取相关上下文
actionEngine.register({
  id: 'task_context_switch',
  name: '任务上下文切换',
  description: '切换任务时自动获取相关上下文',
  steps: [
    {
      tool: 'get_task_detail',
      description: '获取任务详情',
    },
    {
      tool: 'git_status',
      description: '获取相关代码变更',
    },
  ],
  onComplete: (results) => {
    const task = results[0] as any
    const gitStatus = results[1] as any

    // 查询 Memory 中相关记录
    const memories = memoryStore.query({
      search: task?.title || '',
      type: 'risk',
    })

    if (memories.length > 0) {
      eventBus.emit('agent:message', {
        role: 'assistant',
        content: `🧠 我记得这个任务:\n${memories.map((m) => `- ${m.content}`).join('\n')}`,
      })
    }
  },
})
