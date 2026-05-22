import axios from 'axios'
import dotenv from 'dotenv'

dotenv.config()

const BASE_URL = process.env.ZENTAO_BASE_URL || ''
const ACCOUNT = process.env.ZENTAO_ACCOUNT || ''
const PASSWORD = process.env.ZENTAO_PASSWORD || ''

async function checkStructure() {
  console.log('=====================================')
  console.log('  检查项目 289 的完整结构')
  console.log('=====================================')
  console.log()

  try {
    const tokenRes = await axios.post(`${BASE_URL}/api.php/v1/tokens`, {
      account: ACCOUNT,
      password: PASSWORD,
    })
    const token = tokenRes.data.token

    // 1. 获取项目详情
    console.log('【步骤 1】获取项目 289 详情...')
    try {
      const projectRes = await axios.get(`${BASE_URL}/api.php/v1/projects/289`, {
        headers: { Token: token },
      })
      console.log('项目详情:', JSON.stringify(projectRes.data, null, 2).slice(0, 500))
    } catch (e: any) {
      console.log('获取项目详情失败:', e.response?.status)
    }
    console.log()

    // 2. 获取项目下的所有执行（包含子执行）
    console.log('【步骤 2】获取项目 289 的所有执行...')
    const execRes = await axios.get(`${BASE_URL}/api.php/v1/projects/289/executions`, {
      headers: { Token: token },
      params: { page: 1, limit: 100 },
    })

    const executions = execRes.data.executions || []
    console.log(`找到 ${executions.length} 个执行`)
    console.log()

    // 3. 遍历每个执行获取任务
    console.log('【步骤 3】遍历每个执行获取任务...')
    let totalTasks = 0

    for (const exec of executions) {
      const taskRes = await axios.get(`${BASE_URL}/api.php/v1/executions/${exec.id}/tasks`, {
        headers: { Token: token },
        params: { page: 1, limit: 100 },
      })

      const tasks = taskRes.data.tasks || []
      const taskCount = taskRes.data.total || tasks.length
      totalTasks += taskCount

      console.log(`执行 ${exec.id} (${exec.name}): ${taskCount} 个任务`)

      // 查找截图中的任务 ID
      const screenshotIds = [10259, 10258, 10257, 10249, 10244, 10243, 10238, 10195, 10193, 10189, 10188, 10140, 10139, 10123, 10122, 10121, 10120, 10119, 10118]
      const foundInThisExec = tasks.filter((t: any) => screenshotIds.includes(t.id))
      if (foundInThisExec.length > 0) {
        console.log(`  ✅ 找到 ${foundInThisExec.length} 个截图中的任务!`)
        foundInThisExec.forEach((t: any) => {
          console.log(`     - ${t.name} (ID: ${t.id})`)
        })
      }
    }

    console.log()
    console.log(`项目 289 总任务数: ${totalTasks}`)

  } catch (error: any) {
    console.error('❌ 错误:', error.message)
  }
}

checkStructure()
