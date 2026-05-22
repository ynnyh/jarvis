import axios from 'axios'
import dotenv from 'dotenv'

dotenv.config()

const BASE_URL = process.env.ZENTAO_BASE_URL || ''
const ACCOUNT = process.env.ZENTAO_ACCOUNT || ''
const PASSWORD = process.env.ZENTAO_PASSWORD || ''

async function testProjectExecutions() {
  console.log('=====================================')
  console.log('  获取项目 289 的执行列表')
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

    // 2. 获取项目 289 的执行列表
    const projectId = 289
    console.log(`【步骤 2】获取项目 ${projectId} 的执行列表...`)
    
    const execRes = await axios.get(`${BASE_URL}/api.php/v1/projects/${projectId}/executions`, {
      headers: { Token: token },
      params: { page: 1, limit: 100 },
    })

    console.log('响应结构:', Object.keys(execRes.data))
    console.log('page:', execRes.data.page)
    console.log('total:', execRes.data.total)
    console.log('limit:', execRes.data.limit)
    console.log('executions 数量:', execRes.data.executions?.length)
    console.log()

    // 3. 显示执行列表
    const executions = execRes.data.executions || []
    
    console.log('=====================================')
    console.log(`  项目 ${projectId} 的执行列表 (${executions.length} 个)`)
    console.log('=====================================')
    console.log()

    executions.forEach((e: any, i: number) => {
      console.log(`${i + 1}. [${e.status}] ${e.name}`)
      console.log(`   ID: ${e.id} | 代号: ${e.code || '无'}`)
      console.log(`   类型: ${e.type} | 进度: ${e.progress}%`)
      console.log(`   时间: ${e.begin} ~ ${e.end}`)
      console.log()
    })

    // 4. 如果有执行，获取第一个执行的任务
    if (executions.length > 0) {
      const firstExec = executions[0]
      console.log('=====================================')
      console.log(`  执行 ${firstExec.id} 的任务列表`)
      console.log('=====================================')
      console.log()

      const taskRes = await axios.get(`${BASE_URL}/api.php/v1/executions/${firstExec.id}/tasks`, {
        headers: { Token: token },
        params: { page: 1, limit: 100 },
      })

      const tasks = taskRes.data.tasks || []
      console.log(`找到 ${tasks.length} 个任务`)
      console.log()

      tasks.forEach((t: any, i: number) => {
        const assignee = t.assignedTo?.account || t.assignedTo || '无'
        console.log(`${i + 1}. [${t.status}] ${t.name}`)
        console.log(`   ID: ${t.id} | 优先级: ${t.pri} | 指派给: ${assignee}`)
        console.log(`   截止: ${t.deadline || '无'} | 预计: ${t.estimate || 0}h`)
        console.log()
      })

      // 筛选我的任务
      const myTasks = tasks.filter((t: any) => {
        const assignee = t.assignedTo?.account || t.assignedTo
        return assignee === ACCOUNT
      })

      if (myTasks.length > 0) {
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
      }
    }

  } catch (error: any) {
    console.error('❌ 错误:', error.message)
    if (error.response) {
      console.error('状态码:', error.response.status)
      console.error('响应:', error.response.data)
    }
  }
}

testProjectExecutions()
