import { http, tokenManager, ACCOUNT, PASSWORD } from '../src/providers/zentao/request.js'

async function test() {
  const tokenRes = await http.post('/api.php/v1/tokens', { account: ACCOUNT, password: PASSWORD })
  const token = tokenRes.data.token
  tokenManager.setToken(token)
  console.log('Token OK\n')

  // 测试不同参数组合
  const tests = [
    { name: '无参数', params: {} },
    { name: 'status=assignedtome', params: { status: 'assignedtome' } },
    { name: 'status=wait,doing', params: { status: 'wait,doing' } },
    { name: 'status=undone', params: { status: 'undone' } },
    { name: 'assignedTo=REDACTED_ACCOUNT', params: { assignedTo: 'REDACTED_ACCOUNT' } },
    { name: 'status=assignedtome+assignedTo', params: { status: 'assignedtome', assignedTo: 'REDACTED_ACCOUNT' } },
  ]

  // 用一个有任务的执行来测试
  const execId = 35 // 有177个任务的执行

  for (const t of tests) {
    console.log(`=== ${t.name} ===`)
    try {
      const res = await http.get(`/api.php/v1/executions/${execId}/tasks`, {
        params: { ...t.params, recPerPage: 100 },
      })
      const tasks = res.data.tasks || []
      console.log(`✅ 获取到 ${tasks.length} 个任务`)

      // 统计指派给我的
      const mine = tasks.filter((task: any) => {
        const assigned = typeof task.assignedTo === 'object' ? task.assignedTo?.account : task.assignedTo
        return assigned === 'REDACTED_ACCOUNT'
      })
      console.log(`   其中指派给我的: ${mine.length} 个`)

      if (mine.length > 0) {
        mine.forEach((task: any) => {
          console.log(`   📌 ${task.name}`)
        })
      }
    } catch (err: any) {
      console.log(`❌ 错误: ${err.response?.status} ${err.response?.data?.message || err.message}`)
    }
    console.log()
  }
}

test().catch(console.error)
