import axios from 'axios'
import dotenv from 'dotenv'

dotenv.config()

const BASE_URL = process.env.ZENTAO_BASE_URL || ''
const ACCOUNT = process.env.ZENTAO_ACCOUNT || ''
const PASSWORD = process.env.ZENTAO_PASSWORD || ''

async function testExecutionTasks() {
  console.log('=====================================')
  console.log('  获取执行下的任务列表')
  console.log('=====================================')
  console.log()

  try {
    // 1. 获取 Token
    console.log('【步骤 1】获取 Token...')
    const tokenRes = await axios.post(`${BASE_URL}/api.php/v1/tokens`, {
      account: ACCOUNT,
      password: PASSWORD,
    })
    const token = tokenRes.data.token
    console.log('✅ Token 获取成功')
    console.log()

    // 2. 获取执行 289 的任务列表
    const executionId = 289
    console.log(`【步骤 2】获取执行 ${executionId} 的任务列表...`)
    
    const taskRes = await axios.get(`${BASE_URL}/api.php/v1/executions/${executionId}/tasks`, {
      headers: { Token: token },
      params: { page: 1, limit: 100 },
    })

    console.log('响应结构:', Object.keys(taskRes.data))
    console.log('page:', taskRes.data.page)
    console.log('total:', taskRes.data.total)
    console.log('limit:', taskRes.data.limit)
    console.log('tasks 数量:', taskRes.data.tasks?.length)
    console.log()

    // 3. 显示任务列表
    const tasks = taskRes.data.tasks || []
    
    console.log('=====================================')
    console.log(`  执行 ${executionId} 的任务列表 (${tasks.length} 个)`)
    console.log('=====================================')
    console.log()

    tasks.forEach((t: any, i: number) => {
      const assignee = t.assignedTo?.account || t.assignedTo || '无'
      console.log(`${i + 1}. [${t.status}] ${t.name}`)
      console.log(`   ID: ${t.id} | 优先级: ${t.pri}`)
      console.log(`   指派给: ${assignee}`)
      console.log(`   截止: ${t.deadline || '无'} | 预计: ${t.estimate || 0}h`)
      console.log()
    })

    // 4. 筛选我的任务
    const myTasks = tasks.filter((t: any) => {
      const assignee = t.assignedTo?.account || t.assignedTo
      return assignee === ACCOUNT
    })

    console.log('=====================================')
    console.log(`  我的任务 (${myTasks.length} 个)`)
    console.log('=====================================')
    console.log()

    myTasks.forEach((t: any, i: number) => {
      console.log(`${i + 1}. [${t.status}] ${t.name}`)
      console.log(`   ID: ${t.id} | 优先级: ${t.pri}`)
      console.log(`   截止: ${t.deadline || '无'} | 预计: ${t.estimate || 0}h`)
      console.log()
    })

  } catch (error: any) {
    console.error('❌ 错误:', error.message)
    if (error.response) {
      console.error('状态码:', error.response.status)
      console.error('响应:', error.response.data)
    }
  }
}

testExecutionTasks()
