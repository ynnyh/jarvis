import axios from 'axios'
import dotenv from 'dotenv'

dotenv.config()

const BASE_URL = process.env.ZENTAO_BASE_URL || ''
const ACCOUNT = process.env.ZENTAO_ACCOUNT || ''
const PASSWORD = process.env.ZENTAO_PASSWORD || ''

async function checkAllTasks() {
  console.log('=====================================')
  console.log('  检查执行 289 的所有任务（包含 closed）')
  console.log('=====================================')
  console.log()

  try {
    // 1. 获取 Token
    const tokenRes = await axios.post(`${BASE_URL}/api.php/v1/tokens`, {
      account: ACCOUNT,
      password: PASSWORD,
    })
    const token = tokenRes.data.token

    // 2. 获取执行 289 的任务（分页获取所有）
    const executionId = 289
    console.log(`获取执行 ${executionId} 的所有任务...`)
    
    const allTasks: any[] = []
    let page = 1
    let totalPages = 1

    while (page <= totalPages) {
      const taskRes = await axios.get(`${BASE_URL}/api.php/v1/executions/${executionId}/tasks`, {
        headers: { Token: token },
        params: { page, limit: 100 },
      })

      const tasks = taskRes.data.tasks || []
      allTasks.push(...tasks)

      if (page === 1) {
        totalPages = Math.ceil((taskRes.data.total || 0) / (taskRes.data.limit || 100))
        console.log(`总任务数: ${taskRes.data.total}, 每页: ${taskRes.data.limit}, 总页数: ${totalPages}`)
      }

      console.log(`  第 ${page}/${totalPages} 页: +${tasks.length} 个任务`)
      page++
    }

    console.log()
    console.log('=====================================')
    console.log(`  所有任务 (${allTasks.length} 个)`)
    console.log('=====================================')
    console.log()

    // 按状态分组
    const statusCount: Record<string, number> = {}
    allTasks.forEach((t: any) => {
      statusCount[t.status] = (statusCount[t.status] || 0) + 1
    })

    console.log('状态分布:')
    Object.entries(statusCount).forEach(([status, count]) => {
      console.log(`  ${status}: ${count} 个`)
    })
    console.log()

    // 显示前 20 个任务
    console.log('前 20 个任务:')
    allTasks.slice(0, 20).forEach((t: any, i: number) => {
      const assignee = t.assignedTo?.account || t.assignedTo || '无'
      console.log(`${i + 1}. [${t.status}] ${t.name}`)
      console.log(`   ID: ${t.id} | 指派给: ${assignee}`)
    })

    // 查找截图中的任务 ID
    const screenshotIds = [10259, 10258, 10257, 10249, 10244, 10243, 10238, 10195, 10193, 10189, 10188, 10140, 10139, 10123, 10122, 10121, 10120, 10119, 10118]
    console.log()
    console.log('=====================================')
    console.log('  查找截图中的任务')
    console.log('=====================================')
    console.log()

    const foundTasks = allTasks.filter((t: any) => screenshotIds.includes(t.id))
    console.log(`找到 ${foundTasks.length} 个任务`)
    
    foundTasks.forEach((t: any) => {
      const assignee = t.assignedTo?.account || t.assignedTo || '无'
      console.log(`- [${t.status}] ${t.name} (ID: ${t.id}, 指派给: ${assignee})`)
    })

    // 检查是否有权限问题
    const notFoundIds = screenshotIds.filter(id => !allTasks.some((t: any) => t.id === id))
    if (notFoundIds.length > 0) {
      console.log()
      console.log('未找到的任务 ID:', notFoundIds.join(', '))
      console.log('可能是权限不足或任务在其他执行/项目中')
    }

  } catch (error: any) {
    console.error('❌ 错误:', error.message)
  }
}

checkAllTasks()
