import axios from 'axios'
import dotenv from 'dotenv'

dotenv.config()

const BASE_URL = process.env.ZENTAO_BASE_URL || ''
const ACCOUNT = process.env.ZENTAO_ACCOUNT || ''
const PASSWORD = process.env.ZENTAO_PASSWORD || ''

async function check() {
  console.log('=====================================')
  console.log('  检查 Dev API 页面')
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
    console.log('✅ Token 获取成功:', token.slice(0, 20) + '...')
    console.log()

    // 2. 尝试访问 dev-api-restapi.html
    console.log('【步骤 2】访问 /zentao/dev-api-restapi.html...')
    const res = await axios.get(`${BASE_URL}/dev-api-restapi.html`, {
      headers: { Token: token },
      timeout: 30000,
    })

    console.log('状态:', res.status)
    console.log('内容类型:', res.headers['content-type'])
    console.log('内容长度:', res.data.length)
    console.log()

    // 3. 检查内容
    if (typeof res.data === 'string') {
      if (res.data.includes('API') || res.data.includes('接口')) {
        console.log('✅ 页面包含 API 相关内容')
      }
      if (res.data.includes('404') || res.data.includes('not found')) {
        console.log('❌ 页面返回 404')
      }

      // 保存内容用于分析
      const fs = await import('fs')
      fs.writeFileSync('dev-api-page.html', res.data)
      console.log('页面内容已保存到 dev-api-page.html')

      // 输出部分内容
      console.log('\n页面内容片段:')
      console.log(res.data.substring(0, 1000))
    }

  } catch (error: any) {
    console.error('❌ 错误:', error.message)
    if (error.response) {
      console.error('状态码:', error.response.status)
      console.error('响应数据:', error.response.data?.substring?.(0, 500) || error.response.data)
    }
  }
}

check()
