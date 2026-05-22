import axios from 'axios'
import dotenv from 'dotenv'

dotenv.config()

const BASE_URL = process.env.ZENTAO_BASE_URL || ''
const ACCOUNT = process.env.ZENTAO_ACCOUNT || ''
const PASSWORD = process.env.ZENTAO_PASSWORD || ''

async function countAllMyTasks() {
  console.log('=====================================')
  console.log('  统计所有执行中我的任务')
  console.log('=====================================')
  console.log()

  const tokenRes = await axios.post(`${BASE_URL}/api.php/v1/tokens`, {
    account: ACCOUNT,
    password: PASSWORD,
  })
  const token = tokenRes.data.token
  console.log('✅ Token 获取成功')
  console.log()

  // 1. 获取所有执行
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

  // 2. 遍历每个执行，使用 assignedTo 参数获取我的任务
  console.log('【步骤 2】遍历所有执行获取我的任务...')
  console.log()

  let totalMyTasks = 0
  const executionTaskCounts: Array<{ executionId: number; executionName: string; count: number }> = []

  for (const exec of allExecutions) {
    try {
      const taskRes = await axios.get(`${BASE_URL}/api.php/v1/executions/${exec.id}/tasks`, {
        headers: { Token: token },
        params: {
          assignedTo: ACCOUNT,
          module: 0,
          recPerPage: 100,
          page: 1,
        },
      })

      const count = taskRes.data.total || 0

      if (count > 0) {
        totalMyTasks += count
        executionTaskCounts.push({
          executionId: exec.id,
          executionName: exec.name,
          count,
        })
        console.log(`执行 ${exec.id} (${exec.name}): ${count} 个任务`)
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
  console.log(`有任务的执行数: ${executionTaskCounts.length}`)
  console.log(`我的任务总数: ${totalMyTasks}`)
  console.log()

  // 3. 尝试不同的状态筛选
  console.log('【步骤 3】尝试不同状态筛选...')
  console.log()

  const statuses = ['doing', 'wait', 'done', 'closed', 'cancel']
  const statusCounts: Record<string, number> = {}

  for (const status of statuses) {
    let count = 0
    for (const exec of allExecutions) {
      try {
        const taskRes = await axios.get(`${BASE_URL}/api.php/v1/executions/${exec.id}/tasks`, {
          headers: { Token: token },
          params: {
            assignedTo: ACCOUNT,
            status,
            module: 0,
            recPerPage: 100,
            page: 1,
          },
        })
        count += taskRes.data.total || 0
      } catch (e) {
        // ignore
      }
    }
    statusCounts[status] = count
    console.log(`status=${status}: ${count} 个任务`)
  }

  console.log()
  console.log('=====================================')
  console.log('  最终统计')
  console.log('=====================================')
  console.log()
  console.log(`我的任务总数 (assignedTo=${ACCOUNT}): ${totalMyTasks}`)
  console.log()
  console.log('按状态分布:')
  Object.entries(statusCounts).forEach(([status, count]) => {
    console.log(`  ${status}: ${count} 个`)
  })
  console.log()

  // 4. 尝试 unclosed (doing + wait)
  const unclosedCount = (statusCounts['doing'] || 0) + (statusCounts['wait'] || 0)
  console.log(`未关闭任务数 (doing + wait): ${unclosedCount}`)
  console.log()

  // 5. 显示有任务的执行列表（前20个）
  console.log('=====================================')
  console.log('  有任务的执行列表（前20个）')
  console.log('=====================================')
  console.log()

  executionTaskCounts
    .sort((a, b) => b.count - a.count)
    .slice(0, 20)
    .forEach((item, i) => {
      console.log(`${i + 1}. ${item.executionName} (ID: ${item.executionId}): ${item.count} 个任务`)
    })
}

countAllMyTasks().catch(console.error)
