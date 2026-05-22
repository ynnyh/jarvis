import { http, tokenManager, ACCOUNT, PASSWORD } from '../src/providers/zentao/request.js'

async function test() {
  const tokenRes = await http.post('/api.php/v1/tokens', { account: ACCOUNT, password: PASSWORD })
  const token = tokenRes.data.token
  tokenManager.setToken(token)
  console.log('Token OK\n')

  // 1. 获取当前用户信息
  console.log('=== 获取当前用户信息 ===')
  const userRes = await http.get('/api.php/v1/user')
  const userId = userRes.data.profile?.id
  console.log(`用户ID: ${userId}, 账号: ${userRes.data.profile?.account}`)
  console.log()

  // 2. 尝试 v2 用户任务接口
  console.log('=== 尝试 v2 /users/{id}/tasks ===')
  try {
    const res = await http.get(`/api.php/v2/users/${userId}/tasks`)
    console.log('✅ 成功:', res.status)
    console.log('数据:', JSON.stringify(res.data).slice(0, 300))
  } catch (err: any) {
    console.log('❌ 失败:', err.response?.status, err.response?.data?.message || err.message)
  }
  console.log()

  // 3. 尝试 v2 /tasks 带参数
  console.log('=== 尝试 v2 /tasks?assignedTo=REDACTED_ACCOUNT ===')
  try {
    const res = await http.get('/api.php/v2/tasks', {
      params: { assignedTo: 'REDACTED_ACCOUNT', status: 'wait,doing' },
    })
    console.log('✅ 成功:', res.status)
    console.log('数据类型:', typeof res.data)
    if (typeof res.data === 'object') {
      console.log('数据:', JSON.stringify(res.data).slice(0, 300))
    } else {
      console.log('数据:', res.data)
    }
  } catch (err: any) {
    console.log('❌ 失败:', err.response?.status, err.response?.data?.message || err.message)
  }
}

test().catch(console.error)
