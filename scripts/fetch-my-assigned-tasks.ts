import axios from 'axios'
import * as cheerio from 'cheerio'
import dotenv from 'dotenv'

dotenv.config()

const BASE_URL = process.env.ZENTAO_BASE_URL || ''
const ACCOUNT = process.env.ZENTAO_ACCOUNT || ''
const PASSWORD = process.env.ZENTAO_PASSWORD || ''

interface TaskInfo {
  id: number
  name: string
  pri: number
  status: string
  deadline: string
  assignedTo: string
  execution: string
  project: string
}

async function getToken(): Promise<string> {
  const res = await axios.post(`${BASE_URL}/api.php/v1/tokens`, {
    account: ACCOUNT,
    password: PASSWORD,
  })
  return res.data.token
}

function parseTasksFromHtml(html: string): TaskInfo[] {
  const $ = cheerio.load(html)
  const tasks: TaskInfo[] = []

  // The task data is in the zui-create-dtable attribute
  const dtableAttr = $('[zui-create-dtable]').attr('zui-create-dtable')
  if (!dtableAttr) return tasks

  const decoded = dtableAttr
    .replace(/&quot;/g, '"')
    .replace(/&amp;/g, '&')
    .replace(/&lt;/g, '<')
    .replace(/&gt;/g, '>')

  // Extract the data array
  const dataStart = decoded.indexOf('"data":[{')
  if (dataStart === -1) return tasks

  const arrayStart = dataStart + 7 // position of '['
  let depth = 0
  let arrayEnd = arrayStart
  for (let i = arrayStart; i < decoded.length; i++) {
    if (decoded[i] === '[') depth++
    if (decoded[i] === ']') { depth--; if (depth === 0) { arrayEnd = i + 1; break } }
  }

  try {
    const data = JSON.parse(decoded.substring(arrayStart, arrayEnd))
    return data.map((t: any) => ({
      id: t.id,
      name: t.name,
      pri: t.pri,
      status: t.status,
      deadline: t.deadline || '',
      assignedTo: t.assignedTo || '',
      execution: t.execution || '',
      project: t.project || '',
    }))
  } catch {
    return tasks
  }
}

async function main() {
  console.log('=== 获取禅道指派给我的任务 ===\n')

  // 1. 获取 Token
  console.log('获取 Token...')
  const token = await getToken()
  console.log('Token 获取成功\n')

  // 2. 获取第一页，拿到总数
  const firstPageUrl = `${BASE_URL}/my-work-task-assignedTo--id_desc.html`
  console.log('获取第一页:', firstPageUrl)
  const firstRes = await axios.get(firstPageUrl, {
    headers: { 'Token': token },
  })

  const firstTasks = parseTasksFromHtml(firstRes.data)
  console.log('第一页任务数:', firstTasks.length)

  // 从 tab 标签获取总任务数
  const $ = cheerio.load(firstRes.data)
  let totalCount = 0
  $('[data-id="assignedTo"] .label').each((_, el) => {
    const text = $(el).text().trim()
    const num = parseInt(text)
    if (!isNaN(num) && num > totalCount) totalCount = num
  })
  // fallback: look for the label next to the assignedTo tab
  if (!totalCount) {
    const labelEl = $('a.active[data-id="assignedTo"] .label, a[data-id="assignedTo"].active .label')
    totalCount = parseInt(labelEl.text().trim()) || 0
  }
  console.log('Tab 上显示总数:', totalCount)

  const perPage = 20
  const totalPages = Math.ceil(totalCount / perPage)
  console.log('总页数:', totalPages, '\n')

  // 3. 获取后续页面
  const allTasks = [...firstTasks]
  for (let page = 2; page <= totalPages; page++) {
    const offset = (page - 1) * perPage
    const pageUrl = `${BASE_URL}/my-work-task-assignedTo--id_desc-${offset}_${perPage}.html`
    console.log(`获取第 ${page} 页: offset=${offset}`)
    try {
      const res = await axios.get(pageUrl, {
        headers: { 'Token': token },
      })
      const pageTasks = parseTasksFromHtml(res.data)
      console.log(`  → ${pageTasks.length} 个任务`)
      allTasks.push(...pageTasks)
    } catch (e: any) {
      console.log(`  → 获取失败:`, e.message)
    }
  }

  // 4. 去重（以防万一）
  const deduped = new Map<number, TaskInfo>()
  allTasks.forEach(t => deduped.set(t.id, t))

  console.log(`\n=== 结果汇总 ===`)
  console.log(`总计获取 ${deduped.size} 个任务（去重后）\n`)

  // 按状态分组
  const byStatus: Record<string, number> = {}
  deduped.forEach(t => { byStatus[t.status] = (byStatus[t.status] || 0) + 1 })
  console.log('状态分布:', byStatus)

  // 输出所有任务
  console.log('\n=== 全部任务明细 ===\n')
  let i = 0
  deduped.forEach(t => {
    i++
    console.log(`${i}. [#${t.id}] ${t.name}`)
    console.log(`   状态: ${t.status} | 优先级: ${t.pri} | 截止: ${t.deadline || '无'}`)
  })

  // 保存到文件
  const output = Array.from(deduped.values())
  const fs = await import('fs')
  fs.writeFileSync('my-assigned-tasks.json', JSON.stringify(output, null, 2), 'utf-8')
  console.log('\n已保存到 my-assigned-tasks.json')
}

main().catch(console.error)
