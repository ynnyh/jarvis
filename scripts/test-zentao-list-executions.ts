import { http, tokenManager, ACCOUNT, PASSWORD } from '../src/providers/zentao/request.js'

async function test() {
  const tokenRes = await http.post('/api.php/v1/tokens', { account: ACCOUNT, password: PASSWORD })
  const token = tokenRes.data.token
  tokenManager.setToken(token)
  console.log('Token OK\n')

  // 获取所有 execution
  let allExecutions: any[] = []
  let page = 1
  let totalPages = 1

  while (page <= totalPages) {
    const res = await http.get('/api.php/v1/executions', {
      params: { status: 'all', recPerPage: 100, page },
    })
    const executions = res.data.executions || []
    allExecutions.push(...executions)

    if (page === 1) {
      totalPages = Math.ceil((res.data.total || 0) / (res.data.limit || 20))
    }
    page++
  }

  console.log(`=== 共 ${allExecutions.length} 个 execution ===\n`)

  // 按状态分组
  const byStatus: Record<string, any[]> = {}
  for (const e of allExecutions) {
    const s = e.status || 'unknown'
    if (!byStatus[s]) byStatus[s] = []
    byStatus[s].push(e)
  }

  // 输出每个状态
  for (const [status, list] of Object.entries(byStatus)) {
    console.log(`\n【${status}】共 ${list.length} 个`)
    console.log('-'.repeat(80))
    list.forEach((e: any, i: number) => {
      console.log(`${i + 1}. [ID:${e.id}] ${e.name}`)
      console.log(`   项目: ${e.projectName || e.project || 'N/A'} | 开始: ${e.begin || 'N/A'} | 结束: ${e.end || 'N/A'}`)
    })
  }
}

test().catch(console.error)
