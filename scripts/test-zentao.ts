import { http, tokenManager, ACCOUNT, PASSWORD } from '../src/providers/zentao/request.js'

console.log('=====================================')
console.log('  ZenTao API 测试脚本 - 只查指派给我的')
console.log('=====================================')
console.log()

async function test() {
  try {
    // 1. 测试 Token 获取
    console.log('【测试 1】获取 Token')
    console.log('-------------------------------------')
    const tokenRes = await http.post('/api.php/v1/tokens', {
      account: ACCOUNT,
      password: PASSWORD,
    })
    const token = tokenRes.data.token
    tokenManager.setToken(token)
    console.log(`✅ Token 获取成功: ${token.slice(0, 15)}...`)
    console.log()

    // 2. 测试获取指派给我的任务（v2 API）
    console.log('【测试 2】获取指派给我的任务')
    console.log('-------------------------------------')
    const taskRes = await http.get('/api.php/v2/tasks', {
      params: {
        assignedTo: 'REDACTED_ACCOUNT',
        status: 'undone',
        recPerPage: 100,
      },
    })
    const tasks = taskRes.data.tasks || []
    console.log(`✅ 获取到 ${tasks.length} 个指派给我的任务`)
    if (tasks.length > 0) {
      console.log('\n任务列表:')
      tasks.slice(0, 10).forEach((t: any, i: number) => {
        console.log(`  ${i + 1}. [${t.status}] ${t.name} (优先级: ${t.pri})`)
        console.log(`     截止: ${t.deadline || '无'} | 预计: ${t.estimate || 0}h`)
      })
    }
    console.log()

    // 3. 获取所有状态的任务（包括已完成的）
    console.log('【测试 3】获取指派给我的所有任务（含已完成）')
    console.log('-------------------------------------')
    const allTaskRes = await http.get('/api.php/v2/tasks', {
      params: {
        assignedTo: 'REDACTED_ACCOUNT',
        recPerPage: 100,
      },
    })
    const allTasks = allTaskRes.data.tasks || []
    console.log(`✅ 获取到 ${allTasks.length} 个任务（所有状态）`)
    if (allTasks.length > 0) {
      const statusCount: Record<string, number> = {}
      allTasks.forEach((t: any) => {
        statusCount[t.status] = (statusCount[t.status] || 0) + 1
      })
      console.log('状态分布:', statusCount)
    }
    console.log()

    // 4. 如果有任务，测试获取详情
    if (tasks.length > 0) {
      const firstTask = tasks[0]
      console.log(`【测试 4】获取任务 "${firstTask.name}" 的详情`)
      console.log('-------------------------------------')
      const detailRes = await http.get(`/api.php/v2/tasks/${firstTask.id}`)
      const detail = detailRes.data.task || detailRes.data
      console.log('✅ 任务详情获取成功')
      console.log(`   描述: ${(detail.desc || '').slice(0, 100)}...`)
      console.log(`   创建者: ${detail.openedBy || '未知'}`)
      console.log(`   指派给: ${detail.assignedTo || '未指派'}`)
      console.log()
    }

    console.log('=====================================')
    console.log('  测试完成 ✅')
    console.log('=====================================')

  } catch (error: any) {
    console.error()
    console.error('=====================================')
    console.error('  测试失败 ❌')
    console.error('=====================================')
    console.error(`错误: ${error.message}`)
    if (error.response) {
      console.error(`状态码: ${error.response.status}`)
      console.error(`响应数据:`, JSON.stringify(error.response.data, null, 2))
    }
    process.exit(1)
  }
}

test()
