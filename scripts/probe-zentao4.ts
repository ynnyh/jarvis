import { http, tokenManager, ACCOUNT, PASSWORD } from '../src/providers/zentao/request.js'

async function probe() {
  // 1. 获取 Token
  const tokenRes = await http.post('/api.php/v1/tokens', { account: ACCOUNT, password: PASSWORD })
  tokenManager.setToken(tokenRes.data.token)
  console.log('✅ Token 获取成功')

  // 2. 检查 executions 响应的完整结构
  console.log('\n--- 执行列表完整响应 ---')
  const execRes = await http.get('/api.php/v2/executions', { params: { status: 'all', recPerPage: 100 } })
  console.log('响应类型:', typeof execRes.data)
  console.log('响应内容:', JSON.stringify(execRes.data, null, 2))

  // 3. 检查是否有 page 信息
  console.log('\n--- 检查响应头 ---')
  console.log('状态码:', execRes.status)
  console.log('响应头:', JSON.stringify(execRes.headers, null, 2))

  // 4. 尝试不带任何参数
  console.log('\n--- 不带参数获取执行 ---')
  const execRes2 = await http.get('/api.php/v2/executions')
  console.log('响应:', JSON.stringify(execRes2.data, null, 2))

  // 5. 尝试访问禅道内置 API 文档页面看是否有其他接口
  console.log('\n--- 尝试获取模块列表 ---')
  try {
    const res = await http.get('/api.php/v1/modules')
    console.log('模块:', JSON.stringify(res.data, null, 2).slice(0, 1000))
  } catch (e: any) {
    console.log('错误:', e.response?.status, e.message)
  }
}

probe().catch(console.error)
