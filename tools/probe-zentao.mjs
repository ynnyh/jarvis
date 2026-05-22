import 'dotenv/config'

const BASE = process.env.ZENTAO_BASE_URL.replace(/\/$/, '')
const ACCOUNT = process.env.ZENTAO_ACCOUNT
const PASSWORD = process.env.ZENTAO_PASSWORD

const UA = 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36'

const tk = await fetch(`${BASE}/api.php/v1/tokens`, {
  method: 'POST',
  headers: { 'Content-Type': 'application/json', 'User-Agent': UA },
  body: JSON.stringify({ account: ACCOUNT, password: PASSWORD }),
})
const { token } = await tk.json()

const headers = { 'Token': token, 'Cookie': 'pagerMyWork=200', 'User-Agent': UA }

const endpoints = [
  'my-work-task.json',
  'my-work-task--id_desc.json',
  'my-work-task-assignedTo--id_desc.json',
  'my-work-task-finishedBy--id_desc.json',
  'my-work-task-myInvolved--id_desc.json',
  'my-work-task-involved--id_desc.json',
  'my-task.json',
  'my-task-assignedTo--id_desc.json',
]

for (const ep of endpoints) {
  const url = `${BASE}/${ep}`
  try {
    const r = await fetch(url, { headers })
    const status = r.status
    let n = '-'
    let mineExtra = ''
    if (status === 200) {
      const json = await r.json().catch(() => null)
      if (json?.data) {
        const inner = typeof json.data === 'string' ? JSON.parse(json.data) : json.data
        if (Array.isArray(inner.tasks)) {
          n = inner.tasks.length
          const mine = inner.tasks.filter(t => {
            const a = typeof t.assignedTo === 'object' ? t.assignedTo?.account : t.assignedTo
            return a === ACCOUNT
          })
          const teamMember = inner.tasks.filter(t => Array.isArray(t.team) && t.team.some(m => (typeof m === 'object' ? m.account : m) === ACCOUNT))
          mineExtra = ` mine=${mine.length} team=${teamMember.length}`
        }
      }
    }
    console.log(ep.padEnd(45), 'HTTP', status, 'tasks=', n, mineExtra)
  } catch (e) {
    console.log(ep.padEnd(45), 'ERR', e.message)
  }
}
