import axios from 'axios'
import dotenv from 'dotenv'

dotenv.config()

const BASE_URL = process.env.ZENTAO_BASE_URL || ''
const ACCOUNT = process.env.ZENTAO_ACCOUNT || ''
const PASSWORD = process.env.ZENTAO_PASSWORD || ''

async function dumpAllTasks() {
  console.log('=====================================')
  console.log('  导出所有 doing + wait 任务')
  console.log('=====================================')
  console.log()

  const tokenRes = await axios.post(`${BASE_URL}/api.php/v1/tokens`, {
    account: ACCOUNT,
    password: PASSWORD,
  })
  const token = tokenRes.data.token
  console.log('✅ Token 获取成功')
  console.log()

  // 获取所有执行
  console.log('【步骤 1】获取所有执行...')
  const allExecutions: any[] = []
  let page = 1
  let totalPages = 1

  while (page <= totalPages) {
    const execRes = await axios.get(`${BASE_URL}/api.php/v1/executions`, {
      headers: { Token: token },
      params: { status: 'all', recPerPage: 100, page },
    })
    const executions = execRes.data.executions || []
    allExecutions.push(...executions)
    if (page === 1) {
      totalPages = Math.ceil((execRes.data.total || 0) / (execRes.data.limit || 20))
    }
    page++
  }

  console.log(`共 ${allExecutions.length} 个执行`)
  console.log()

  // 获取 doing 和 wait 的任务
  console.log('【步骤 2】获取所有 doing + wait 任务...')
  console.log()

  const taskMap = new Map<number, any>()
  let processedExecutions = 0

  for (const status of ['doing', 'wait']) {
    for (const exec of allExecutions) {
      try {
        let taskPage = 1
        let taskTotalPages = 1

        while (taskPage <= taskTotalPages) {
          const taskRes = await axios.get(`${BASE_URL}/api.php/v1/executions/${exec.id}/tasks`, {
            headers: { Token: token },
            params: {
              assignedTo: ACCOUNT,
              status,
              module: 0,
              recPerPage: 100,
              page: taskPage,
            },
          })

          const tasks = taskRes.data.tasks || []
          
          if (taskPage === 1) {
            taskTotalPages = Math.ceil((taskRes.data.total || 0) / (taskRes.data.limit || 100))
          }

          for (const t of tasks) {
            if (!taskMap.has(t.id)) {
              taskMap.set(t.id, {
                id: t.id,
                name: t.name,
                status: t.status,
                pri: t.pri,
                deadline: t.deadline,
                assignedTo: t.assignedTo?.account || t.assignedTo,
                executionId: exec.id,
                executionName: exec.name,
              })
            }
          }

          taskPage++
        }

        processedExecutions++
        if (processedExecutions % 20 === 0) {
          console.log(`已处理 ${processedExecutions}/${allExecutions.length} 个执行，当前 ${taskMap.size} 个任务`)
        }
      } catch (e) {
        // ignore
      }
    }
  }

  const allTasks = Array.from(taskMap.values())
  
  console.log()
  console.log('=====================================')
  console.log(`  任务列表 (${allTasks.length} 个)`)
  console.log('=====================================')
  console.log()

  // 按执行分组显示
  const tasksByExecution: Record<string, any[]> = {}
  allTasks.forEach(t => {
    if (!tasksByExecution[t.executionName]) {
      tasksByExecution[t.executionName] = []
    }
    tasksByExecution[t.executionName].push(t)
  })

  let index = 1
  Object.entries(tasksByExecution).forEach(([execName, tasks]) => {
    console.log(`\n【${execName}】(${tasks.length} 个)`)
    console.log('-'.repeat(50))
    
    tasks.forEach(t => {
      console.log(`${index}. [${t.status}] ${t.name}`)
      console.log(`   ID: ${t.id} | 优先级: ${t.pri} | 截止: ${t.deadline || '无'}`)
      index++
    })
  })

  console.log()
  console.log('=====================================')
  console.log(`  总计: ${allTasks.length} 个任务`)
  console.log('=====================================')
}

dumpAllTasks().catch(console.error)
