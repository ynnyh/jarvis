import axios from 'axios'
import dotenv from 'dotenv'

dotenv.config()

const BASE_URL = process.env.ZENTAO_BASE_URL || ''
const ACCOUNT = process.env.ZENTAO_ACCOUNT || ''
const PASSWORD = process.env.ZENTAO_PASSWORD || ''

async function analyzeTaskModel() {
  console.log('=====================================')
  console.log('  禅道任务查询模型分析')
  console.log('=====================================')
  console.log()

  const tokenRes = await axios.post(`${BASE_URL}/api.php/v1/tokens`, {
    account: ACCOUNT,
    password: PASSWORD,
  })
  const token = tokenRes.data.token
  console.log('✅ Token 获取成功')
  console.log()

  // ========================================
  // 1. 分析 execution 289 的 module 结构
  // ========================================
  console.log('【分析 1】execution 289 的 module 结构')
  console.log('-------------------------------------')

  // 尝试获取 execution 的 module 列表
  try {
    const moduleRes = await axios.get(`${BASE_URL}/api.php/v1/executions/289/modules`, {
      headers: { Token: token },
    })
    console.log('Module API 响应:', JSON.stringify(moduleRes.data, null, 2).slice(0, 1000))
  } catch (e: any) {
    console.log('Module API 不存在:', e.response?.status)
  }
  console.log()

  // ========================================
  // 2. 尝试不同的 browseType
  // ========================================
  console.log('【分析 2】不同的 browseType 参数')
  console.log('-------------------------------------')

  const browseTypes = ['all', 'unclosed', 'assignedtome', 'myinvolved', 'delayed', 'needconfirm', 'wait', 'doing', 'done', 'closed', 'cancel']

  for (const browseType of browseTypes) {
    try {
      const res = await axios.get(`${BASE_URL}/api.php/v1/executions/289/tasks`, {
        headers: { Token: token },
        params: { browseType, recPerPage: 100 },
      })

      const tasks = res.data.tasks || []
      console.log(`browseType=${browseType}: ${tasks.length} 个任务`)

      if (tasks.length > 0 && browseType === 'assignedtome') {
        console.log('  ✅ assignedtome 有任务!')
        tasks.slice(0, 3).forEach((t: any) => {
          console.log(`     - ${t.name} (ID: ${t.id})`)
        })
      }
    } catch (e: any) {
      console.log(`browseType=${browseType}: 错误 ${e.response?.status}`)
    }
  }
  console.log()

  // ========================================
  // 3. 尝试 module 参数
  // ========================================
  console.log('【分析 3】不同的 module 参数')
  console.log('-------------------------------------')

  // 尝试一些常见的 module ID
  const moduleIds = [0, 1, 759, 760, 761, 762, 763]

  for (const moduleId of moduleIds) {
    try {
      const res = await axios.get(`${BASE_URL}/api.php/v1/executions/289/tasks`, {
        headers: { Token: token },
        params: { module: moduleId, recPerPage: 100 },
      })

      const tasks = res.data.tasks || []
      console.log(`module=${moduleId}: ${tasks.length} 个任务`)

      if (tasks.length > 0) {
        console.log('  任务列表:')
        tasks.slice(0, 3).forEach((t: any) => {
          console.log(`     - ${t.name} (ID: ${t.id})`)
        })
      }
    } catch (e: any) {
      console.log(`module=${moduleId}: 错误 ${e.response?.status}`)
    }
  }
  console.log()

  // ========================================
  // 4. 组合参数测试
  // ========================================
  console.log('【分析 4】组合参数测试')
  console.log('-------------------------------------')

  try {
    const res = await axios.get(`${BASE_URL}/api.php/v1/executions/289/tasks`, {
      headers: { Token: token },
      params: {
        browseType: 'all',
        module: 0,
        recPerPage: 500,
      },
    })

    const tasks = res.data.tasks || []
    console.log(`browseType=all, module=0: ${tasks.length} 个任务`)

    if (tasks.length > 0) {
      console.log('  前 5 个任务:')
      tasks.slice(0, 5).forEach((t: any) => {
        console.log(`     - ${t.name} (ID: ${t.id}, status: ${t.status})`)
      })
    }
  } catch (e: any) {
    console.log('组合参数错误:', e.response?.status)
  }
  console.log()

  // ========================================
  // 5. 检查 assignedTo 参数
  // ========================================
  console.log('【分析 5】assignedTo 参数测试')
  console.log('-------------------------------------')

  try {
    const res = await axios.get(`${BASE_URL}/api.php/v1/executions/289/tasks`, {
      headers: { Token: token },
      params: {
        assignedTo: 'REDACTED_ACCOUNT',
        recPerPage: 100,
      },
    })

    const tasks = res.data.tasks || []
    console.log(`assignedTo=REDACTED_ACCOUNT: ${tasks.length} 个任务`)

    if (tasks.length > 0) {
      tasks.forEach((t: any) => {
        console.log(`  - ${t.name} (ID: ${t.id})`)
      })
    }
  } catch (e: any) {
    console.log('assignedTo 参数错误:', e.response?.status)
  }
  console.log()

  // ========================================
  // 6. 尝试直接访问 HTML 页面获取 module 信息
  // ========================================
  console.log('【分析 6】从 HTML 页面获取 module 信息')
  console.log('-------------------------------------')

  try {
    const htmlRes = await axios.get(`${BASE_URL}/execution-task-289.html`, {
      headers: { Token: token },
    })

    const html = htmlRes.data
    console.log('HTML 页面长度:', html.length)

    // 查找 module 相关信息
    const moduleMatch = html.match(/module.*?=.*?(\d+)/g)
    if (moduleMatch) {
      console.log('找到 module 相关代码:', moduleMatch.slice(0, 5))
    }

    // 查找 browseType 相关信息
    const browseMatch = html.match(/browseType.*?=.*?'(\w+)'/g)
    if (browseMatch) {
      console.log('找到 browseType 相关代码:', browseMatch.slice(0, 5))
    }
  } catch (e: any) {
    console.log('HTML 页面错误:', e.response?.status)
  }

  console.log()
  console.log('=====================================')
  console.log('  分析完成')
  console.log('=====================================')
}

analyzeTaskModel().catch(console.error)
