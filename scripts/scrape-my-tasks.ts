import axios from 'axios'
import { wrapper } from 'axios-cookiejar-support'
import { CookieJar } from 'tough-cookie'
import * as cheerio from 'cheerio'
import dotenv from 'dotenv'

dotenv.config()

const BASE_URL = process.env.ZENTAO_BASE_URL || ''
const ACCOUNT = process.env.ZENTAO_ACCOUNT || ''
const PASSWORD = process.env.ZENTAO_PASSWORD || ''

async function main() {
  const jar = new CookieJar()
  const http = wrapper(axios.create({
    baseURL: BASE_URL,
    timeout: 30000,
    jar,
    withCredentials: true,
    headers: { 'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36' },
  }))

  // 1. 获取登录页（拿 session cookie）
  console.log('【步骤 1】获取登录页...')
  await http.get('/user-login.html')

  // 2. 提交登录表单
  console.log('【步骤 2】登录...')
  const loginRes = await http.post('/user-login.html', {
    account: ACCOUNT,
    password: PASSWORD,
    keepLogin: 'on',
  }, {
    headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
    maxRedirects: 5,
  })
  console.log('登录状态:', loginRes.status)
  const loggedIn = loginRes.data.includes('我的地盘') || loginRes.data.includes('工作台') || loginRes.request?.path?.includes('my')
  console.log('登录成功:', loggedIn)

  // 如果没直接跳转，尝试访问 /my/ 验证
  if (!loggedIn) {
    const myRes = await http.get('/my/')
    const checkLogin = myRes.data.includes(ACCOUNT) || myRes.data.includes('我的地盘')
    console.log('/my/ 验证:', checkLogin)
    if (!checkLogin) {
      console.error('登录失败！')
      return
    }
  }

  // 3. 获取"指派给我"页面
  console.log('\n【步骤 3】获取指派给我的任务...')
  const taskRes = await http.get('/my-work-task-assignedTo.html')
  const html = taskRes.data
  console.log('页面长度:', html.length)
  console.log('包含 task-view:', html.includes('task-view'))

  // 4. 解析 HTML
  const $ = cheerio.load(html)
  const tasks: any[] = []

  $('a[href*="task-view-"]').each((i, el) => {
    const $link = $(el)
    const href = $link.attr('href') || ''
    const idMatch = href.match(/task-view-(\d+)/)
    if (!idMatch) return

    const id = parseInt(idMatch[1])
    const name = $link.text().trim()
    if (!name || !id) return

    const $row = $link.closest('tr')
    const cells = $row.find('td')

    tasks.push({
      id,
      name,
      pri: cells.eq(2).text().trim(),
      status: cells.eq(3).text().trim(),
      deadline: cells.eq(4).text().trim(),
      project: cells.eq(5).text().trim(),
    })
  })

  console.log(`\n共找到 ${tasks.length} 个任务\n`)
  tasks.forEach((t, i) => {
    console.log(`${i + 1}. [#${t.id}] ${t.name}`)
    console.log(`   优先级: ${t.pri} | 状态: ${t.status} | 截止: ${t.deadline} | 项目: ${t.project}`)
  })
}

main().catch(console.error)
