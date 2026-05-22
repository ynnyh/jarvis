import 'dotenv/config'

const BASE = process.env.ZENTAO_BASE_URL.replace(/\/$/, '')
const ACCOUNT = process.env.ZENTAO_ACCOUNT
const PASSWORD = process.env.ZENTAO_PASSWORD

const tk = await fetch(`${BASE}/api.php/v1/tokens`, {
  method: 'POST',
  headers: { 'Content-Type': 'application/json' },
  body: JSON.stringify({ account: ACCOUNT, password: PASSWORD }),
})
const { token } = await tk.json()

// 1. my-work-task 列表里看 9509
const r1 = await fetch(`${BASE}/my-work-task-assignedTo--id_desc.json`, {
  headers: { Token: token, Cookie: 'pagerMyWork=200' },
})
const j1 = await r1.json()
const inner = typeof j1.data === 'string' ? JSON.parse(j1.data) : j1.data
const t9509 = inner.tasks.find(t => t.id === 9509 || t.id === '9509')
console.log('--- my-work-task 列表里 9509 ---')
if (t9509) {
  console.log('mode:', t9509.mode)
  console.log('assignedTo:', t9509.assignedTo)
  console.log('estimate:', t9509.estimate, 'consumed:', t9509.consumed)
  console.log('team:', JSON.stringify(t9509.team))
} else {
  console.log('未找到')
}

// 2. 详情接口
console.log('\n--- /api.php/v1/tasks/9509 详情 ---')
const r2 = await fetch(`${BASE}/api.php/v1/tasks/9509`, { headers: { Token: token } })
const j2 = await r2.json()
const t = j2.task || j2
console.log('mode:', t.mode)
console.log('assignedTo:', typeof t.assignedTo === 'object' ? t.assignedTo?.account : t.assignedTo)
console.log('estimate:', t.estimate, 'consumed:', t.consumed)
console.log('team:', JSON.stringify(t.team, null, 2))
