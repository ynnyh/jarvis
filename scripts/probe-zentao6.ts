import axios from 'axios'
import dotenv from 'dotenv'

dotenv.config()

const BASE_URL = process.env.ZENTAO_BASE_URL || ''
const ACCOUNT = process.env.ZENTAO_ACCOUNT || ''
const PASSWORD = process.env.ZENTAO_PASSWORD || ''

async function probe() {
  // 1. 获取 Token
  console.log('--- 1. 获取 Token ---')
  const tokenRes = await axios.post(`${BASE_URL}/api.php/v1/tokens`, {
    account: ACCOUNT,
    password: PASSWORD,
  })
  const token = tokenRes.data.token
  console.log('Token:', token.slice(0, 20) + '...')

  // 2. 尝试截图中的端点格式: /api.php/v2/executions/:id/tasks
  // 先尝试几个 sprint ID
  const sprintIds = [3, 6, 10, 13, 15, 19, 22, 25, 26, 27]

  console.log('\n--- 2. 测试 /api.php/v2/executions/:id/tasks ---')
  for (const sid of sprintIds) {
    try {
      const res = await axios.get(`${BASE_URL}/api.php/v2/executions/${sid}/tasks`, {
        headers: { 'Token': token },
        params: { recPerPage: 50 },
      })
      console.log(`Sprint ${sid}: 状态=${res.status}, 数据长度=${JSON.stringify(res.data).length}`)
      if (res.data && JSON.stringify(res.data).length > 10) {
        console.log('  数据:', JSON.stringify(res.data).slice(0, 300))
      }
    } catch (e: any) {
      console.log(`Sprint ${sid}: 错误 ${e.response?.status} ${e.message}`)
    }
  }

  // 3. 尝试不带 api.php 前缀
  console.log('\n--- 3. 测试 /v2/executions/:id/tasks (无 api.php) ---')
  try {
    const res = await axios.get(`${BASE_URL}/v2/executions/3/tasks`, {
      headers: { 'Token': token },
      params: { recPerPage: 10 },
    })
    console.log('状态:', res.status, '数据:', JSON.stringify(res.data).slice(0, 300))
  } catch (e: any) {
    console.log('错误:', e.response?.status, e.message)
  }

  // 4. 尝试 /api.php/v1/executions/:id/tasks
  console.log('\n--- 4. 测试 /api.php/v1/executions/:id/tasks ---')
  try {
    const res = await axios.get(`${BASE_URL}/api.php/v1/executions/3/tasks`, {
      headers: { 'Token': token },
      params: { recPerPage: 10 },
    })
    console.log('状态:', res.status, '数据:', JSON.stringify(res.data).slice(0, 300))
  } catch (e: any) {
    console.log('错误:', e.response?.status, e.message)
  }

  // 5. 尝试直接 /executions/3/tasks (完全按照截图)
  console.log('\n--- 5. 测试 /executions/:id/tasks (完全按截图) ---')
  try {
    const res = await axios.get(`${BASE_URL}/executions/3/tasks`, {
      headers: { 'Token': token },
      params: { recPerPage: 10 },
    })
    console.log('状态:', res.status, '数据:', JSON.stringify(res.data).slice(0, 300))
  } catch (e: any) {
    console.log('错误:', e.response?.status, e.message)
  }

  // 6. 尝试获取任务详情 /api.php/v2/tasks/:id
  console.log('\n--- 6. 测试 /api.php/v2/tasks/:id ---')
  try {
    const res = await axios.get(`${BASE_URL}/api.php/v2/tasks/1`, {
      headers: { 'Token': token },
    })
    console.log('状态:', res.status, '数据:', JSON.stringify(res.data).slice(0, 300))
  } catch (e: any) {
    console.log('错误:', e.response?.status, e.message)
  }
}

probe().catch(console.error)
