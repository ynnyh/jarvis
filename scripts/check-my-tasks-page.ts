import axios from 'axios'
import { wrapper } from 'axios-cookiejar-support'
import { CookieJar } from 'tough-cookie'
import * as cheerio from 'cheerio'
import dotenv from 'dotenv'

dotenv.config()

const BASE_URL = process.env.ZENTAO_BASE_URL || ''
const ACCOUNT = process.env.ZENTAO_ACCOUNT || ''
const PASSWORD = process.env.ZENTAO_PASSWORD || ''

async function check() {
  const jar = new CookieJar()
  const http = wrapper(axios.create({
    baseURL: BASE_URL,
    timeout: 30000,
    jar: jar,
    withCredentials: true,
  }))

  // 获取 Token
  const tokenRes = await http.post('/api.php/v1/tokens', {
    account: ACCOUNT,
    password: PASSWORD,
  })
  const token = tokenRes.data.token

  // 获取页面
  const res = await http.get('/my-work-task-assignedTo.html', {
    headers: { 'Token': token },
  })

  const html = res.data
  console.log('HTML 长度:', html.length)

  // 保存用于分析
  const fs = await import('fs')
  fs.writeFileSync('my-tasks-full.html', html)
  console.log('已保存到 my-tasks-full.html')

  const $ = cheerio.load(html)

  // 检查页面标题
  console.log('\n页面标题:', $('title').text())

  // 检查是否有 zin 数据
  const zinData = html.match(/window\.zin\s*=\s*(\{.*?\});/s)
  if (zinData) {
    console.log('\n找到 zin 数据')
  }

  // 检查是否有 data 属性
  console.log('\n检查 data 属性:')
  const dataElements = $('[data-id], [data-task]')
  console.log('带 data 属性的元素:', dataElements.length)

  // 检查是否有 JSON 数据
  const jsonData = html.match(/"tasks":\s*(\[.*?\])/s)
  if (jsonData) {
    console.log('\n找到 tasks JSON 数据')
    try {
      const tasks = JSON.parse(jsonData[1])
      console.log('任务数量:', tasks.length)
    } catch (e) {
      console.log('JSON 解析失败')
    }
  }

  // 检查 script 标签内容
  console.log('\nScript 标签分析:')
  $('script').each((i, el) => {
    const content = $(el).html() || ''
    if (content.includes('task') || content.includes('Task')) {
      console.log(`Script ${i}: 包含 task 相关代码，长度 ${content.length}`)
      if (content.length < 1000) {
        console.log('  内容:', content.substring(0, 200))
      }
    }
  })

  // 检查是否有 table
  console.log('\n表格分析:')
  $('table').each((i, table) => {
    const $table = $(table)
    console.log(`Table ${i}: id=${$table.attr('id') || '无'}, class=${$table.attr('class') || '无'}`)
    console.log(`  行数: ${$table.find('tr').length}`)
  })

  // 检查是否有特定的任务列表容器
  console.log('\n任务列表容器:')
  const containers = [
    '#taskList',
    '.task-list',
    '[data-module="task"]',
    '.main-content',
    '#main',
  ]
  for (const selector of containers) {
    const el = $(selector)
    if (el.length > 0) {
      console.log(`${selector}: 找到，长度 ${el.html()?.length || 0}`)
    }
  }
}

check().catch(console.error)
