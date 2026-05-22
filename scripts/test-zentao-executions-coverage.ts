import { http, tokenManager, ACCOUNT, PASSWORD } from '../src/providers/zentao/request.js'

async function test() {
  const tokenRes = await http.post('/api.php/v1/tokens', { account: ACCOUNT, password: PASSWORD })
  const token = tokenRes.data.token
  tokenManager.setToken(token)
  console.log('Token OK\n')

  // 1. 测试不同 status 参数的 executions
  console.log('=====================================')
  console.log('【1】测试 executions 不同 status 参数')
  console.log('=====================================\n')

  const statusTests = [
    { name: '无参数（默认）', params: {} },
    { name: 'status=all', params: { status: 'all' } },
    { name: 'status=active', params: { status: 'active' } },
    { name: 'status=wait', params: { status: 'wait' } },
    { name: 'status=doing', params: { status: 'doing' } },
    { name: 'status=suspended', params: { status: 'suspended' } },
    { name: 'status=closed', params: { status: 'closed' } },
    { name: 'status=undeleted', params: { status: 'undeleted' } },
  ]

  for (const t of statusTests) {
    const res = await http.get('/api.php/v1/executions', {
      params: { ...t.params, recPerPage: 100, page: 1 },
    })
    const executions = res.data.executions || []
    console.log(`${t.name}: ${executions.length} 个（total: ${res.data.total}, limit: ${res.data.limit}）`)
  }
  console.log()

  // 2. 获取所有 execution（不分页，取全部）
  console.log('=====================================')
  console.log('【2】获取所有 execution（完整分页）')
  console.log('=====================================\n')

  let allExecutions: any[] = []
  let page = 1
  let totalPages = 1

  while (page <= totalPages) {
    const res = await http.get('/api.php/v1/executions', {
      params: { status: 'all', recPerPage: 100, page },
    })
    const executions = res.data.executions || []
    allExecutions.push(...executions)

    if (page === 1) {
      totalPages = Math.ceil((res.data.total || 0) / (res.data.limit || 20))
      console.log(`总 execution 数: ${res.data.total}, 每页: ${res.data.limit}, 总页数: ${totalPages}`)
    }
    page++
  }

  console.log(`实际获取到: ${allExecutions.length} 个 execution`)
  console.log(`状态分布: ${allExecutions.reduce((acc: any, e: any) => {
    acc[e.status] = (acc[e.status] || 0) + 1
    return acc
  }, {})}`)
  console.log()

  // 3. 遍历每个 execution，统计任务
  console.log('=====================================')
  console.log('【3】遍历每个 execution 统计任务')
  console.log('=====================================\n')

  let totalTasks = 0
  let totalMyTasks = 0
  const myTaskDetails: any[] = []

  for (const exec of allExecutions) {
    let taskPage = 1
    let taskTotalPages = 1
    let execTasks = 0
    let execMyTasks = 0

    while (taskPage <= taskTotalPages) {
      const taskRes = await http.get(`/api.php/v1/executions/${exec.id}/tasks`, {
        params: { recPerPage: 100, page: taskPage },
      })
      const tasks = taskRes.data.tasks || []

      if (taskPage === 1) {
        taskTotalPages = Math.ceil((taskRes.data.total || 0) / (taskRes.data.limit || 100))
      }

      for (const t of tasks) {
        execTasks++
        const assigned = typeof t.assignedTo === 'object' ? t.assignedTo?.account : t.assignedTo
        if (assigned === 'REDACTED_ACCOUNT') {
          execMyTasks++
          myTaskDetails.push({
            taskId: t.id,
            taskName: t.name,
            status: t.status,
            executionId: exec.id,
            executionName: exec.name,
            executionStatus: exec.status,
          })
        }
      }

      taskPage++
    }

    totalTasks += execTasks
    totalMyTasks += execMyTasks

    if (execMyTasks > 0) {
      console.log(`📌 [${exec.status}] ${exec.name} (ID:${exec.id})`)
      console.log(`   总任务: ${execTasks}, 我的任务: ${execMyTasks}`)
    }
  }

  console.log()
  console.log('=====================================')
  console.log('【4】最终统计')
  console.log('=====================================\n')
  console.log(`execution 总数: ${allExecutions.length}`)
  console.log(`任务总数: ${totalTasks}`)
  console.log(`指派给我的任务: ${totalMyTasks}`)
  console.log()

  if (myTaskDetails.length > 0) {
    console.log('我的任务明细:')
    myTaskDetails.forEach((t, i) => {
      console.log(`  ${i + 1}. [${t.status}] ${t.taskName}`)
      console.log(`     执行: ${t.executionName} (${t.executionStatus})`)
    })
  }
}

test().catch(console.error)
