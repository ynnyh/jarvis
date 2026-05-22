import { http, tokenManager, ACCOUNT, PASSWORD } from '../src/providers/zentao/request.js'

async function probe() {
  // 1. 获取 Token
  const tokenRes = await http.post('/api.php/v1/tokens', { account: ACCOUNT, password: PASSWORD })
  tokenManager.setToken(tokenRes.data.token)
  console.log('✅ Token 获取成功')

  // 2. 获取产品列表
  console.log('\n--- 测试: 获取产品列表 ---')
  try {
    const res = await http.get('/api.php/v2/products', { params: { recPerPage: 50 } })
    console.log('产品数量:', res.data.products?.length || 0)
    if (res.data.products?.length > 0) {
      console.log('前3个:', JSON.stringify(res.data.products.slice(0, 3), null, 2))
    }
  } catch (e: any) {
    console.log('错误:', e.response?.status, e.message)
  }

  // 3. 获取需求列表
  console.log('\n--- 测试: 获取需求列表 ---')
  try {
    const res = await http.get('/api.php/v2/stories', { params: { recPerPage: 10 } })
    console.log('需求数量:', res.data.stories?.length || 0)
    if (res.data.stories?.length > 0) {
      console.log('前2个:', JSON.stringify(res.data.stories.slice(0, 2), null, 2))
    }
  } catch (e: any) {
    console.log('错误:', e.response?.status, e.message)
  }

  // 4. 尝试用 v1 获取 tasks（不同参数）
  console.log('\n--- 测试: v1 tasks 不同参数 ---')
  try {
    const res = await http.get('/api.php/v1/tasks', { params: { recPerPage: 10, page: 1 } })
    console.log('v1 tasks 数量:', res.data.tasks?.length || 0)
  } catch (e: any) {
    console.log('错误:', e.response?.status, e.message)
  }

  // 5. 尝试获取 todo 列表
  console.log('\n--- 测试: 获取待办列表 ---')
  try {
    const res = await http.get('/api.php/v1/todos', { params: { recPerPage: 50 } })
    console.log('待办数量:', res.data.todos?.length || 0)
    if (res.data.todos?.length > 0) {
      console.log('前3个:', JSON.stringify(res.data.todos.slice(0, 3), null, 2))
    }
  } catch (e: any) {
    console.log('错误:', e.response?.status, e.message)
  }

  // 6. 尝试获取 bug 列表
  console.log('\n--- 测试: 获取 Bug 列表 ---')
  try {
    const res = await http.get('/api.php/v2/bugs', { params: { recPerPage: 10 } })
    console.log('Bug 数量:', res.data.bugs?.length || 0)
    if (res.data.bugs?.length > 0) {
      console.log('前2个:', JSON.stringify(res.data.bugs.slice(0, 2), null, 2))
    }
  } catch (e: any) {
    console.log('错误:', e.response?.status, e.message)
  }

  // 7. 获取某个具体 sprint 的详细信息
  console.log('\n--- 测试: 获取 sprint 详情 ---')
  try {
    const res = await http.get('/api.php/v2/executions/3')
    console.log('Sprint 3 详情:', JSON.stringify(res.data, null, 2).slice(0, 500))
  } catch (e: any) {
    console.log('错误:', e.response?.status, e.message)
  }
}

probe().catch(console.error)
