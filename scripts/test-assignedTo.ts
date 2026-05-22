import axios from 'axios'
import dotenv from 'dotenv'

dotenv.config()

const BASE_URL = process.env.ZENTAO_BASE_URL || ''
const ACCOUNT = process.env.ZENTAO_ACCOUNT || ''
const PASSWORD = process.env.ZENTAO_PASSWORD || ''

async function testAssignedTo() {
  console.log('=====================================')
  console.log('  使用 assignedTo 参数测试')
  console.log('=====================================')
  console.log()

  const tokenRes = await axios.post(`${BASE_URL}/api.php/v1/tokens`, {
    account: ACCOUNT,
    password: PASSWORD,
  })
  const token = tokenRes.data.token

  const executionId = 289

  // 测试不同的 assignedTo 值
  const assignedToValues = [
    'REDACTED_ACCOUNT',
    'zhangyingchao',
    'admin',
    '',
  ]

  for (const assignedTo of assignedToValues) {
    try {
      const res = await axios.get(`${BASE_URL}/api.php/v1/executions/${executionId}/tasks`, {
        headers: { Token: token },
        params: {
          assignedTo: assignedTo || undefined,
          module: 0,
          recPerPage: 100,
          page: 1,
        },
      })

      const tasks = res.data.tasks || []
      const count = res.data.total || tasks.length

      console.log(`assignedTo=${assignedTo || '(空)'}`)
      console.log(`  count=${count}`)
      tasks.slice(0, 3).forEach((t: any, i: number) => {
        console.log(`  ${i + 1}. [${t.status}] ${t.name}`)
      })
      console.log()
    } catch (e: any) {
      console.log(`assignedTo=${assignedTo}: 错误 ${e.response?.status}`)
    }
  }

  // 测试组合参数
  console.log('=====================================')
  console.log('  组合参数测试')
  console.log('=====================================')
  console.log()

  const combinations = [
    { assignedTo: 'REDACTED_ACCOUNT', status: 'doing' },
    { assignedTo: 'REDACTED_ACCOUNT', status: 'wait' },
    { assignedTo: 'REDACTED_ACCOUNT', status: 'done' },
    { assignedTo: 'REDACTED_ACCOUNT', browseType: 'unclosed' },
  ]

  for (const combo of combinations) {
    try {
      const params: any = {
        assignedTo: combo.assignedTo,
        module: 0,
        recPerPage: 100,
        page: 1,
      }
      if (combo.status) params.status = combo.status
      if (combo.browseType) params.browseType = combo.browseType

      const res = await axios.get(`${BASE_URL}/api.php/v1/executions/${executionId}/tasks`, {
        headers: { Token: token },
        params,
      })

      const tasks = res.data.tasks || []
      const count = res.data.total || tasks.length

      console.log(`组合: ${JSON.stringify(combo)}`)
      console.log(`  count=${count}`)
      console.log()
    } catch (e: any) {
      console.log(`组合 ${JSON.stringify(combo)}: 错误 ${e.response?.status}`)
    }
  }
}

testAssignedTo().catch(console.error)
