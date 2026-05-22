import { http, tokenManager, ACCOUNT, PASSWORD } from '../src/providers/zentao/request.js'

async function test() {
  const tokenRes = await http.post('/api.php/v1/tokens', { account: ACCOUNT, password: PASSWORD })
  const token = tokenRes.data.token
  tokenManager.setToken(token)
  console.log('Token OK\n')

  // 1. 获取所有执行
  const execRes = await http.get('/api.php/v1/executions', { params: { recPerPage: 100 } })
  const executions = execRes.data.executions || []
  console.log(`=== 共 ${executions.length} 个执行 ===\n`)

  // 2. 遍历所有执行，获取所有任务（不传status参数），然后过滤指派给我的
  const myTasks: any[] = []
  const allAssignees = new Map<string, number>()

  for (const exec of executions) {
    try {
      // 不传status参数，获取所有任务
      const taskRes = await http.get(`/api.php/v1/executions/${exec.id}/tasks`, {
        params: { recPerPage: 100 },
      })
      const tasks = taskRes.data.tasks || []

      if (tasks.length > 0) {
        // 统计被指派人
        for (const t of tasks) {
          const assigned = typeof t.assignedTo === 'object' ? t.assignedTo?.account : t.assignedTo
          if (assigned) {
            allAssignees.set(assigned, (allAssignees.get(assigned) || 0) + 1)
          }

          // 收集指派给我的
          if (assigned === 'REDACTED_ACCOUNT') {
            myTasks.push({
              ...t,
              executionName: exec.name,
            })
          }
        }
      }
    } catch (err: any) {
      // 忽略错误
    }
  }

  console.log(`=== 指派给我的任务: ${myTasks.length} 个 ===`)
  myTasks.forEach((t, i) => {
    console.log(`${i + 1}. [${t.status}] ${t.name}`)
    console.log(`   执行: ${t.executionName}`)
    console.log(`   优先级: ${t.pri} | 截止: ${t.deadline || '无'}`)
    console.log()
  })

  console.log('\n=== 所有被指派人统计（前20）===')
  const sorted = Array.from(allAssignees.entries()).sort((a, b) => b[1] - a[1])
  sorted.slice(0, 20).forEach(([name, count]) => {
    console.log(`  ${name}: ${count} 个任务`)
  })
}

test().catch(console.error)
