import { http, tokenManager, ACCOUNT, PASSWORD } from '../src/providers/zentao/request.js'

async function test() {
  const tokenRes = await http.post('/api.php/v1/tokens', { account: ACCOUNT, password: PASSWORD })
  const token = tokenRes.data.token
  tokenManager.setToken(token)
  console.log('Token OK\n')

  // 1. 获取项目 289 详情
  console.log('=== 项目 289 详情 ===')
  try {
    const projectRes = await http.get('/api.php/v1/projects/289')
    console.log('项目信息:', JSON.stringify(projectRes.data, null, 2).slice(0, 500))
  } catch (err: any) {
    console.log('获取项目详情失败:', err.response?.status, err.response?.data?.message || err.message)
  }
  console.log()

  // 2. 获取项目 289 下的执行
  console.log('=== 项目 289 的执行列表 ===')
  let execPage = 1
  let execTotalPages = 1
  const projectExecutions: any[] = []

  while (execPage <= execTotalPages) {
    const execRes = await http.get('/api.php/v1/projects/289/executions', {
      params: { page: execPage, limit: 100 },
    })
    const executions = execRes.data.executions || []
    projectExecutions.push(...executions)

    if (execPage === 1) {
      execTotalPages = Math.ceil((execRes.data.total || 0) / (execRes.data.limit || 20))
      console.log(`总执行数: ${execRes.data.total}, 每页: ${execRes.data.limit}, 总页数: ${execTotalPages}`)
    }
    execPage++
  }

  console.log(`获取到 ${projectExecutions.length} 个执行:`)
  projectExecutions.forEach((e: any, i: number) => {
    console.log(`  ${i + 1}. [${e.status}] ${e.name} (ID:${e.id})`)
  })
  console.log()

  // 3. 遍历项目 289 下所有执行，获取任务
  console.log('=== 项目 289 下的所有任务 ===')
  let totalTasks = 0
  let myTasks = 0
  const myTaskList: any[] = []

  for (const exec of projectExecutions) {
    let taskPage = 1
    let taskTotalPages = 1

    while (taskPage <= taskTotalPages) {
      const taskRes = await http.get(`/api.php/v1/executions/${exec.id}/tasks`, {
        params: { recPerPage: 100, page: taskPage },
      })
      const tasks = taskRes.data.tasks || []

      if (taskPage === 1) {
        taskTotalPages = Math.ceil((taskRes.data.total || 0) / (taskRes.data.limit || 100))
      }

      for (const t of tasks) {
        totalTasks++
        const assigned = typeof t.assignedTo === 'object' ? t.assignedTo?.account : t.assignedTo
        if (assigned === 'REDACTED_ACCOUNT') {
          myTasks++
          myTaskList.push({
            taskId: t.id,
            taskName: t.name,
            status: t.status,
            executionName: exec.name,
            executionStatus: exec.status,
          })
        }
      }

      taskPage++
    }
  }

  console.log(`总任务数: ${totalTasks}`)
  console.log(`指派给我的任务: ${myTasks}`)
  console.log()

  if (myTaskList.length > 0) {
    console.log('我的任务明细:')
    myTaskList.forEach((t, i) => {
      console.log(`  ${i + 1}. [${t.status}] ${t.taskName}`)
      console.log(`     执行: ${t.executionName} (${t.executionStatus})`)
    })
  }
}

test().catch(console.error)
