import { http, tokenManager, ACCOUNT, PASSWORD } from '../src/providers/zentao/request.js'

async function test() {
  const tokenRes = await http.post('/api.php/v1/tokens', { account: ACCOUNT, password: PASSWORD })
  const token = tokenRes.data.token
  tokenManager.setToken(token)
  console.log('Token OK\n')

  // 获取所有项目
  const allProjects: any[] = []
  let page = 1
  let totalPages = 1

  while (page <= totalPages) {
    const res = await http.get('/api.php/v1/projects', {
      params: { page, limit: 100 },
    })
    const projects = res.data.projects || []
    allProjects.push(...projects)

    if (page === 1) {
      totalPages = Math.ceil((res.data.total || 0) / (res.data.limit || 20))
      console.log(`总项目数: ${res.data.total}, 每页: ${res.data.limit}, 总页数: ${totalPages}`)
    }
    page++
  }

  console.log(`\n=== 共 ${allProjects.length} 个项目 ===\n`)

  // 按状态分组
  const byStatus: Record<string, any[]> = {}
  for (const p of allProjects) {
    const s = p.status || 'unknown'
    if (!byStatus[s]) byStatus[s] = []
    byStatus[s].push(p)
  }

  // 输出每个状态
  for (const [status, list] of Object.entries(byStatus)) {
    console.log(`\n【${status}】共 ${list.length} 个`)
    console.log('-'.repeat(100))
    list.forEach((p: any, i: number) => {
      console.log(`${i + 1}. [ID:${p.id}] ${p.name}`)
      console.log(`   编号: ${p.code || '无'} | 模型: ${p.model || 'N/A'} | 开始: ${p.begin || 'N/A'} | 结束: ${p.end || 'N/A'}`)
      console.log(`   父项目: ${p.parent || '无'} | 路径: ${p.path || 'N/A'}`)
    })
  }
}

test().catch(console.error)
