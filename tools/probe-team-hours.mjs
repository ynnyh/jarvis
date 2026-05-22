import 'dotenv/config'
const BASE = process.env.ZENTAO_BASE_URL.replace(/\/$/, '')
const ACCOUNT = process.env.ZENTAO_ACCOUNT
const tk = await fetch(`${BASE}/api.php/v1/tokens`, {
  method: 'POST',
  headers: { 'Content-Type': 'application/json' },
  body: JSON.stringify({ account: ACCOUNT, password: process.env.ZENTAO_PASSWORD }),
})
const { token } = await tk.json()

// 看 9975 的列表数据
const r1 = await fetch(`${BASE}/my-work-task-assignedTo--id_desc.json`, {
  headers: { Token: token, Cookie: 'pagerMyWork=200' },
})
const j1 = await r1.json()
const inner = typeof j1.data === 'string' ? JSON.parse(j1.data) : j1.data

for (const id of [9975, 10238, 10193, 10200, 10108]) {
  const t = inner.tasks.find(t => Number(t.id) === id)
  if (!t) { console.log(id, '未找到'); continue }
  console.log(`\n=== ${id} ${t.name?.slice(0,30)} ===`)
  console.log('mode:', t.mode, '/ assignedTo:', typeof t.assignedTo === 'object' ? t.assignedTo?.account : t.assignedTo)
  console.log('整体: estimate=', t.estimate, ' consumed=', t.consumed, ' left=', t.left)
  if (Array.isArray(t.team)) {
    console.log('team:')
    t.team.forEach(m => console.log('  ', m.account.padEnd(15), 'est='+m.estimate, 'con='+m.consumed, 'left='+m.left, 'st='+m.status))
    const mine = t.team.find(m => m.account === ACCOUNT)
    if (mine) console.log('  >>> 我个人: est='+mine.estimate, 'con='+mine.consumed, 'left='+mine.left)
  } else {
    console.log('无 team 字段（=单人任务）')
  }
}
