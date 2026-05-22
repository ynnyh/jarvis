const BASE = 'http://REDACTED_DOMAIN/zentao'
const ACCOUNT = 'REDACTED_ACCOUNT'

async function main() {
  // 1. 登录获取 token
  const tokenResp = await fetch(`${BASE}/api.php/v1/tokens`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ account: ACCOUNT, password: 'REDACTED_PASSWORD' }),
  })
  const { token } = await tokenResp.json() as any
  const headers = { 'Token': token }
  console.log('登录成功，Token:', token.slice(0, 10) + '...')

  // 2. 获取所有 doing 状态的 executions
  const execResp = await fetch(`${BASE}/api.php/v1/executions?status=doing&recPerPage=100`, { headers })
  const execData = await execResp.json() as any
  const executions = execData.executions || []
  console.log(`共 ${executions.length} 个进行中的执行\n`)

  // 3. 遍历每个 execution，查找我的任务
  const myTasks: any[] = []
  for (const exec of executions) {
    const taskResp = await fetch(`${BASE}/api.php/v1/executions/${exec.id}/tasks?recPerPage=100`, { headers })
    const taskData = await taskResp.json() as any
    const tasks = taskData.tasks || []
    
    for (const t of tasks) {
      const assigned = typeof t.assignedTo === 'object' ? t.assignedTo?.account : t.assignedTo
      if (assigned === ACCOUNT && t.status !== 'closed' && t.status !== 'cancel') {
        myTasks.push({ ...t, executionName: exec.name, executionId: exec.id })
      }
    }
  }

  // 4. 输出结果
  const statusMap: Record<string, string> = { wait: '未开始', doing: '进行中', done: '已完成', closed: '已关闭' }
  
  console.log(`📋 指派给 ${ACCOUNT} 的任务（共 ${myTasks.length} 条）\n`)
  myTasks.forEach(t => {
    console.log(`  [#${t.id}] ${t.name}`)
    console.log(`     状态: ${statusMap[t.status] || t.status} | 优先级: ${t.pri} | 截止: ${t.deadline || '无'}`)
    console.log(`     所属执行: ${t.executionName}`)
  })
}

main().catch(err => {
  console.error('失败:', err)
  process.exit(1)
})
