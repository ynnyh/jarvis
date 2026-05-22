import { http, tokenManager, ACCOUNT, PASSWORD } from '../src/providers/zentao/request.js'

async function test() {
  const tokenRes = await http.post('/api.php/v1/tokens', { account: ACCOUNT, password: PASSWORD })
  const token = tokenRes.data.token
  tokenManager.setToken(token)
  console.log('Token OK\n')

  const allMyTasks: any[] = []
  const visitedProjects = new Set<number>()
  const visitedExecutions = new Set<number>()

  // 递归遍历项目树
  async function exploreProject(projectId: number, depth: number = 0) {
    const indent = '  '.repeat(depth)

    if (visitedProjects.has(projectId)) return
    visitedProjects.add(projectId)

    // 获取项目详情
    let projectName = `项目${projectId}`
    try {
      const projectRes = await http.get(`/api.php/v1/projects/${projectId}`)
      projectName = projectRes.data.name || projectName
    } catch { /* ignore */ }

    // 获取项目下的执行
    let execPage = 1
    let execTotalPages = 1
    while (execPage <= execTotalPages) {
      const execRes = await http.get(`/api.php/v1/projects/${projectId}/executions`, {
        params: { page: execPage, limit: 100 },
      })
      const executions = execRes.data.executions || []
      if (execPage === 1) {
        execTotalPages = Math.ceil((execRes.data.total || 0) / (execRes.data.limit || 20))
      }

      for (const exec of executions) {
        if (exec.id) await exploreExecution(exec.id, projectName, depth + 1)
      }
      execPage++
    }

    // 获取子项目
    let childPage = 1
    let childTotalPages = 1
    while (childPage <= childTotalPages) {
      const childRes = await http.get('/api.php/v1/projects', {
        params: { parent: projectId, page: childPage, limit: 100 },
      })
      const children = childRes.data.projects || []
      if (childPage === 1) {
        childTotalPages = Math.ceil((childRes.data.total || 0) / (childRes.data.limit || 20))
      }

      for (const child of children) {
        if (child.id) await exploreProject(child.id, depth + 1)
      }
      childPage++
    }
  }

  // 遍历执行下的任务
  async function exploreExecution(executionId: number, projectName: string, depth: number) {
    if (visitedExecutions.has(executionId)) return
    visitedExecutions.add(executionId)

    let taskPage = 1
    let taskTotalPages = 1

    while (taskPage <= taskTotalPages) {
      const taskRes = await http.get(`/api.php/v1/executions/${executionId}/tasks`, {
        params: { recPerPage: 100, page: taskPage },
      })
      const tasks = taskRes.data.tasks || []
      if (taskPage === 1) {
        taskTotalPages = Math.ceil((taskRes.data.total || 0) / (taskRes.data.limit || 100))
      }

      for (const t of tasks) {
        const assigned = typeof t.assignedTo === 'object' ? t.assignedTo?.account : t.assignedTo
        if (assigned === 'REDACTED_ACCOUNT') {
          allMyTasks.push({
            taskId: t.id,
            taskName: t.name,
            status: t.status,
            projectName,
            executionId,
          })
        }
      }
      taskPage++
    }
  }

  // 获取所有根项目 (parent=0)
  console.log('=== 获取根项目列表 ===')
  let rootPage = 1
  let rootTotalPages = 1
  const rootProjects: any[] = []

  while (rootPage <= rootTotalPages) {
    const rootRes = await http.get('/api.php/v1/projects', {
      params: { page: rootPage, limit: 100 },
    })
    const projects = rootRes.data.projects || []
    if (rootPage === 1) {
      rootTotalPages = Math.ceil((rootRes.data.total || 0) / (rootRes.data.limit || 20))
      console.log(`总项目数: ${rootRes.data.total}, 总页数: ${rootTotalPages}`)
    }
    rootProjects.push(...projects)
    rootPage++
  }

  console.log(`获取到 ${rootProjects.length} 个根项目\n`)

  // 遍历每个根项目
  for (const project of rootProjects) {
    if (project.id) await exploreProject(project.id)
  }

  // 输出结果
  console.log('\n' + '='.repeat(70))
  console.log('最终统计')
  console.log('='.repeat(70))
  console.log(`访问的项目数: ${visitedProjects.size}`)
  console.log(`访问的执行数: ${visitedExecutions.size}`)
  console.log(`指派给我的任务: ${allMyTasks.length}`)

  if (allMyTasks.length > 0) {
    console.log('\n我的任务明细:')
    allMyTasks.forEach((t, i) => {
      console.log(`\n${i + 1}. [${t.status}] ${t.taskName}`)
      console.log(`   项目: ${t.projectName}`)
      console.log(`   执行ID: ${t.executionId}`)
    })
  }
}

test().catch(console.error)
