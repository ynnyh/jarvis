import { http, tokenManager, ACCOUNT, PASSWORD } from './request.js'

export interface ZenTaoTokenResponse {
  token: string
  expires?: number
}

export interface ZenTaoUser {
  id?: number
  account?: string
  realname?: string
  avatar?: string
}

export interface ZenTaoTask {
  id: number
  name: string
  desc?: string
  status: string
  pri: number
  estimate?: number
  consumed?: number
  left?: number
  deadline?: string
  assignedTo?: ZenTaoUser | string
  assignedDate?: string
  openedBy?: ZenTaoUser
  openedDate?: string
  finishedBy?: ZenTaoUser
  finishedDate?: string
  closedBy?: ZenTaoUser
  closedDate?: string
  story?: number
  parent?: number
  execution?: number
  project?: number
  type?: string
  realStarted?: string
  estStarted?: string
}

export interface ZenTaoExecution {
  id: number
  name: string
  project?: number
  status?: string
  type?: string
  begin?: string
  end?: string
}

export interface ZenTaoProject {
  id: number
  name: string
  status?: string
  code?: string
  model?: string
  begin?: string
  end?: string
}

export interface ZenTaoTaskListResponse {
  tasks: ZenTaoTask[]
  total?: number
  page?: number
  limit?: number
}

export interface ZenTaoExecutionListResponse {
  executions: ZenTaoExecution[]
  total?: number
  page?: number
  limit?: number
}

export interface ZenTaoProjectListResponse {
  projects: ZenTaoProject[]
  total?: number
  page?: number
  limit?: number
}

export interface ZenTaoTaskDetailResponse {
  task: ZenTaoTask
}

export class ZenTaoProvider {
  private authenticated = false

  async authenticate(): Promise<string> {
    const cached = tokenManager.getToken()
    if (cached) {
      console.log('[ZenTaoProvider] 使用缓存的 Token')
      this.authenticated = true
      return cached
    }

    console.log('[ZenTaoProvider] 正在获取 Token...')
    console.log(`[ZenTaoProvider] 账号: ${ACCOUNT}`)

    try {
      const response = await http.post<ZenTaoTokenResponse>('/api.php/v1/tokens', {
        account: ACCOUNT,
        password: PASSWORD,
      })

      const token = response.data.token
      if (!token) {
        throw new Error('响应中未包含 token')
      }

      tokenManager.setToken(token, response.data.expires)
      this.authenticated = true
      console.log(`[ZenTaoProvider] Token 获取成功: ${token.slice(0, 10)}...`)
      return token
    } catch (error: any) {
      this.authenticated = false
      console.error('[ZenTaoProvider] 认证失败:', error.message)
      throw error
    }
  }

  async getExecutions(status: string = 'all'): Promise<ZenTaoExecution[]> {
    await this.ensureAuthenticated()

    const allExecutions: ZenTaoExecution[] = []
    let page = 1
    let totalPages = 1

    console.log(`[ZenTaoProvider] 获取执行列表 (status=${status})...`)

    try {
      while (page <= totalPages) {
        const params: Record<string, string | number> = { status, recPerPage: 100, page }

        const response = await http.get<ZenTaoExecutionListResponse>('/api.php/v1/executions', { params })
        const executions = response.data.executions || []
        allExecutions.push(...executions)

        // 使用 API 返回的实际 limit 计算总页数
        totalPages = Math.ceil((response.data.total || 0) / (response.data.limit || 20))
        page++
      }

      console.log(`[ZenTaoProvider] 获取到 ${allExecutions.length} 个执行`)
      return allExecutions
    } catch (error: any) {
      console.error('[ZenTaoProvider] 获取执行列表失败:', error.message)
      throw error
    }
  }

  async getTasksByExecution(executionId: number): Promise<ZenTaoTask[]> {
    await this.ensureAuthenticated()

    console.log(`[ZenTaoProvider] 获取执行 ${executionId} 的任务...`)

    const allTasks: ZenTaoTask[] = []
    let page = 1
    let totalPages = 1

    try {
      while (page <= totalPages) {
        const response = await http.get<ZenTaoTaskListResponse>(`/api.php/v1/executions/${executionId}/tasks`, {
          params: { recPerPage: 100, page },
        })
        const tasks = response.data.tasks || []
        allTasks.push(...tasks)

        // 使用 API 返回的实际 limit 计算总页数
        totalPages = Math.ceil((response.data.total || 0) / (response.data.limit || 100))
        page++
      }

      console.log(`[ZenTaoProvider] 执行 ${executionId} 有 ${allTasks.length} 个任务`)
      return allTasks
    } catch (error: any) {
      console.error(`[ZenTaoProvider] 获取执行 ${executionId} 任务失败:`, error.message)
      return []
    }
  }

