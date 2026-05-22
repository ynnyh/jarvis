import { TaskService } from '../services/task-service.js'
import { memoryStore } from '../memory/memory-store.js'
import { GitProvider } from '../providers/git/git-provider.js'
import { toolRegistry } from '../core/tool-registry.js'
import { MockProvider } from '../providers/mock-provider.js'

export interface ContextOptions {
  // 是否包含任务上下文
  includeTasks?: boolean
  // 是否包含记忆
  includeMemory?: boolean
  // 是否包含 Git 上下文
  includeGit?: boolean
  // 是否包含可用工具
  includeTools?: boolean
  // 记忆查询关键词
  memoryQuery?: string
  // 记忆数量限制
  memoryLimit?: number
  // 上下文用途：'analysis' | 'chat' | 'workflow'
  purpose?: 'analysis' | 'chat' | 'workflow'
}

export class ContextBuilder {
  private static instance: ContextBuilder

  static getInstance(): ContextBuilder {
    if (!ContextBuilder.instance) {
      ContextBuilder.instance = new ContextBuilder()
    }
    return ContextBuilder.instance
  }

  async buildPrompt(options: ContextOptions = {}): Promise<string> {
    const {
      includeTasks = true,
      includeMemory = true,
      includeGit = true,
      includeTools = true,
      memoryQuery = '',
      memoryLimit = 5,
      purpose = 'analysis',
    } = options

    const sections: string[] = []

    // 1. 系统角色定义
    sections.push(this.buildSystemRole(purpose))

    // 2. 当前时间
    sections.push(this.buildTimeContext())

    // 3. 任务上下文
    if (includeTasks) {
      const taskContext = await this.buildTaskContext()
      sections.push(taskContext)
    }

    // 4. Git 上下文
    if (includeGit) {
      const gitContext = await this.buildGitContext()
      sections.push(gitContext)
    }

    // 5. 长期记忆（核心：参与推理）
    if (includeMemory) {
      const memoryContext = await this.buildMemoryContext(memoryQuery, memoryLimit)
      sections.push(memoryContext)
    }

    // 6. 可用工具
    if (includeTools) {
      const toolContext = this.buildToolContext()
      sections.push(toolContext)
    }

    // 7. 工作流指导
    if (purpose === 'workflow') {
      sections.push(this.buildWorkflowGuidance())
    }

    return sections.filter(Boolean).join('\n\n')
  }

  // 系统角色定义
  private buildSystemRole(purpose: string): string {
    const roles: Record<string, string> = {
      analysis: `你是 Jarvis，一位资深的软件开发项目助手。
你擅长：
- 分析禅道任务风险
- 理解 Git 代码上下文
- 识别开发流程中的问题
- 基于历史经验给出建议

分析原则：
1. 优先关注延期风险
2. 识别任务依赖关系
3. 结合代码变更评估影响
4. 参考历史类似问题`,

      chat: `你是 Jarvis，开发者的 AI 工作搭子。
你记得项目历史，理解开发习惯，能提供有针对性的建议。

交流风格：
1. 简洁直接，不废话
2. 基于事实，不猜测
3. 主动提醒风险
4. 记住用户偏好`,

      workflow: `你是 Jarvis，负责执行开发工作流编排。
你可以调用各种工具来完成复杂任务。

执行原则：
1. 按顺序执行步骤
2. 上一步结果为下一步输入
3. 遇到错误及时停止
4. 记录执行过程到 Memory`,
    }

    return `# 系统角色\n\n${roles[purpose] || roles.analysis}`
  }

  // 时间上下文
  private buildTimeContext(): string {
    const now = new Date()
    const hour = now.getHours()
    let timeDesc = ''

    if (hour < 9) timeDesc = '早晨'
    else if (hour < 12) timeDesc = '上午'
    else if (hour < 14) timeDesc = '中午'
    else if (hour < 18) timeDesc = '下午'
    else timeDesc = '晚上'

    return `# 当前时间\n\n- 日期: ${now.toLocaleDateString('zh-CN')}\n- 时间: ${now.toLocaleTimeString('zh-CN')}\n- 时段: ${timeDesc}`
  }

