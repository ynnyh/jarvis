import axios from 'axios'
import dotenv from 'dotenv'

dotenv.config()

const BASE_URL = process.env.ZENTAO_BASE_URL || ''
const ACCOUNT = process.env.ZENTAO_ACCOUNT || ''
const PASSWORD = process.env.ZENTAO_PASSWORD || ''

async function probe() {
  // 1. 获取 Token
  const tokenRes = await axios.post(`${BASE_URL}/api.php/v1/tokens`, {
    account: ACCOUNT,
    password: PASSWORD,
  })
  const token = tokenRes.data.token
  console.log('✅ Token 获取成功:', token.slice(0, 15) + '...')

  // 2. 使用 v1 接口获取执行列表
  console.log('\n--- 获取执行列表 (v1) ---')
  try {
    const execRes = await axios.get(`${BASE_URL}/api.php/v1/executions`, {
      headers: { 'Token': token },
      params: { status: 'all', recPerPage: 100 },
    })
    console.log('执行列表响应:', JSON.stringify(execRes.data).slice(0, 500))
  } catch (e: any) {
    console.log('错误:', e.response?.status, e.message)
  }

  // 3. 使用 v1 接口获取任务列表（有数据的 sprint）
  console.log('\n--- 获取 Sprint 3 的任务列表 (v1) ---')
  const taskRes = await axios.get(`${BASE_URL}/api.php/v1/executions/3/tasks`, {
    headers: { 'Token': token },
    params: { recPerPage: 100 },
  })

  const tasks = taskRes.data.tasks || []
  console.log(`✅ 获取到 ${tasks.length} 个任务`)
  console.log(`分页信息: page=${taskRes.data.page}, total=${taskRes.data.total}, limit=${taskRes.data.limit}`)

  console.log('\n前 5 个任务预览:')
  tasks.slice(0, 5).forEach((t: any, i: number) => {
    console.log(`\n${i + 1}. [${t.status}] ${t.name}`)
    console.log(`   ID: ${t.id} | 优先级: ${t.pri} | 类型: ${t.type}`)
    console.log(`   指派给: ${t.assignedTo?.realname || t.assignedTo || '无'}`)
    console.log(`   截止日期: ${t.deadline || '无'} | 预计工时: ${t.estimate || 0}h`)
    console.log(`   已消耗: ${t.consumed || 0}h | 剩余: ${t.left || 0}h`)
  })

  // 4. 获取第一个任务的详情
  if (tasks.length > 0) {
    const firstTask = tasks[0]
    console.log(`\n--- 获取任务详情 (ID: ${firstTask.id}) ---`)
    try {
      const detailRes = await axios.get(`${BASE_URL}/api.php/v1/tasks/${firstTask.id}`, {
        headers: { 'Token': token },
      })
      const detail = detailRes.data
      console.log('任务详情:')
      console.log(JSON.stringify(detail, null, 2).slice(0, 1000))
    } catch (e: any) {
      console.log('错误:', e.response?.status, e.message)
    }
  }

  // 5. 获取 "我的任务" - 筛选指派给当前用户的
  console.log('\n--- 筛选 "我的任务" ---')
  const myTasks = tasks.filter((t: any) => {
    const assignee = t.assignedTo?.account || t.assignedTo
    return assignee === 'REDACTED_ACCOUNT'
  })
  console.log(`指派给 REDACTED_ACCOUNT 的任务: ${myTasks.length} 个`)
  myTasks.slice(0, 3).forEach((t: any, i: number) => {
    console.log(`  ${i + 1}. [${t.status}] ${t.name}`)
  })
}

probe().catch(console.error)
