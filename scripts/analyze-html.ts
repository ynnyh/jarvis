import axios from 'axios'
import { wrapper } from 'axios-cookiejar-support'
import { CookieJar } from 'tough-cookie'
import * as cheerio from 'cheerio'
import dotenv from 'dotenv'

dotenv.config()

const BASE_URL = process.env.ZENTAO_BASE_URL || ''
const ACCOUNT = process.env.ZENTAO_ACCOUNT || ''
const PASSWORD = process.env.ZENTAO_PASSWORD || ''

async function analyze() {
  const jar = new CookieJar()
  const http = wrapper(axios.create({
    baseURL: BASE_URL,
    timeout: 30000,
    jar: jar,
    withCredentials: true,
  }))

  // 1. 获取 Token
  const tokenRes = await http.post('/api.php/v1/tokens', {
    account: ACCOUNT,
    password: PASSWORD,
  })
  const token = tokenRes.data.token

  // 2. 获取任务页面
  const taskRes = await http.get('/my-work-task-assignedTo.html', {
    headers: { 'Token': token },
  })

  const $ = cheerio.load(taskRes.data)

  // 3. 查找任务数据
  console.log('=== 页面结构分析 ===\n')

  // 检查是否有 script 包含任务数据
  const scripts = $('script').map((i, el) => $(el).html()).get()
  console.log(`页面中有 ${scripts.length} 个 script 标签`)

  // 查找包含 task 的 script
  const taskScript = scripts.find(s => s && (s.includes('tasks') || s.includes('taskList')))
  if (taskScript) {
    console.log('找到包含 task 的 script')
    console.log('Script 长度:', taskScript.length)
    console.log('Script 前 500 字符:', taskScript.substring(0, 500))
  }

  // 4. 查找所有 table
  console.log('\n=== 表格分析 ===')
  $('table').each((i, table) => {
    const $table = $(table)
    const className = $table.attr('class') || '无 class'
    const id = $table.attr('id') || '无 id'
    const rows = $table.find('tr').length
    console.log(`Table ${i}: class=${className}, id=${id}, rows=${rows}`)
  })

  // 5. 查找包含任务信息的元素
  console.log('\n=== 任务链接分析 ===')
  const taskLinks = $('a[href*="/task-view-"]')
  console.log(`找到 ${taskLinks.length} 个任务链接`)

  if (taskLinks.length > 0) {
    taskLinks.slice(0, 5).each((i, link) => {
      const $link = $(link)
      console.log(`\n链接 ${i + 1}:`)
      console.log('  href:', $link.attr('href'))
      console.log('  text:', $link.text().trim())
      console.log('  parent:', $link.parent().prop('tagName'))
    })
  }

  // 6. 尝试找到数据属性
  console.log('\n=== 数据属性分析 ===')
  const dataElements = $('[data-id], [data-task-id], [data-task]')
  console.log(`找到 ${dataElements.length} 个带数据属性的元素`)

  // 7. 检查是否有 JSON 数据
  console.log('\n=== JSON 数据查找 ===')
  const jsonMatch = taskRes.data.match(/window\.(tasks|taskList|data)\s*=\s*(\{.*?\});/s)
  if (jsonMatch) {
    console.log('找到 JSON 数据:', jsonMatch[1])
  }

  // 8. 保存完整 HTML 用于分析
  console.log('\n=== 保存 HTML ===')
  const fs = await import('fs')
  fs.writeFileSync('zentao-tasks-page.html', taskRes.data)
  console.log('HTML 已保存到 zentao-tasks-page.html')
}

analyze().catch(console.error)
