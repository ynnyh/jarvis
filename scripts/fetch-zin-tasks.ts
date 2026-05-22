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
}

async function fetchTasks() {
  console.log('=====================================')
  console.log('  获取禅道 zin 框架任务')
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

    // 2. 尝试 zin 框架的 AJAX 接口
    // 禅道 zin 框架使用特殊的 URL 格式
    const ajaxUrls = [
      // 标准 zin 接口格式
      '/index.php?m=my&f=work&task=task&t=html&orderBy=id_desc&recPerPage=100&pageID=1',
      '/index.php?m=my&f=task&t=html&orderBy=id_desc&recPerPage=100&pageID=1',
      // 尝试获取 JSON 数据
      '/index.php?m=my&f=work&task=task&t=json&orderBy=id_desc&recPerPage=100&pageID=1',
      // 尝试不同的 module/method
      '/my-ajaxGetTasks.html',
      '/my-getTasks.html',
    ]

    for (const url of ajaxUrls) {
      console.log(`【步骤 2】尝试: ${url}`)
      try {
        const res = await http.get(url, {
          headers: {
            'Token': token,
            'Accept': 'application/json, text/html',
          },
        })
        console.log('状态:', res.status)
        console.log('内容类型:', res.headers['content-type'])

        if (typeof res.data === 'object') {
          console.log('✅ JSON 响应!')
          console.log('数据结构:', Object.keys(res.data))

          if (res.data.tasks || res.data.data || res.data.list) {
            const tasks = res.data.tasks || res.data.data || res.data.list
            console.log(`找到 ${tasks.length} 个任务`)
            tasks.slice(0, 5).forEach((t: any, i: number) => {
              console.log(`${i + 1}. [${t.status}] ${t.name}`)
            })
            return
          }
        } else if (typeof res.data === 'string') {
          // 检查是否是 zin 指令
          if (res.data.includes('load') && res.data.length < 1000) {
            console.log('zin 指令:', res.data)
          } else if (res.data.includes('task-view-')) {
            console.log('✅ HTML 包含任务链接')
            console.log('长度:', res.data.length)
          }
        }
      } catch (e: any) {
        console.log('❌ 失败:', e.response?.status || e.message)
      }
      console.log()
    }

    // 3. 尝试通过 API 获取所有任务（遍历所有执行）
    console.log('【步骤 3】通过 API 获取所有任务...')

    // 获取所有执行
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
