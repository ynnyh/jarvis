import axios from 'axios'
import { wrapper } from 'axios-cookiejar-support'
import { CookieJar } from 'tough-cookie'
import * as cheerio from 'cheerio'
import dotenv from 'dotenv'

dotenv.config()

const BASE_URL = process.env.ZENTAO_BASE_URL || ''
const ACCOUNT = process.env.ZENTAO_ACCOUNT || ''
const PASSWORD = process.env.ZENTAO_PASSWORD || ''

async function parse() {
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

  // 2. 获取任务 HTML
  const res = await http.get('/index.php?m=my&f=task&t=html&orderBy=id_desc&recPerPage=100&pageID=1', {
    headers: { 'Token': token },
  })

  const html = res.data
  console.log('HTML 长度:', html.length)

  // 3. 保存 HTML 用于分析
  const fs = await import('fs')
  fs.writeFileSync('zentao-my-tasks.html', html)
  console.log('HTML 已保存到 zentao-my-tasks.html')

  // 4. 解析 HTML
  const $ = cheerio.load(html)

  // 查找任务链接
  const taskLinks = $('a[href*="/task-view-"]')
  console.log('\n找到任务链接:', taskLinks.length)

  if (taskLinks.length > 0) {
    console.log('\n前 10 个任务:')
    taskLinks.slice(0, 10).each((i, el) => {
      const $link = $(el)
      const href = $link.attr('href') || ''
      const idMatch = href.match(/task-view-(\d+)/)
      const id = idMatch ? idMatch[1] : '?'
      const name = $link.text().trim()

      // 查找同行其他单元格
      const $row = $link.closest('tr')
      const status = $row.find('.status, td:nth-child(3)').text().trim()
      const pri = $row.find('.pri, td:nth-child(4)').text().trim()
      const deadline = $row.find('.deadline, td:nth-child(5)').text().trim()

      console.log(`${i + 1}. [${status || '?'}] ${name}`)
      console.log(`   ID: ${id} | 优先级: ${pri || '?'} | 截止: ${deadline || '无'}`)
    })
  }

  // 5. 查找表格结构
  console.log('\n\n表格结构分析:')
  $('table').each((i, table) => {
    const $table = $(table)
    const id = $table.attr('id') || '无 ID'
    const className = $table.attr('class') || '无 class'
    const rows = $table.find('tr').length
    const headers = $table.find('th').map((j, th) => $(th).text().trim()).get()
    console.log(`\nTable ${i}: ${id} (${className})`)
    console.log(`  行数: ${rows}`)
    console.log(`  表头: ${headers.join(', ')}`)
  })
}

parse().catch(console.error)
