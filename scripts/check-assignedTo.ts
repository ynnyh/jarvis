import axios from 'axios'
import dotenv from 'dotenv'

dotenv.config()

const BASE_URL = process.env.ZENTAO_BASE_URL || ''
const ACCOUNT = process.env.ZENTAO_ACCOUNT || ''
const PASSWORD = process.env.ZENTAO_PASSWORD || ''

async function checkAssignedTo() {
  console.log('=====================================')
  console.log('  检查 assignedTo 字段')
  console.log('=====================================')
  console.log()

  const tokenRes = await axios.post(`${BASE_URL}/api.php/v1/tokens`, {
    account: ACCOUNT,
    password: PASSWORD,
  })
  const token = tokenRes.data.token

  // 获取几个任务检查 assignedTo
  const executionId = 289
  
  console.log(`检查执行 ${executionId} 的任务...`)
  console.log()

  // 不带 assignedTo 参数
  console.log('【不带 assignedTo 参数】')
  const res1 = await axios.get(`${BASE_URL}/api.php/v1/executions/${executionId}/tasks`, {
    headers: { Token: token },
    params: { recPerPage: 5 },
  })

  const tasks1 = res1.data.tasks || []
  tasks1.forEach((t: any, i: number) => {
    console.log(`${i + 1}. ${t.name}`)
    console.log(`   assignedTo: ${JSON.stringify(t.assignedTo)}`)
  })
  console.log()

  // 带 assignedTo 参数
  console.log('【带 assignedTo=REDACTED_ACCOUNT 参数】')
  const res2 = await axios.get(`${BASE_URL}/api.php/v1/executions/${executionId}/tasks`, {
    headers: { Token: token },
    params: { assignedTo: 'REDACTED_ACCOUNT', recPerPage: 5 },
  })

  const tasks2 = res2.data.tasks || []
  console.log(`返回 ${tasks2.length} 个任务`)
  tasks2.forEach((t: any, i: number) => {
    console.log(`${i + 1}. ${t.name}`)
    console.log(`   assignedTo: ${JSON.stringify(t.assignedTo)}`)
  })
  console.log()

  // 检查 assignedTo 参数是否生效
  console.log('【验证 assignedTo 参数是否生效】')
  console.log(`不带参数返回: ${tasks1.length} 个`)
  console.log(`带参数返回: ${tasks2.length} 个`)
  
  if (tasks1.length === tasks2.length) {
    console.log('⚠️ assignedTo 参数可能未生效！')
  } else {
    console.log('✅ assignedTo 参数生效')
  }
}

checkAssignedTo().catch(console.error)