  // 任务上下文
  private async buildTaskContext(): Promise<string> {
    try {
      const provider = new MockProvider({ baseUrl: '', username: '', password: '' })
      const taskService = new TaskService(provider)
      const tasks = await taskService.getAllTasks({})
      const urgentTasks = tasks.filter((t: any) => t.priority === 'urgent')
      const overdueTasks = tasks.filter((t: any) => {
        if (!t.deadline) return false
        return new Date(t.deadline) < new Date()
      })

      let context = `# 任务上下文\n\n`
      context += `今日任务总数: ${tasks.length}\n`
      context += `紧急任务: ${urgentTasks.length}\n`
      context += `已延期: ${overdueTasks.length}\n\n`

      if (tasks.length > 0) {
        context += `## 任务列表\n\n`
        for (const task of tasks.slice(0, 10)) {
          const status = task.status === 'doing' ? '进行中' : task.status === 'wait' ? '未开始' : task.status
          context += `- [${status}] ${task.title} (${task.priority === 'urgent' ? '紧急' : task.priority === 'high' ? '高优' : '普通'})\n`
          if (task.deadline) {
            context += `  截止: ${task.deadline}\n`
          }
        }
      }

      // 关联 Memory 中的任务风险
      const taskMemories = memoryStore.query({
        type: 'risk',
        search: '延期',
      })

      if (taskMemories.length > 0) {
        context += `\n## 历史风险提醒\n\n`
        for (const mem of taskMemories.slice(0, 3)) {
          context += `- ${mem.content}\n`
        }
      }

      return context
    } catch (error) {
      return `# 任务上下文\n\n获取任务信息失败: ${error}`
    }
  }

  // Git 上下文
  private async buildGitContext(): Promise<string> {
    try {
      const gitProvider = new GitProvider()
      const repoInfo = await gitProvider.getRepoInfo()
      if (!repoInfo) {
        return ''
      }

      const status = await gitProvider.getStatus()
      const recentCommits = await gitProvider.getRecentCommits(3)

      let context = `# Git 上下文\n\n`
      context += `当前分支: ${repoInfo.branch}\n`
      context += `提交总数: ${repoInfo.commitCount}\n\n`

      if (recentCommits.length > 0) {
        context += `## 最近提交\n\n`
        for (const commit of recentCommits) {
          context += `- ${commit.message} (${commit.author}, ${commit.date})\n`
        }
      }

      if (status && (status.modified.length > 0 || status.added.length > 0)) {
        context += `\n## 未提交修改\n\n`
        if (status.modified.length > 0) {
          context += `修改: ${status.modified.join(', ')}\n`
        }
        if (status.added.length > 0) {
          context += `新增: ${status.added.join(', ')}\n`
        }
      }

      return context
    } catch {
      return ''
    }
  }

  // 记忆上下文（核心：参与推理）
  private async buildMemoryContext(query: string, limit: number): Promise<string> {
    // 获取相关记忆
    const memories = query
      ? memoryStore.getRelevantMemories(query, limit)
      : memoryStore.query({})

    if (memories.length === 0) {
      return ''
    }

    let context = `# 项目记忆\n\n`
    context += `以下是从长期记忆中提取的相关信息，请结合这些信息进行分析和决策：\n\n`

    // 按类型分组
    const byType: Record<string, typeof memories> = {}
    for (const mem of memories) {
      if (!byType[mem.type]) byType[mem.type] = []
      byType[mem.type].push(mem)
    }

    // 风险记忆（最高优先级）
    if (byType['risk']) {
      context += `## ⚠️ 风险记录\n\n`
      for (const mem of byType['risk']) {
        context += `- ${mem.content} (重要度: ${mem.importance}/10)\n`
      }
      context += `\n`
    }

    // 项目知识
    if (byType['project']) {
      context += `## 📚 项目知识\n\n`
      for (const mem of byType['project']) {
        context += `- ${mem.content}\n`
      }
      context += `\n`
    }

    // 历史分析
    if (byType['analysis']) {
      context += `## 📊 历史分析\n\n`
      for (const mem of byType['analysis']) {
        context += `- ${mem.content}\n`
      }
      context += `\n`
    }

    // 用户习惯
    if (byType['habit']) {
      context += `## 👤 工作习惯\n\n`
      for (const mem of byType['habit']) {
        context += `- ${mem.content}\n`
      }
      context += `\n`
    }

    // 偏好设置
    if (byType['preference']) {
      context += `## ⚙️ 偏好设置\n\n`
      for (const mem of byType['preference']) {
        context += `- ${mem.content}\n`
      }
    }

    return context
  }

  // 工具上下文
  private buildToolContext(): string {
    const tools = toolRegistry.search('')

    let context = `# 可用工具\n\n`
    context += `你可以使用以下工具来完成任务：\n\n`

    for (const tool of tools) {
      context += `## ${tool.name}\n`
      context += `${tool.description}\n`
      if (tool.inputSchema) {
        context += `参数: ${JSON.stringify(tool.inputSchema)}\n`
      }
      context += `\n`
    }

    return context
  }

  // 工作流指导
  private buildWorkflowGuidance(): string {
    return `# 工作流执行指导\n\n1. 按顺序执行每个步骤\n2. 上一步的输出作为下一步的输入\n3. 使用 \${stepN.field} 引用之前的结果\n4. 遇到错误时根据 onError 策略处理\n5. 完成后记录结果到 Memory`
  }
}

export const contextBuilder = ContextBuilder.getInstance()
