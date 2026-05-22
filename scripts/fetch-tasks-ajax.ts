import axios from 'axios'
import { wrapper } from 'axios-cookiejar-support'
import { CookieJar } from 'tough-cookie'
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
  project?: string
}

async function fetchTasks() {
  console.log('=====================================')
  console.log('  AJAX 方式获取禅道任务')
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
      'X-Requested-With': 'XMLHttpRequest',
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

    // 2. 尝试 AJAX 接口获取任务
    // 禅道 zin 框架通常使用类似这样的接口
    const ajaxUrls = [
      '/index.php?m=my&f=task&t=html&orderBy=id_desc&recPerPage=100&pageID=1',
      '/index.php?m=my&f=work&task=task&t=html&orderBy=id_desc&recPerPage=100&pageID=1',
      '/my-task-ajax.html',
      '/my-task.json',
    ]

    for (const url of ajaxUrls) {
      console.log(`【步骤 2】尝试接口: ${url}`)
      try {
        const res = await http.get(url, {
          headers: { 'Token': token },
        })
        console.log('状态:', res.status)
        console.log('内容类型:', res.headers['content-type'])
        console.log('数据长度:', res.data.length || JSON.stringify(res.data).length)

        // 如果是 JSON，尝试解析
        if (typeof res.data === 'object') {
          console.log('✅ 找到 JSON 接口!')
          console.log('数据结构:', Object.keys(res.data))
          if (res.data.tasks || res.data.data) {
            const tasks = res.data.tasks || res.data.data
            console.log(`\n找到 ${tasks.length} 个任务`)
            tasks.slice(0, 5).forEach((t: any, i: number) => {
              console.log(`${i + 1}. [${t.status}] ${t.name}`)
            })
            break
          }
        }

        // 如果是 HTML，检查是否包含任务数据
        if (typeof res.data === 'string' && res.data.includes('task-view-')) {
          console.log('✅ 找到包含任务的 HTML')
          console.log('内容片段:', res.data.substring(0, 500))
        }
      } catch (e: any) {
        console.log('❌ 失败:', e.response?.status || e.message)
      }
      console.log()
    }

    // 3. 尝试直接请求 API 获取所有任务（不分页）
    console.log('【步骤 3】尝试直接 API 获取...')

    // 先获取所有执行
    const execRes = await http.get('/api.php/v1/executions', {
      headers: { 'Token': token },
      params: { status: 'all', recPerPage: 500 },
    })

    const executions = execRes.data.executions || []
    console.log(`找到 ${executions.length} 个执行`)

    // 遍历所有执行获取任务
    const allTasks: ZenTaoTask[] = []
    for (const exec of executions) {
      try {
        const taskRes = await http.get(`/api.php/v1/executions/${exec.id}/tasks`, {
          headers: { 'Token': token },
          params: { recPerPage: 500 },
        })

        const tasks = taskRes.data.tasks || []
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
      } catch (e) {
        // ignore
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
