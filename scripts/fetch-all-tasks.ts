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
  console.log('  获取所有禅道任务')
  console.log('=====================================')
  console.log()

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
  console.log('✅ Token 获取成功')
  console.log()

  // 获取所有执行（分页）
  console.log('【步骤 1】获取所有执行...')
  const allExecutions: any[] = []
  let execPage = 1
  let execTotalPages = 1

  while (execPage <= execTotalPages) {
    const execRes = await http.get('/api.php/v1/executions', {
      headers: { 'Token': token },
      params: { status: 'all', recPerPage: 100, page: execPage },
    })

    const executions = execRes.data.executions || []
    allExecutions.push(...executions)

    if (execPage === 1) {
      execTotalPages = Math.ceil((execRes.data.total || 0) / (execRes.data.limit || 20))
      console.log(`总执行数: ${execRes.data.total}, 每页: ${execRes.data.limit}, 总页数: ${execTotalPages}`)
    }

    console.log(`  第 ${execPage}/${execTotalPages} 页: +${executions.length} 个执行`)
    execPage++
  }

  console.log(`✅ 共获取 ${allExecutions.length} 个执行`)
  console.log()

  // 遍历所有执行获取任务
  console.log('【步骤 2】遍历所有执行获取任务...')
  const allTasks: ZenTaoTask[] = []
  let totalTasksChecked = 0

  for (let i = 0; i < allExecutions.length; i++) {
    const exec = allExecutions[i]
    let taskPage = 1
    let taskTotalPages = 1
    let execTaskCount = 0

    while (taskPage <= taskTotalPages) {
      try {
        const taskRes = await http.get(`/api.php/v1/executions/${exec.id}/tasks`, {
          headers: { 'Token': token },
          params: { recPerPage: 100, page: taskPage },
        })

        const tasks = taskRes.data.tasks || []
        execTaskCount += tasks.length
        totalTasksChecked += tasks.length

        if (taskPage === 1) {
          taskTotalPages = Math.ceil((taskRes.data.total || 0) / (taskRes.data.limit || 100))
        }

        // 筛选我的任务
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

        taskPage++
      } catch (e) {
        break
      }
    }

    if (execTaskCount > 0) {
      console.log(`  执行 ${exec.id}: ${execTaskCount} 个任务`)
    }

    // 每 20 个执行显示一次进度
    if ((i + 1) % 20 === 0) {
      console.log(`  ... 已处理 ${i + 1}/${allExecutions.length} 个执行，找到 ${allTasks.length} 个我的任务`)
    }
  }

  console.log()
  console.log('=====================================')
  console.log(`  结果统计`)
  console.log('=====================================')
  console.log(`总检查任务数: ${totalTasksChecked}`)
  console.log(`我的任务数: ${allTasks.length}`)
  console.log()

  if (allTasks.length > 0) {
    console.log('任务列表:')
    allTasks.forEach((t, i) => {
      console.log(`${i + 1}. [${t.status}] ${t.name}`)
      console.log(`   执行: ${t.execution} | 截止: ${t.deadline || '无'}`)
    })
  }
}

fetchTasks().catch(console.error)
