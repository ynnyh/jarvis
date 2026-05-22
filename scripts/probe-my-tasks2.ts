import axios from 'axios'
import dotenv from 'dotenv'

dotenv.config()

const BASE_URL = process.env.ZENTAO_BASE_URL || ''
const ACCOUNT = process.env.ZENTAO_ACCOUNT || ''
const PASSWORD = process.env.ZENTAO_PASSWORD || ''

async function probe() {
  const tokenRes = await axios.post(`${BASE_URL}/api.php/v1/tokens`, {
    account: ACCOUNT,
    password: PASSWORD,
  })
  const token = tokenRes.data.token
  console.log('✅ Token 获取成功')

  // 1. 检查执行列表的分页信息
  console.log('\n--- 检查执行列表分页 ---')
  const execRes = await axios.get(`${BASE_URL}/api.php/v1/executions`, {
    headers: { 'Token': token },
    params: { status: 'all', recPerPage: 20, page: 1 },
  })
  console.log('分页信息:', {
    page: execRes.data.page,
    total: execRes.data.total,
    limit: execRes.data.limit,
  })
  console.log(`第一页执行数: ${execRes.data.executions?.length || 0}`)

  // 2. 遍历所有页获取所有执行
  const allExecutions: any[] = []
  let page = 1
  let totalPages = 1

  console.log('\n--- 获取所有执行（分页）---')
  while (page <= totalPages) {
    const res = await axios.get(`${BASE_URL}/api.php/v1/executions`, {
      headers: { 'Token': token },
      params: { status: 'all', recPerPage: 100, page: page },
    })
    const executions = res.data.executions || []
    allExecutions.push(...executions)
    totalPages = Math.ceil((res.data.total || 0) / (res.data.limit || 100))
    console.log(`第 ${page}/${totalPages} 页: +${executions.length} 个执行`)
    page++
  }
  console.log(`总执行数: ${allExecutions.length}`)

  // 3. 遍历所有执行获取任务
  console.log('\n--- 遍历所有执行查找我的任务 ---')
  let totalTasks = 0
  const myTasks: any[] = []

  for (let i = 0; i < allExecutions.length; i++) {
    const exec = allExecutions[i]
    try {
      const taskRes = await axios.get(`${BASE_URL}/api.php/v1/executions/${exec.id}/tasks`, {
        headers: { 'Token': token },
        params: { recPerPage: 500 },
      })
      const tasks = taskRes.data.tasks || []
      totalTasks += tasks.length

      for (const t of tasks) {
        const assignedToAccount = t.assignedTo?.account || t.assignedTo
        if (assignedToAccount === 'REDACTED_ACCOUNT') {
          myTasks.push({
            id: t.id,
            name: t.name,
            status: t.status,
            execution: exec.name,
            assignedTo: t.assignedTo,
            deadline: t.deadline,
            pri: t.pri,
          })
        }
      }

      if ((i + 1) % 20 === 0) {
        console.log(`已处理 ${i + 1}/${allExecutions.length} 个执行，找到 ${myTasks.length} 个我的任务...`)
      }
    } catch (e) {
      // ignore
    }
  }

  console.log(`\n总任务数: ${totalTasks}`)
  console.log(`指派给 REDACTED_ACCOUNT 的任务: ${myTasks.length}`)

  if (myTasks.length > 0) {
    console.log('\n--- 我的任务列表 ---')
    myTasks.forEach((t, i) => {
      console.log(`${i + 1}. [${t.status}] ${t.name}`)
      console.log(`   执行: ${t.execution} | 截止: ${t.deadline || '无'} | 优先级: ${t.pri}`)
    })
  }
}

probe().catch(console.error)
