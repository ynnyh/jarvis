import { http, tokenManager, ACCOUNT, PASSWORD } from '../src/providers/zentao/request.js'

async function test() {
  const tokenRes = await http.post('/api.php/v1/tokens', { account: ACCOUNT, password: PASSWORD })
  const token = tokenRes.data.token
  tokenManager.setToken(token)

  // 1. 获取所有执行（正确处理分页）
  const allExecutions: any[] = []
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
      console.log(`总执行数: ${res.data.total}, 每页: ${res.data.limit}, 总页数: ${totalPages}`)
    }
    page++
  }

  console.log(`实际获取到 ${allExecutions.length} 个执行\n`)

  // 2. 遍历所有执行，找指派给我的任务
  const myTasks: any[] = []

  for (const exec of allExecutions) {
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
        const assigned = typeof t.assignedTo === 'object' ? t.assignedTo?.account : t.assignedTo
        if (assigned === 'REDACTED_ACCOUNT' && t.status !== 'closed') {
          myTasks.push({
            ...t,
            executionName: exec.name,
          })
        }
      }

      taskPage++
    }
  }

  console.log(`=== 找到 ${myTasks.length} 个指派给我的任务 ===`)
  myTasks.forEach((t, i) => {
    console.log(`\n${i + 1}. [${t.status}] ${t.name}`)
    console.log(`   执行: ${t.executionName}`)
    console.log(`   优先级: ${t.pri} | 截止: ${t.deadline || '无'}`)
  })
}

test().catch(console.error)