  async getProjects(): Promise<ZenTaoProject[]> {
    await this.ensureAuthenticated()

    const allProjects: ZenTaoProject[] = []
    let page = 1
    let totalPages = 1

    console.log('[ZenTaoProvider] 获取项目列表...')

    try {
      while (page <= totalPages) {
        const response = await http.get<ZenTaoProjectListResponse>('/api.php/v1/projects', {
          params: { page, limit: 100 },
        })
        const projects = response.data.projects || []
        allProjects.push(...projects)

        totalPages = Math.ceil((response.data.total || 0) / (response.data.limit || 20))
        page++
      }

      console.log(`[ZenTaoProvider] 获取到 ${allProjects.length} 个项目`)
      return allProjects
    } catch (error: any) {
      console.error('[ZenTaoProvider] 获取项目列表失败:', error.message)
      throw error
    }
  }

  async getExecutionsByProject(projectId: number): Promise<ZenTaoExecution[]> {
    await this.ensureAuthenticated()

    const allExecutions: ZenTaoExecution[] = []
    let page = 1
    let totalPages = 1

    try {
      while (page <= totalPages) {
        const response = await http.get<ZenTaoExecutionListResponse>(`/api.php/v1/projects/${projectId}/executions`, {
          params: { page, limit: 100 },
        })
        const executions = response.data.executions || []
        allExecutions.push(...executions)

        totalPages = Math.ceil((response.data.total || 0) / (response.data.limit || 20))
        page++
      }

      return allExecutions
    } catch (error: any) {
      console.error(`[ZenTaoProvider] 获取项目 ${projectId} 执行列表失败:`, error.message)
      return []
    }
  }

  async getMyTasks(): Promise<ZenTaoTask[]> {
    await this.ensureAuthenticated()

    console.log('[ZenTaoProvider] 获取我的任务...')

    try {
      // 方法1: 通过所有执行获取任务
      const executions = await this.getExecutions()
      const allTasks: ZenTaoTask[] = []

      for (const exec of executions) {
        const tasks = await this.getTasksByExecution(exec.id)
        const myTasks = tasks.filter(t => {
          const assignee = typeof t.assignedTo === 'string' ? t.assignedTo : t.assignedTo?.account
          return assignee === ACCOUNT && t.status !== 'closed'
        })
        allTasks.push(...myTasks)
      }

      // 方法2: 通过项目→执行获取任务（补充）
      const projects = await this.getProjects()
      for (const project of projects) {
        const projectExecutions = await this.getExecutionsByProject(project.id)
        for (const exec of projectExecutions) {
          // 避免重复获取已遍历过的执行
          if (!executions.find(e => e.id === exec.id)) {
            const tasks = await this.getTasksByExecution(exec.id)
            const myTasks = tasks.filter(t => {
              const assignee = typeof t.assignedTo === 'string' ? t.assignedTo : t.assignedTo?.account
              return assignee === ACCOUNT && t.status !== 'closed'
            })
            allTasks.push(...myTasks)
          }
        }
      }

      // 去重
      const uniqueTasks = Array.from(new Map(allTasks.map(t => [t.id, t])).values())

      console.log(`[ZenTaoProvider] 获取到 ${uniqueTasks.length} 个我的任务（已过滤 closed，已去重）`)
      return uniqueTasks
    } catch (error: any) {
      console.error('[ZenTaoProvider] 获取我的任务失败:', error.message)
      throw error
    }
  }

  async getTaskDetail(taskId: number): Promise<ZenTaoTask> {
    await this.ensureAuthenticated()

    console.log(`[ZenTaoProvider] 获取任务详情: ${taskId}`)

    try {
      const response = await http.get<ZenTaoTaskDetailResponse>(`/api.php/v1/tasks/${taskId}`)
      return response.data.task
    } catch (error: any) {
      console.error(`[ZenTaoProvider] 获取任务 ${taskId} 详情失败:`, error.message)
      throw error
    }
  }

  async getTodayTasks(): Promise<ZenTaoTask[]> {
    const allTasks = await this.getMyTasks()
    const today = new Date().toISOString().split('T')[0]

    const todayTasks = allTasks.filter(t => {
      if (!t.deadline) return false
      return t.deadline.startsWith(today)
    })

    console.log(`[ZenTaoProvider] 今日截止任务: ${todayTasks.length} 个`)
    return todayTasks
  }

  private async ensureAuthenticated(): Promise<void> {
    if (!this.authenticated || !tokenManager.getToken()) {
      await this.authenticate()
    }
  }

  isAuthenticated(): boolean {
    return this.authenticated && !!tokenManager.getToken()
  }

  logout(): void {
    tokenManager.clear()
    this.authenticated = false
    console.log('[ZenTaoProvider] 已登出')
  }
}

export const zentaoProvider = new ZenTaoProvider()
