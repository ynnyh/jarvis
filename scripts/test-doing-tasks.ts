import axios from 'axios'
import dotenv from 'dotenv'

dotenv.config()

const BASE_URL = process.env.ZENTAO_BASE_URL || ''
const ACCOUNT = process.env.ZENTAO_ACCOUNT || ''
const PASSWORD = process.env.ZENTAO_PASSWORD || ''

async function testDoingTasks() {
  console.log('=====================================')
  console.log('  获取 doing 状态的任务')
  console.log('=====================================')
  console.log()

  const tokenRes = await axios.post(`${BASE_URL}/api.php/v1/tokens`, {
    account: ACCOUNT,
    password: PASSWORD,
  })
  const token = tokenRes.data.token

  // 获取所有执行
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

  // 使用 assignedTo + status=doing
  const taskMap = new Map<number, any>()

  for (const exec of allExecutions) {
    try {
      const taskRes = await axios.get(`${BASE_URL}/api.php/v1/executions/${exec.id}/tasks`, {
        headers: { Token: token },
        params: {
          assignedTo: ACCOUNT,
          status: 'doing',
          module: 0,
          recPerPage: 100,
          page: 1,
        },
      })

      const tasks = taskRes.data.tasks || []
      const count = taskRes.data.total || tasks.length

      if (count > 0) {
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
  console.log(`doing 状态任务总数: ${taskMap.size}`)
  console.log()

  // 显示前 20 个
  const tasks = Array.from(taskMap.values())
  tasks.slice(0, 20).forEach((t, i) => {
    console.log(`${i + 1}. ${t.name}`)
    console.log(`   ID: ${t.id} | 执行: ${t.executionName}`)
  })
}

testDoingTasks().catch(console.error)
