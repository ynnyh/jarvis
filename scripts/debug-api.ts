import axios from 'axios'
import { wrapper } from 'axios-cookiejar-support'
import { CookieJar } from 'tough-cookie'
import dotenv from 'dotenv'

dotenv.config()

const BASE_URL = process.env.ZENTAO_BASE_URL || ''
const ACCOUNT = process.env.ZENTAO_ACCOUNT || ''
const PASSWORD = process.env.ZENTAO_PASSWORD || ''

async function debug() {
  const jar = new CookieJar()
  const http = wrapper(axios.create({
    baseURL: BASE_URL,
    timeout: 30000,
    jar: jar,
    withCredentials: true,
  }))

  // 获取 Token
  const tokenRes = await http.post('/api.php/v1/tokens', {
    account: ACCOUNT,
    password: PASSWORD,
  })
  const token = tokenRes.data.token
  console.log('Token:', token.slice(0, 20) + '...')

  // 获取执行列表
  console.log('\n=== 获取执行列表 ===')
  const execRes = await http.get('/api.php/v1/executions', {
    headers: { 'Token': token },
    params: { status: 'all', recPerPage: 500 },
  })

  console.log('响应结构:', Object.keys(execRes.data))
  console.log('page:', execRes.data.page)
  console.log('total:', execRes.data.total)
  console.log('limit:', execRes.data.limit)
  console.log('executions 数量:', execRes.data.executions?.length)

  // 获取第一个执行的任务
  if (execRes.data.executions?.length > 0) {
    const firstExec = execRes.data.executions[0]
    console.log(`\n=== 获取执行 ${firstExec.id} 的任务 ===`)

    const taskRes = await http.get(`/api.php/v1/executions/${firstExec.id}/tasks`, {
      headers: { 'Token': token },
      params: { recPerPage: 500 },
    })

    console.log('响应结构:', Object.keys(taskRes.data))
    console.log('page:', taskRes.data.page)
    console.log('total:', taskRes.data.total)
    console.log('limit:', taskRes.data.limit)
    console.log('tasks 数量:', taskRes.data.tasks?.length)

    if (taskRes.data.tasks?.length > 0) {
      console.log('\n前 3 个任务的 assignedTo:')
      taskRes.data.tasks.slice(0, 3).forEach((t: any, i: number) => {
        console.log(`  ${i + 1}. ID=${t.id}, assignedTo=${JSON.stringify(t.assignedTo)}`)
      })
    }
  }

  // 遍历所有执行查找我的任务
  console.log('\n=== 遍历所有执行查找我的任务 ===')
  let myTasksCount = 0
  let totalTasksChecked = 0

  for (const exec of execRes.data.executions || []) {
    try {
      const taskRes = await http.get(`/api.php/v1/executions/${exec.id}/tasks`, {
        headers: { 'Token': token },
        params: { recPerPage: 500 },
      })

      const tasks = taskRes.data.tasks || []
      totalTasksChecked += tasks.length

      for (const t of tasks) {
        const assignee = t.assignedTo?.account || t.assignedTo
        if (assignee === 'REDACTED_ACCOUNT' && t.status !== 'closed') {
          myTasksCount++
          console.log(`找到任务: [${t.status}] ${t.name} (执行: ${exec.name})`)
        }
      }
    } catch (e) {
      // ignore
    }
  }

  console.log(`\n总检查任务数: ${totalTasksChecked}`)
  console.log(`我的任务数: ${myTasksCount}`)
}

debug().catch(console.error)
