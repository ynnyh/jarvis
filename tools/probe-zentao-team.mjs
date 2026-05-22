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

const r = await fetch(`${BASE}/my-work-task-assignedTo--id_desc.json`, {
  headers: { Token: token, Cookie: 'pagerMyWork=200' },
})
const json = await r.json()
const inner = typeof json.data === 'string' ? JSON.parse(json.data) : json.data
const tasks = inner.tasks

// 找一个 team 含 me 的任务，打印 team 字段
const t = tasks.find(t => Array.isArray(t.team) && t.team.some(m => (typeof m === 'object' ? m.account : m) === ACCOUNT))
console.log('---团队任务示例---')
console.log('id:', t.id, 'name:', t.name)
console.log('assignedTo:', t.assignedTo)
console.log('team:', JSON.stringify(t.team, null, 2))
console.log('mode:', t.mode)

// 计算今天(5-21)的"我"的所有任务（assignee=me 或 team 含 me）
const today = '2026-05-21'
const isMine = t => {
  const a = typeof t.assignedTo === 'object' ? t.assignedTo?.account : t.assignedTo
  if (a === ACCOUNT) return true
  if (Array.isArray(t.team)) {
    return t.team.some(m => (typeof m === 'object' ? m.account : m) === ACCOUNT)
  }
  return false
}
const mine = tasks.filter(isMine).filter(t => t.status !== 'closed' && t.status !== 'cancel' && t.status !== 'done')
console.log('\n=== 修复后我的任务总数:', mine.length)
const overdue = mine.filter(t => t.deadline && t.deadline.length >= 10 && !t.deadline.startsWith('2099') && t.deadline.slice(0, 10) < today)
console.log('=== 逾期:', overdue.length)
overdue.forEach(t => console.log('  ', t.id, t.deadline, '['+t.status+']', t.name.slice(0, 50)))

// 7 天内
const days = s => {
  const d = new Date(s); d.setHours(0,0,0,0)
  const tod = new Date(today); tod.setHours(0,0,0,0)
  return Math.round((d - tod) / 86400000)
}
const soon = mine.filter(t => t.deadline && t.deadline.length >= 10 && days(t.deadline) >= 1 && days(t.deadline) <= 7)
console.log('=== 7 天内:', soon.length)
soon.forEach(t => console.log('  ', t.id, t.deadline, 'days='+days(t.deadline), t.name.slice(0, 50)))
