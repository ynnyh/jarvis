import axios from 'axios'
import { wrapper } from 'axios-cookiejar-support'
import { CookieJar } from 'tough-cookie'
import * as cheerio from 'cheerio'
import dotenv from 'dotenv'

dotenv.config()

const BASE_URL = process.env.ZENTAO_BASE_URL || ''
const ACCOUNT = process.env.ZENTAO_ACCOUNT || ''
const PASSWORD = process.env.ZENTAO_PASSWORD || ''

interface ZenTaoTask {
  id: number
  name: string
  status: string
  pri: number
  deadline?: string
  assignedTo?: string
  execution?: string
}

async function fetchTasks() {
  console.log('=====================================')
  console.log('  获取禅道真实任务数据')
  console.log('=====================================')
  console.log()

  const jar = new CookieJar()
  const http = wrapper(axios.create({
    baseURL: BASE_URL,
    timeout: 30000,
    jar: jar,
    withCredentials: true,
    headers: {
      'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36',
    },
  }))

  try {
    // 1. 获取 Token
    console.log('【步骤 1】获取 Token...')
    const tokenRes = await http.post('/api.php/v1/tokens', {
      account: ACCOUNT,
      password: PASSWORD,
    })
    const token = tokenRes.data.token
    console.log('✅ Token 获取成功')
    console.log()

    // 2. 尝试多个 URL 获取任务页面
    const urls = [
      '/my-work-task-assignedTo.html',
      '/my/task-assignedTo.html',
      '/my-task.html',
      '/my/task.html',
    ]

    for (const url of urls) {
      console.log(`【步骤 2】尝试: ${url}`)
      try {
        const res = await http.get(url, {
          headers: { 'Token': token },
        })
        console.log('状态:', res.status)
        console.log('路径:', res.request?.path)
        console.log('长度:', res.data.length)

        if (typeof res.data === 'string' && res.data.includes('task-view-')) {
          console.log('✅ 找到包含任务的页面!')

          // 解析任务
          const $ = cheerio.load(res.data)
          const tasks: ZenTaoTask[] = []

          $('a[href*="/task-view-"]').each((i, el) => {
            const $link = $(el)
            const href = $link.attr('href') || ''
            const idMatch = href.match(/task-view-(\d+)/)
            const id = idMatch ? parseInt(idMatch[1]) : 0
            const name = $link.text().trim()

            if (id && name) {
              const $row = $link.closest('tr')
              const status = $row.find('.status, td:nth-child(3)').text().trim()
              const pri = parseInt($row.find('.pri, td:nth-child(4)').text().trim()) || 0
              const deadline = $row.find('.deadline, td:nth-child(5)').text().trim() || undefined

              tasks.push({ id, name, status, pri, deadline })
            }
          })

          console.log(`\n找到 ${tasks.length} 个任务`)
          if (tasks.length > 0) {
            console.log('\n任务列表:')
            tasks.slice(0, 10).forEach((t, i) => {
              console.log(`${i + 1}. [${t.status}] ${t.name}`)
              console.log(`   ID: ${t.id} | 优先级: ${t.pri} | 截止: ${t.deadline || '无'}`)
            })
            return
          }
        }
      } catch (e: any) {
        console.log('❌ 失败:', e.response?.status || e.message)
      }
      console.log()
    }

    // 3. 如果上面的都失败，尝试用 API 获取所有执行的任务
    console.log('【步骤 3】使用 API 获取所有任务...')
    const execRes = await http.get('/api.php/v1/executions', {
      headers: { 'Token': token },
      params: { status: 'all', recPerPage: 500 },
    })

    const executions = execRes.data.executions || []
    console.log(`找到 ${executions.length} 个执行`)

    const allTasks: ZenTaoTask[] = []
    for (const exec of executions) {
      let page = 1
      let totalPages = 1

      while (page <= totalPages) {
        try {
          const taskRes = await http.get(`/api.php/v1/executions/${exec.id}/tasks`, {
            headers: { 'Token': token },
            params: { recPerPage: 100, page },
          })

          const tasks = taskRes.data.tasks || []
          totalPages = Math.ceil((taskRes.data.total || 0) / (taskRes.data.limit || 100))

          for (const t of tasks) {
            const assignee = t.assignedTo?.account || t.assignedTo
            if (assignee === ACCOUNT && t.status !== 'closed') {
              allTasks.push({
                id: t.id,
                name: t.name,
                status: t.status,
                pri: t.pri,
                deadline: t.deadline,
                assignedTo: assignee,
                execution: exec.name,
              })
            }
          }

          page++
        } catch (e) {
          break
        }
      }
    }

    console.log(`\n✅ 总共找到 ${allTasks.length} 个指派给 ${ACCOUNT} 的任务`)

    if (allTasks.length > 0) {
      console.log('\n任务列表:')
      allTasks.forEach((t, i) => {
        console.log(`${i + 1}. [${t.status}] ${t.name}`)
        console.log(`   执行: ${t.execution} | 截止: ${t.deadline || '无'}`)
      })
    }

  } catch (error: any) {
    console.error('\n❌ 错误:', error.message)
  }
}

fetchTasks()
