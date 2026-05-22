import axios from 'axios'
import { wrapper } from 'axios-cookiejar-support'
import { CookieJar } from 'tough-cookie'
import * as cheerio from 'cheerio'
import dotenv from 'dotenv'

dotenv.config()

const BASE_URL = process.env.ZENTAO_BASE_URL || ''
const ACCOUNT = process.env.ZENTAO_ACCOUNT || ''
const PASSWORD = process.env.ZENTAO_PASSWORD || ''

interface ZenTaoTask {
  id: number
  name: string
  status: string
  pri: number
  deadline?: string
  execution?: string
}

async function getTasks() {
  console.log('=====================================')
  console.log('  Token 方式获取禅道任务')
  console.log('=====================================')
  console.log()

  const jar = new CookieJar()
  const http = wrapper(axios.create({
    baseURL: BASE_URL,
    timeout: 30000,
    jar: jar,
    withCredentials: true,
    headers: {
      'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36',
    },
  }))

  try {
    // 1. 获取 Token
    console.log('【步骤 1】获取 Token...')
    const tokenRes = await http.post('/api.php/v1/tokens', {
      account: ACCOUNT,
      password: PASSWORD,
    })
    const token = tokenRes.data.token
    console.log('✅ Token 获取成功:', token.slice(0, 20) + '...')
    console.log()

    // 2. 访问工作台任务页面
    console.log('【步骤 2】获取任务页面...')
    const taskRes = await http.get('/my-work-task-assignedTo.html', {
      headers: {
        'Token': token,
      },
    })
    console.log('页面状态:', taskRes.status)
    console.log('页面路径:', taskRes.request?.path)
    console.log('页面长度:', taskRes.data.length)
    console.log()

    // 3. 解析 HTML
    console.log('【步骤 3】解析任务...')
    const $ = cheerio.load(taskRes.data)
    const tasks: ZenTaoTask[] = []

    // 保存 HTML 用于分析
    console.log('页面标题:', $('title').text())

    // 查找任务表格 - 尝试多种选择器
    const selectors = [
      'table.datatable tbody tr',
      'table.table tbody tr',
      '#taskList tbody tr',
      '.main-table tbody tr',
      'table tbody tr',
    ]

    let foundSelector = ''
    for (const selector of selectors) {
      const rows = $(selector)
      if (rows.length > 0) {
        foundSelector = selector
        console.log(`找到选择器: ${selector}, 行数: ${rows.length}`)
        break
      }
    }

    if (!foundSelector) {
      console.log('未找到任务表格，输出页面内容片段:')
      console.log(taskRes.data.substring(0, 2000))
      return
    }

    // 解析任务行
    $(foundSelector).each((index, element) => {
      const $row = $(element)

      // 跳过表头行
      if ($row.find('th').length > 0) return

      // 提取任务ID
      const idLink = $row.find('a[href*="/task-view-"]').first()
      const href = idLink.attr('href') || ''
      const idMatch = href.match(/task-view-(\d+)/)
      const id = idMatch ? parseInt(idMatch[1]) : 0

      // 提取任务名称
      const name = idLink.text().trim() || $row.find('td').eq(1).text().trim()

      // 提取状态
      const statusCell = $row.find('td').eq(2)
      const statusText = statusCell.text().trim() || statusCell.find('span').text().trim()

      // 提取优先级
      const priText = $row.find('td').eq(3).text().trim()
      const pri = parseInt(priText) || 0

      // 提取截止日期
      const deadline = $row.find('td').eq(4).text().trim() || undefined

      // 提取执行/项目
      const execution = $row.find('td').eq(5).text().trim() || undefined

      if (id && name) {
        tasks.push({
          id,
          name,
          status: statusText,
          pri,
          deadline: deadline || undefined,
          execution: execution || undefined,
        })
      }
    })

    console.log()
    console.log('=====================================')
    console.log(`  任务列表 (${tasks.length} 个)`)
    console.log('=====================================')
    console.log()

    tasks.forEach((t, i) => {
      console.log(`${i + 1}. [${t.status}] ${t.name}`)
      console.log(`   ID: ${t.id} | 优先级: ${t.pri}`)
      if (t.deadline) console.log(`   截止: ${t.deadline}`)
      if (t.execution) console.log(`   执行: ${t.execution}`)
      console.log()
    })

  } catch (error: any) {
    console.error('\n❌ 错误:', error.message)
    if (error.response) {
      console.error('响应状态:', error.response.status)
    }
  }
}

getTasks()
