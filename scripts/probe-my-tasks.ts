import axios from 'axios'
import dotenv from 'dotenv'

dotenv.config()

const BASE_URL = process.env.ZENTAO_BASE_URL || ''
const ACCOUNT = process.env.ZENTAO_ACCOUNT || ''
const PASSWORD = process.env.ZENTAO_PASSWORD || ''

async function probe() {
  // 1. 获取 Token
  const tokenRes = await axios.post(`${BASE_URL}/api.php/v1/tokens`, {
    account: ACCOUNT,
    password: PASSWORD,
  })
  const token = tokenRes.data.token
  console.log('✅ Token 获取成功')

  // 2. 获取所有执行（不限状态）
  console.log('\n--- 获取所有执行 ---')
  const execRes = await axios.get(`${BASE_URL}/api.php/v1/executions`, {
    headers: { 'Token': token },
    params: { status: 'all', recPerPage: 200 },
  })
  const executions = execRes.data.executions || []
  console.log(`总执行数: ${executions.length}`)

  // 3. 遍历所有执行获取任务，查找指派给 REDACTED_ACCOUNT 的
  console.log('\n--- 遍历所有执行查找我的任务 ---')
  let totalTasks = 0
  let myTasksCount = 0
  const myTasks: any[] = []

  for (const exec of executions) {
    try {
      const taskRes = await axios.get(`${BASE_URL}/api.php/v1/executions/${exec.id}/tasks`, {
        headers: { 'Token': token },
        params: { recPerPage: 200 },
      })
      const tasks = taskRes.data.tasks || []
      totalTasks += tasks.length

      for (const t of tasks) {
        // 检查 assignedTo 的各种可能格式
        const assignedToStr = typeof t.assignedTo === 'string' ? t.assignedTo : JSON.stringify(t.assignedTo)
        const assignedToAccount = t.assignedTo?.account || t.assignedTo

        if (assignedToAccount === 'REDACTED_ACCOUNT' || assignedToStr.includes('REDACTED_ACCOUNT')) {
          myTasksCount++
          myTasks.push({
            id: t.id,
            name: t.name,
            status: t.status,
            execution: exec.name,
            assignedTo: t.assignedTo,
          })
        }
      }
    } catch (e) {
      // ignore
    }
  }

  console.log(`总任务数: ${totalTasks}`)
  console.log(`指派给 REDACTED_ACCOUNT 的任务: ${myTasksCount}`)

  if (myTasks.length > 0) {
    console.log('\n前 10 个我的任务:')
    myTasks.slice(0, 10).forEach((t, i) => {
      console.log(`  ${i + 1}. [${t.status}] ${t.name} (执行: ${t.execution})`)
    })
  }

  // 4. 打印一个任务的完整 assignedTo 字段结构
  console.log('\n--- 检查任务字段结构 ---')
  for (const exec of executions.slice(0, 5)) {
    try {
      const taskRes = await axios.get(`${BASE_URL}/api.php/v1/executions/${exec.id}/tasks`, {
        headers: { 'Token': token },
        params: { recPerPage: 5 },
      })
      const tasks = taskRes.data.tasks || []
      if (tasks.length > 0) {
        console.log('\n第一个任务的 assignedTo 字段:')
        console.log(JSON.stringify(tasks[0].assignedTo, null, 2))
        console.log('\n第一个任务的完整结构:')
        console.log(JSON.stringify(tasks[0], null, 2).slice(0, 1500))
        break
      }
    } catch (e) {
      // ignore
    }
  }

  // 5. 尝试用 /my-tasks 或 /tasks?assignedTo= 接口
  console.log('\n--- 尝试其他接口 ---')
  try {
    const res = await axios.get(`${BASE_URL}/api.php/v1/tasks`, {
      headers: { 'Token': token },
      params: { assignedTo: 'REDACTED_ACCOUNT', recPerPage: 100 },
    })
    console.log('/tasks 接口返回:', res.data?.tasks?.length || 0)
  } catch (e: any) {
    console.log('/tasks 接口错误:', e.response?.status, e.message)
  }
}

probe().catch(console.error)
