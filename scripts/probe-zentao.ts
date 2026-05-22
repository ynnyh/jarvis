import { http, tokenManager, ACCOUNT, PASSWORD } from '../src/providers/zentao/request.js'

async function probe() {
  // 1. 获取 Token
  const tokenRes = await http.post('/api.php/v1/tokens', { account: ACCOUNT, password: PASSWORD })
  tokenManager.setToken(tokenRes.data.token)
  console.log('✅ Token 获取成功')

  // 2. 尝试获取所有执行（不筛选状态）
  console.log('\n--- 测试: 获取所有执行 (status=all) ---')
  const allExecRes = await http.get('/api.php/v2/executions', { params: { status: 'all', recPerPage: 100 } })
  console.log('执行数量:', allExecRes.data.executions?.length || 0)

  // 3. 尝试获取项目列表
  console.log('\n--- 测试: 获取项目列表 ---')
  try {
    const projRes = await http.get('/api.php/v2/projects', { params: { status: 'all', recPerPage: 50 } })
    console.log('项目数量:', projRes.data.projects?.length || 0)
    if (projRes.data.projects?.length > 0) {
      console.log('前 3 个项目:', JSON.stringify(projRes.data.projects.slice(0, 3), null, 2))
    }
  } catch (e: any) {
    console.log('项目接口错误:', e.message)
  }

  // 4. 查看 execRes 的完整响应结构
  console.log('\n--- 执行列表响应结构 ---')
  console.log(JSON.stringify(allExecRes.data, null, 2).slice(0, 2000))

  // 5. 尝试直接获取任务列表（v1 API）
  console.log('\n--- 测试: v1 任务列表 ---')
  try {
    const taskRes = await http.get('/api.php/v1/tasks', { params: { limit: 10 } })
    console.log('v1 任务数量:', taskRes.data.tasks?.length || 0)
  } catch (e: any) {
    console.log('v1 任务接口错误:', e.response?.status, e.message)
  }

  // 6. 尝试获取当前用户信息
  console.log('\n--- 测试: 当前用户信息 ---')
  try {
    const userRes = await http.get('/api.php/v1/user')
    console.log('用户信息:', JSON.stringify(userRes.data, null, 2))
  } catch (e: any) {
    console.log('用户信息接口错误:', e.response?.status, e.message)
  }
}

probe().catch(console.error)
