import { http, tokenManager, ACCOUNT, PASSWORD } from '../src/providers/zentao/request.js'

async function test() {
  const tokenRes = await http.post('/api.php/v1/tokens', { account: ACCOUNT, password: PASSWORD })
  const token = tokenRes.data.token
  tokenManager.setToken(token)
  console.log('Token OK\n')

  // 1. 获取项目列表
  console.log('=== 获取项目列表 ===')
  const projectRes = await http.get('/api.php/v1/projects', {
    params: { page: 1, limit: 100 },
  })
  const projects = projectRes.data.projects || []
  console.log(`获取到 ${projects.length} 个项目\n`)

  // 2. 遍历每个项目，获取执行，再获取任务
  const myTasks: any[] = []
  let totalExecutions = 0

  for (const project of projects) {
    // 获取项目下的执行
    const execRes = await http.get(`/api.php/v1/projects/${project.id}/executions`, {
      params: { page: 1, limit: 100 },
    })
    const executions = execRes.data.executions || []
    totalExecutions += executions.length

    for (const exec of executions) {
      if (!exec.id) {
        console.log(`  ⚠️ 项目 ${project.name} 的执行没有ID:`, JSON.stringify(exec).slice(0, 100))
        continue
      }

      // 获取执行下的任务
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
              projectName: project.name,
              executionName: exec.name,
            })
          }
        }

        taskPage++
      }
    }
  }

  console.log(`遍历了 ${projects.length} 个项目, ${totalExecutions} 个执行`)
  console.log(`=== 共找到 ${myTasks.length} 个指派给我的任务 ===`)
  myTasks.forEach((t, i) => {
    console.log(`\n${i + 1}. [${t.status}] ${t.name}`)
    console.log(`   项目: ${t.projectName}`)
    console.log(`   执行: ${t.executionName}`)
    console.log(`   优先级: ${t.pri} | 截止: ${t.deadline || '无'}`)
  })
}

test().catch(console.error)
