import { http, tokenManager, ACCOUNT, PASSWORD } from '../src/providers/zentao/request.js'

async function probe() {
  // 1. 获取 Token
  const tokenRes = await http.post('/api.php/v1/tokens', { account: ACCOUNT, password: PASSWORD })
  tokenManager.setToken(tokenRes.data.token)
  console.log('✅ Token 获取成功')

  // 2. 尝试用具体项目ID获取执行/迭代
  const projectIds = [4, 2, 5, 8, 9, 11, 12, 14, 17, 18]
  console.log('\n--- 测试: 用项目ID获取执行列表 ---')
  for (const pid of projectIds) {
    try {
      const res = await http.get(`/api.php/v2/projects/${pid}/executions`, { params: { recPerPage: 50 } })
      const execs = res.data.executions || []
      console.log(`项目 ${pid}: ${execs.length} 个执行`)
      if (execs.length > 0) {
        console.log('  执行:', JSON.stringify(execs.slice(0, 2), null, 2))
      }
    } catch (e: any) {
      console.log(`项目 ${pid}: 错误 ${e.response?.status || e.message}`)
    }
  }

  // 3. 尝试用 sprint ID 直接获取任务
  const sprintIds = [3, 6, 10, 13, 15, 19, 22, 25, 26, 27]
  console.log('\n--- 测试: 用 sprint ID 获取任务列表 ---')
  for (const sid of sprintIds) {
    try {
      const res = await http.get(`/api.php/v2/executions/${sid}/tasks`, { params: { recPerPage: 50 } })
      const tasks = res.data.tasks || []
      console.log(`Sprint ${sid}: ${tasks.length} 个任务`)
      if (tasks.length > 0) {
        console.log('  前2个任务:', JSON.stringify(tasks.slice(0, 2).map((t: any) => ({ id: t.id, name: t.name, status: t.status, assignedTo: t.assignedTo })), null, 2))
        break // 找到有任务的就不继续了
      }
    } catch (e: any) {
      console.log(`Sprint ${sid}: 错误 ${e.response?.status || e.message}`)
    }
  }

  // 4. 尝试获取 "我的任务" 接口（v2）
  console.log('\n--- 测试: 获取我的任务 (v2) ---')
  try {
    const res = await http.get('/api.php/v2/tasks', { params: { assignedTo: 'REDACTED_ACCOUNT', recPerPage: 50 } })
    console.log('我的任务数量:', res.data.tasks?.length || 0)
    if (res.data.tasks?.length > 0) {
      console.log('前3个:', JSON.stringify(res.data.tasks.slice(0, 3).map((t: any) => ({ id: t.id, name: t.name, status: t.status })), null, 2))
    }
  } catch (e: any) {
    console.log('错误:', e.response?.status, e.message)
  }
}

probe().catch(console.error)
