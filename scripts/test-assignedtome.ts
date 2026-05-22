import axios from 'axios'
import dotenv from 'dotenv'

dotenv.config()

const BASE_URL = process.env.ZENTAO_BASE_URL || ''
const ACCOUNT = process.env.ZENTAO_ACCOUNT || ''
const PASSWORD = process.env.ZENTAO_PASSWORD || ''

async function testAssignedToMe() {
  console.log('=====================================')
  console.log('  使用 status=assignedtome 获取任务')
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

  // 遍历所有执行，使用 status=assignedtome 获取任务
  console.log('【步骤 2】遍历所有执行获取任务...')
  console.log()

  const taskMap = new Map<number, any>()
  let totalTasks = 0

  for (const exec of allExecutions) {
    try {
      const taskRes = await axios.get(`${BASE_URL}/api.php/v1/executions/${exec.id}/tasks`, {
        headers: { Token: token },
        params: {
          status: 'assignedtome',
          module: 0,
          recPerPage: 100,
          page: 1,
        },
      })

      const tasks = taskRes.data.tasks || []
      const count = taskRes.data.total || tasks.length

      if (count > 0) {
        totalTasks += count
        console.log(`执行 ${exec.id} (${exec.name}): ${count} 个任务`)

        for (const t of tasks) {
          if (!taskMap.has(t.id)) {
            taskMap.set(t.id, {
              ...t,
              executionName: exec.name,
            })
          }
        }
      }
    } catch (e) {
      // ignore
    }
  }

  console.log()
  console.log('=====================================')
  console.log('  统计结果')
  console.log('=====================================')
  console.log()
  console.log(`总任务数（未去重）: ${totalTasks}`)
  console.log(`去重后任务数: ${taskMap.size}`)
  console.log()

  // 按状态统计
  const statusCount: Record<string, number> = {}
  taskMap.forEach((task) => {
    statusCount[task.status] = (statusCount[task.status] || 0) + 1
  })

  console.log('状态分布:')
  Object.entries(statusCount).forEach(([status, count]) => {
    console.log(`  ${status}: ${count} 个`)
  })
  console.log()

  // 过滤掉 closed 的任务
  const activeTasks = Array.from(taskMap.values()).filter((t) => t.status !== 'closed')
  console.log(`未关闭任务数: ${activeTasks.length}`)
  console.log()

  // 显示前 20 个任务
  console.log('=====================================')
  console.log('  任务列表（前 20 个）')
  console.log('=====================================')
  console.log()

  activeTasks.slice(0, 20).forEach((t, i) => {
    console.log(`${i + 1}. [${t.status}] ${t.name}`)
    console.log(`   ID: ${t.id} | 优先级: ${t.pri}`)
    console.log(`   执行: ${t.executionName}`)
    console.log(`   截止: ${t.deadline || '无'}`)
    console.log()
  })
}

testAssignedToMe().catch(console.error)
