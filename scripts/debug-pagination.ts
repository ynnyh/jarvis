import axios from 'axios'
import dotenv from 'dotenv'

dotenv.config()

const BASE_URL = process.env.ZENTAO_BASE_URL || ''
const ACCOUNT = process.env.ZENTAO_ACCOUNT || ''
const PASSWORD = process.env.ZENTAO_PASSWORD || ''

async function debug() {
  const tokenRes = await axios.post(`${BASE_URL}/api.php/v1/tokens`, {
    account: ACCOUNT,
    password: PASSWORD,
  })
  const token = tokenRes.data.token
  console.log('✅ Token 获取成功')

  // 1. 检查执行列表的分页
  console.log('\n=== 检查执行列表分页 ===')
  const firstPage = await axios.get(`${BASE_URL}/api.php/v1/executions`, {
    headers: { 'Token': token },
    params: { status: 'all', recPerPage: 500, page: 1 },
  })

  console.log('第一页响应:')
  console.log('  page:', firstPage.data.page)
  console.log('  total:', firstPage.data.total)
  console.log('  limit:', firstPage.data.limit)
  console.log('  executions 数量:', firstPage.data.executions?.length)

  // 2. 获取所有执行
  const allExecutions: any[] = []
  let page = 1
  let totalPages = Math.ceil((firstPage.data.total || 0) / (firstPage.data.limit || 500))

  console.log(`\n预计总页数: ${totalPages}`)

  while (page <= totalPages) {
    const res = await axios.get(`${BASE_URL}/api.php/v1/executions`, {
      headers: { 'Token': token },
      params: { status: 'all', recPerPage: 500, page },
    })
    const executions = res.data.executions || []
    allExecutions.push(...executions)
    console.log(`第 ${page} 页: +${executions.length} 个执行，累计: ${allExecutions.length}`)
    page++
  }

  console.log(`\n总执行数: ${allExecutions.length} (API 报告: ${firstPage.data.total})`)

  // 3. 检查某个有大量任务的执行的分页情况
  console.log('\n=== 检查任务列表分页 ===')
  // 找一个任务多的执行，比如执行 35 有 100 个任务
  const execWithManyTasks = 35

  const taskFirstPage = await axios.get(`${BASE_URL}/api.php/v1/executions/${execWithManyTasks}/tasks`, {
    headers: { 'Token': token },
    params: { recPerPage: 500, page: 1 },
  })

  console.log(`执行 ${execWithManyTasks} 任务列表:`)
  console.log('  page:', taskFirstPage.data.page)
  console.log('  total:', taskFirstPage.data.total)
  console.log('  limit:', taskFirstPage.data.limit)
  console.log('  tasks 数量:', taskFirstPage.data.tasks?.length)

  // 4. 遍历所有执行获取所有任务（带分页）
  console.log('\n=== 遍历所有执行获取所有任务 ===')
  let totalTasks = 0
  let myTasksCount = 0

  for (const exec of allExecutions) {
    let taskPage = 1
    let taskTotalPages = 1
    let execTasks = 0

    while (taskPage <= taskTotalPages) {
      try {
        const res = await axios.get(`${BASE_URL}/api.php/v1/executions/${exec.id}/tasks`, {
          headers: { 'Token': token },
          params: { recPerPage: 500, page: taskPage },
        })
        const tasks = res.data.tasks || []
        execTasks += tasks.length
        totalTasks += tasks.length

        // 统计我的任务
        for (const t of tasks) {
          const assignee = t.assignedTo?.account || t.assignedTo
          if (assignee === 'REDACTED_ACCOUNT' && t.status !== 'closed') {
            myTasksCount++
          }
        }

        if (taskPage === 1) {
          taskTotalPages = Math.ceil((res.data.total || 0) / (res.data.limit || 500))
        }

        taskPage++
      } catch (e) {
        break
      }
    }

    if (execTasks > 0) {
      console.log(`执行 ${exec.id} (${exec.name}): ${execTasks} 个任务`)
    }
  }

  console.log(`\n总任务数: ${totalTasks}`)
  console.log(`我的任务数 (不含 closed): ${myTasksCount}`)
}

debug().catch(console.error)
