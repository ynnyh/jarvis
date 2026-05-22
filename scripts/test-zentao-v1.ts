import { http, tokenManager, ACCOUNT, PASSWORD } from '../src/providers/zentao/request.js'

async function test() {
  const tokenRes = await http.post('/api.php/v1/tokens', { account: ACCOUNT, password: PASSWORD })
  tokenManager.setToken(tokenRes.data.token)
  console.log('Token OK\n')

  // 获取前3个执行的任务，看看 assignedTo 的结构
  const execRes = await http.get('/api.php/v1/executions')
  const executions = execRes.data.executions || execRes.data

  const assignees = new Set<string>()
  let totalTasks = 0

  for (const exec of executions.slice(0, 5)) {
    const taskRes = await http.get(`/api.php/v1/executions/${exec.id}/tasks`)
    const tasks = taskRes.data.tasks || []
    totalTasks += tasks.length

    console.log(`\n📁 执行: ${exec.name} (${tasks.length} 个任务)`)
    tasks.slice(0, 3).forEach((t: any) => {
      const assignedTo = typeof t.assignedTo === 'object'
        ? `${t.assignedTo?.realname}(${t.assignedTo?.account})`
        : t.assignedTo
      assignees.add(assignedTo)
      console.log(`   📌 [${t.status}] ${t.name} -> ${assignedTo}`)
    })
    if (tasks.length > 3) console.log(`   ... 还有 ${tasks.length - 3} 个任务`)
  }

  console.log(`\n=== 总结 ===`)
  console.log(`总任务数: ${totalTasks}`)
  console.log(`所有被指派人: ${Array.from(assignees).join(', ')}`)
}

test().catch(console.error)
