import axios from 'axios'
import * as cheerio from 'cheerio'
import dotenv from 'dotenv'

dotenv.config()

const BASE_URL = process.env.ZENTAO_BASE_URL || ''
const ACCOUNT = process.env.ZENTAO_ACCOUNT || ''
const PASSWORD = process.env.ZENTAO_PASSWORD || ''

async function parseApiDoc() {
  console.log('=====================================')
  console.log('  解析禅道 API 文档')
  console.log('=====================================')
  console.log()

  try {
    // 1. 获取 Token
    console.log('【步骤 1】获取 Token...')
    const tokenRes = await axios.post(`${BASE_URL}/api.php/v1/tokens`, {
      account: ACCOUNT,
      password: PASSWORD,
    })
    const token = tokenRes.data.token
    console.log('✅ Token 获取成功')
    console.log()

    // 2. 获取 API 文档页面
    console.log('【步骤 2】获取 API 文档...')
    const res = await axios.get(`${BASE_URL}/dev-api-restapi.html`, {
      headers: { Token: token },
    })

    const html = res.data
    console.log('页面长度:', html.length)
    console.log()

    // 3. 解析 HTML
    const $ = cheerio.load(html)

    // 查找 API 端点
    console.log('【步骤 3】提取 API 端点...')
    console.log()

    // 查找所有面板（每个面板是一个 API）
    const panels = $('.panel')
    console.log(`找到 ${panels.length} 个 API 面板`)
    console.log()

    const apis: any[] = []

    panels.each((i, panel) => {
      const $panel = $(panel)

      // 获取 HTTP 方法
      const method = $panel.find('.http-method').text().trim()

      // 获取路径
      const path = $panel.find('.path').text().trim()

      // 获取标题
      const title = $panel.find('.title').first().text().trim()

      // 获取描述
      const desc = $panel.find('.desc').text().trim()

      if (method && path) {
        apis.push({ method, path, title, desc })
      }
    })

    console.log('=====================================')
    console.log(`  API 列表 (${apis.length} 个)`)
    console.log('=====================================')
    console.log()

    // 筛选与任务相关的 API
    const taskApis = apis.filter(api =>
      api.path.includes('/task') ||
      api.path.includes('/tasks') ||
      api.title.includes('任务')
    )

    console.log('【任务相关 API】')
    console.log()
    taskApis.forEach((api, i) => {
      console.log(`${i + 1}. ${api.method} ${api.path}`)
      console.log(`   标题: ${api.title}`)
      if (api.desc) console.log(`   描述: ${api.desc}`)
      console.log()
    })

    // 查找与 my 相关的 API
    const myApis = apis.filter(api =>
      api.path.includes('/my') ||
      api.title.includes('我的')
    )

    console.log('【我的相关 API】')
    console.log()
    myApis.forEach((api, i) => {
      console.log(`${i + 1}. ${api.method} ${api.path}`)
      console.log(`   标题: ${api.title}`)
      console.log()
    })

    // 输出所有 API（前 20 个）
    console.log('【所有 API（前 20 个）】')
    console.log()
    apis.slice(0, 20).forEach((api, i) => {
      console.log(`${i + 1}. ${api.method} ${api.path}`)
      console.log(`   ${api.title}`)
      console.log()
    })

    // 4. 查找侧边栏菜单
    console.log('【侧边栏菜单】')
    console.log()
    const menuItems = $('.sidebar .tree-item')
    console.log(`菜单项数量: ${menuItems.length}`)

    menuItems.slice(0, 10).each((i, item) => {
      const text = $(item).text().trim()
      console.log(`${i + 1}. ${text}`)
    })

  } catch (error: any) {
    console.error('❌ 错误:', error.message)
    if (error.response) {
      console.error('状态码:', error.response.status)
    }
  }
}

parseApiDoc()
