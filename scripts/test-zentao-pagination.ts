import { http, tokenManager, ACCOUNT, PASSWORD } from '../src/providers/zentao/request.js'

async function test() {
  const tokenRes = await http.post('/api.php/v1/tokens', { account: ACCOUNT, password: PASSWORD })
  const token = tokenRes.data.token
  tokenManager.setToken(token)
  console.log('Token OK\n')

  // 测试 executions 分页
  console.log('=== 测试 executions 分页 ===')
  const allExecutions: any[] = []
  let page = 1
  let totalPages = 1

  while (page <= totalPages) {
    const res = await http.get('/api.php/v1/executions', {
      params: { status: 'all', recPerPage: 100, page },
    })

    const executions = res.data.executions || []
    allExecutions.push(...executions)

    console.log(`第 ${page} 页: ${executions.length} 个执行`)
    console.log('响应键:', Object.keys(res.data))
    console.log('total:', res.data.total, 'limit:', res.data.limit, 'page:', res.data.page)

    if (page === 1) {
      totalPages = Math.ceil((res.data.total || 0) / (res.data.limit || 20))
      console.log(`预计总页数: ${totalPages}`)
    }

    page++
    if (page > 5) break // 安全限制
  }

  console.log(`\n总共获取到 ${allExecutions.length} 个执行`)
  console.log('执行ID范围:', allExecutions.map((e: any) => e.id).sort((a, b) => a - b).slice(0, 10), '...')
}

test().catch(console.error)
