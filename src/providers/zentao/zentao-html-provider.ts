import axios, { AxiosInstance } from 'axios'
import { wrapper } from 'axios-cookiejar-support'
import { CookieJar } from 'tough-cookie'
import * as cheerio from 'cheerio'
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
  assignedTo?: string
  execution?: string
  project?: string
}

export class ZenTaoHtmlProvider {
  private jar: CookieJar
  private http: AxiosInstance
  private authenticated = false

  constructor() {
    this.jar = new CookieJar()
    this.http = wrapper(axios.create({
      baseURL: BASE_URL,
      timeout: 30000,
      jar: this.jar,
      withCredentials: true,
      headers: {
        'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36',
      },
    }))
  }

  async login(): Promise<boolean> {
    console.log('[ZenTaoHtmlProvider] 开始登录...')

    try {
      // 1. 先访问登录页获取 session
      await this.http.get('/user-login.html')
      console.log('[ZenTaoHtmlProvider] 已访问登录页')

      // 2. 提交登录表单
      const loginRes = await this.http.post('/user-login.html', {
        account: ACCOUNT,
        password: PASSWORD,
        keepLogin: 'on',
      }, {
        headers: {
          'Content-Type': 'application/x-www-form-urlencoded',
        },
      })

      // 3. 检查登录结果
      if (loginRes.data.includes('登录失败') || loginRes.data.includes('密码错误')) {
        console.error('[ZenTaoHtmlProvider] 登录失败：账号或密码错误')
        return false
      }

      // 4. 检查是否有跳转或登录成功标识
      if (loginRes.data.includes('我的地盘') || loginRes.data.includes('工作台') || loginRes.request?.path?.includes('my')) {
        this.authenticated = true
        console.log('[ZenTaoHtmlProvider] 登录成功')
        return true
      }

      // 5. 尝试访问工作台确认登录状态
      const myRes = await this.http.get('/my/')
      if (myRes.data.includes(ACCOUNT) || myRes.data.includes('我的地盘')) {
        this.authenticated = true
        console.log('[ZenTaoHtmlProvider] 登录成功（通过工作台验证）')
        return true
      }

      console.error('[ZenTaoHtmlProvider] 登录状态未知')
      return false
    } catch (error: any) {
      console.error('[ZenTaoHtmlProvider] 登录失败:', error.message)
      return false
    }
  }

  async getMyTasksFromWorkBench(): Promise<ZenTaoTask[]> {
    if (!this.authenticated) {
      const loggedIn = await this.login()
      if (!loggedIn) {
        throw new Error('登录失败')
      }
    }

    console.log('[ZenTaoHtmlProvider] 获取工作台任务...')

    try {
      // 请求工作台任务页面
      const response = await this.http.get('/my-work-task-assignedTo.html')
      const html = response.data

      console.log('[ZenTaoHtmlProvider] 页面获取成功，开始解析...')

      // 使用 cheerio 解析 HTML
      const $ = cheerio.load(html)
      const tasks: ZenTaoTask[] = []

      // 查找任务表格
      // 禅道工作台任务通常在 table 中，class 可能包含 datatable 或类似
      const taskTable = $('table.datatable, table.table, #taskList').first()

      if (taskTable.length === 0) {
        console.log('[ZenTaoHtmlProvider] 未找到任务表格，尝试其他选择器...')
      }

      // 遍历表格行
      taskTable.find('tbody tr, tr').each((index, element) => {
        const $row = $(element)

        // 提取任务ID（通常在链接中）
        const idLink = $row.find('a[href*="/task-view-"], a[href*="/task-"]').first()
        const idMatch = idLink.attr('href')?.match(/task-view-(\d+)/) || idLink.attr('href')?.match(/task-(\d+)/)
        const id = idMatch ? parseInt(idMatch[1]) : 0

        // 提取任务名称
        const name = idLink.text().trim() || $row.find('td:nth-child(2)').text().trim()

        // 提取状态
        const statusText = $row.find('span.status, .status, td:nth-child(3)').text().trim()
        const status = this.parseStatus(statusText)

        // 提取优先级
        const priText = $row.find('td:nth-child(4), .pri').text().trim()
        const pri = parseInt(priText) || 0

        // 提取截止日期
        const deadline = $row.find('td:nth-child(5), .deadline').text().trim() || undefined

        // 提取执行/项目
        const execution = $row.find('td:nth-child(6)').text().trim()

        if (id && name) {
          tasks.push({
            id,
            name,
            status,
            pri,
            deadline: deadline || undefined,
            assignedTo: ACCOUNT,
            execution: execution || undefined,
          })
        }
      })

      console.log(`[ZenTaoHtmlProvider] 解析到 ${tasks.length} 个任务`)
      return tasks
    } catch (error: any) {
      console.error('[ZenTaoHtmlProvider] 获取任务失败:', error.message)
      throw error
    }
  }

  private parseStatus(statusText: string): string {
    const statusMap: Record<string, string> = {
      '未开始': 'wait',
      '进行中': 'doing',
      '已完成': 'done',
      '已关闭': 'closed',
      '已取消': 'cancel',
    }
    return statusMap[statusText] || statusText.toLowerCase()
  }

  isAuthenticated(): boolean {
    return this.authenticated
  }

  async logout(): Promise<void> {
    try {
      await this.http.get('/user-logout.html')
      this.authenticated = false
      console.log('[ZenTaoHtmlProvider] 已登出')
    } catch (error: any) {
      console.error('[ZenTaoHtmlProvider] 登出失败:', error.message)
    }
  }
}

export const zentaoHtmlProvider = new ZenTaoHtmlProvider()
