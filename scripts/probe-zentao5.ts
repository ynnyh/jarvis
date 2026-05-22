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

  // 2. 尝试不同的认证方式获取执行列表

  // 方式 A: Header 中放 Token
  console.log('\n--- 2A. Header Token ---')
  const resA = await axios.get(`${BASE_URL}/api.php/v2/executions`, {
    headers: { 'Token': token },
    params: { status: 'all', recPerPage: 10 },
  })
  console.log('状态:', resA.status, '数据长度:', JSON.stringify(resA.data).length)
  console.log('数据:', JSON.stringify(resA.data).slice(0, 200))

  // 方式 B: URL 参数 token
  console.log('\n--- 2B. URL token 参数 ---')
  const resB = await axios.get(`${BASE_URL}/api.php/v2/executions`, {
    params: { status: 'all', recPerPage: 10, token: token },
  })
  console.log('状态:', resB.status, '数据长度:', JSON.stringify(resB.data).length)
  console.log('数据:', JSON.stringify(resB.data).slice(0, 200))

  // 方式 C: Authorization Bearer
  console.log('\n--- 2C. Authorization Bearer ---')
  const resC = await axios.get(`${BASE_URL}/api.php/v2/executions`, {
    headers: { 'Authorization': `Bearer ${token}` },
    params: { status: 'all', recPerPage: 10 },
  })
  console.log('状态:', resC.status, '数据长度:', JSON.stringify(resC.data).length)
  console.log('数据:', JSON.stringify(resC.data).slice(0, 200))

  // 方式 D: 使用 zentaosid cookie
  console.log('\n--- 2D. 使用 zentaosid ---')
  const cookie = tokenRes.headers['set-cookie']?.find((c: string) => c.includes('zentaosid'))
  console.log('Cookie:', cookie?.slice(0, 50))

  // 3. 尝试获取任务（使用 Token header）
  console.log('\n--- 3. 尝试获取任务列表 ---')
  try {
    const taskRes = await axios.get(`${BASE_URL}/api.php/v2/tasks`, {
      headers: { 'Token': token },
      params: { recPerPage: 10 },
    })
    console.log('任务响应状态:', taskRes.status)
    console.log('任务数据:', JSON.stringify(taskRes.data).slice(0, 500))
  } catch (e: any) {
    console.log('错误:', e.response?.status, e.message)
  }

  // 4. 尝试 v1 接口 /my-tasks
  console.log('\n--- 4. 尝试 /my-tasks ---')
  try {
    const myRes = await axios.get(`${BASE_URL}/api.php/v1/my-tasks`, {
      headers: { 'Token': token },
      params: { recPerPage: 10 },
    })
    console.log('响应:', JSON.stringify(myRes.data).slice(0, 500))
  } catch (e: any) {
    console.log('错误:', e.response?.status, e.message)
  }
}

probe().catch(console.error)
