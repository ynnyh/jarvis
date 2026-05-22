import axios, { AxiosInstance } from 'axios'
import dotenv from 'dotenv'

dotenv.config()

const BASE_URL = process.env.ZENTAO_BASE_URL || ''
const ACCOUNT = process.env.ZENTAO_ACCOUNT || ''
const PASSWORD = process.env.ZENTAO_PASSWORD || ''

export interface ZenTaoTask {
  id: number
  name: string
  status: string
  pri: number
  deadline?: string
  assignedTo?: string | { account?: string; realname?: string }
  execution?: string
  project?: string
}

export interface ZenTaoExecution {
  id: number
  name: string
  status?: string
  project?: number
}

export interface ZenTaoProject {
  id: number
  name: string
  status?: string
}

export class ZenTaoTokenProvider {
  private http: AxiosInstance
  private token: string | null = null

  constructor() {
    this.http = axios.create({
      baseURL: BASE_URL,
      timeout: 30000,
      headers: {
        'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36',
      },
    })
  }

  async authenticate(): Promise<string> {
    if (this.token) return this.token

    console.log('[ZenTaoTokenProvider] 获取 Token...')
    const res = await this.http.post('/api.php/v1/tokens', {
      account: ACCOUNT,
      password: PASSWORD,
    })

    this.token = res.data.token
    console.log(`[ZenTaoTokenProvider] Token 获取成功: ${this.token?.slice(0, 15)}...`)
    return this.token!
  }

  async getProjects(): Promise<ZenTaoProject[]> {
    const token = await this.authenticate()
    const allProjects: ZenTaoProject[] = []
    let page = 1
    let totalPages = 1

    console.log('[ZenTaoTokenProvider] 获取项目列表...')

    while (page <= totalPages) {
      const res = await this.http.get('/api.php/v1/projects', {
        headers: { Token: token },
        params: { page, limit: 100 },
      })

      const projects = res.data.projects || []
      allProjects.push(...projects)

      if (page === 1) {
        totalPages = Math.ceil((res.data.total || 0) / (res.data.limit || 20))
      }
      page++
    }

    console.log(`[ZenTaoTokenProvider] 获取到 ${allProjects.length} 个项目`)
    return allProjects
  }

  async getExecutionsByProject(projectId: number): Promise<ZenTaoExecution[]> {
    const token = await this.authenticate()
    const allExecutions: ZenTaoExecution[] = []
    let page = 1
    let totalPages = 1

    while (page <= totalPages) {
      const res = await this.http.get(`/api.php/v1/projects/${projectId}/executions`, {
        headers: { Token: token },
        params: { page, limit: 100 },
      })

      const executions = res.data.executions || []
      allExecutions.push(...executions)

      if (page === 1) {
        totalPages = Math.ceil((res.data.total || 0) / (res.data.limit || 20))
      }
      page++
    }

    return allExecutions
  }

  async getAllExecutions(): Promise<ZenTaoExecution[]> {
    const token = await this.authenticate()
    const allExecutions: ZenTaoExecution[] = []
    let page = 1
    let totalPages = 1

    console.log('[ZenTaoTokenProvider] 获取所有执行...')

    while (page <= totalPages) {
      const res = await this.http.get('/api.php/v1/executions', {
        headers: { Token: token },
        params: { status: 'all', recPerPage: 100, page },
      })

      const executions = res.data.executions || []
      allExecutions.push(...executions)

      if (page === 1) {
        totalPages = Math.ceil((res.data.total || 0) / (res.data.limit || 20))
      }
      page++
    }

    console.log(`[ZenTaoTokenProvider] 获取到 ${allExecutions.length} 个执行`)
    return allExecutions
  }

  async getTasksByExecution(executionId: number): Promise<ZenTaoTask[]> {
    const token = await this.authenticate()
    const allTasks: ZenTaoTask[] = []
    let page = 1
    let totalPages = 1

    while (page <= totalPages) {
      const res = await this.http.get(`/api.php/v1/executions/${executionId}/tasks`, {
        headers: { Token: token },
        params: { recPerPage: 100, page },
      })

      const tasks = res.data.tasks || []
      allTasks.push(...tasks)

      if (page === 1) {
        totalPages = Math.ceil((res.data.total || 0) / (res.data.limit || 100))
      }
      page++
    }

    return allTasks
  }

  async getMyTasks(): Promise<ZenTaoTask[]> {
    console.log('[ZenTaoTokenProvider] 获取我的任务...')

    const executions = await this.getAllExecutions()
    const taskMap = new Map<number, ZenTaoTask>()

    for (const exec of executions) {
      const tasks = await this.getTasksByExecution(exec.id)

      for (const t of tasks) {
        const assignedTo = typeof t.assignedTo === 'string' ? t.assignedTo : t.assignedTo?.account
        if (assignedTo === ACCOUNT && t.status !== 'closed') {
          const existingTask = taskMap.get(t.id)
          if (!existingTask) {
            taskMap.set(t.id, {
              id: t.id,
              name: t.name,
              status: t.status,
              pri: t.pri,
              deadline: t.deadline,
              assignedTo: typeof t.assignedTo === 'string' ? t.assignedTo : t.assignedTo?.account || '',
              execution: exec.name,
            })
          }
        }
      }
    }

    const myTasks = Array.from(taskMap.values())
    console.log(`[ZenTaoTokenProvider] 找到 ${myTasks.length} 个我的任务（已过滤 closed，已去重）`)
    return myTasks
  }

  async getMyTasksCount(): Promise<number> {
    const tasks = await this.getMyTasks()
    return tasks.length
  }
}

export const zentaoTokenProvider = new ZenTaoTokenProvider()
