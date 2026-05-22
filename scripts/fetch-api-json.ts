import axios from 'axios'
import dotenv from 'dotenv'

dotenv.config()

const BASE_URL = process.env.ZENTAO_BASE_URL || ''
const ACCOUNT = process.env.ZENTAO_ACCOUNT || ''
const PASSWORD = process.env.ZENTAO_PASSWORD || ''

async function fetchApiJson() {
  console.log('=====================================')
  console.log('  获取禅道 API JSON 数据')
  console.log('=====================================')
  console.log()

  try {
    // 1. 获取 Token
    const tokenRes = await axios.post(`${BASE_URL}/api.php/v1/tokens`, {
      account: ACCOUNT,
      password: PASSWORD,
    })
    const token = tokenRes.data.token
    console.log('✅ Token 获取成功')
    console.log()

    // 2. 尝试获取 API 的 JSON 数据
    const urls = [
      '/dev-api-restapi.json',
      '/dev-api.json',
      '/api.php/v1/dev-api',
      '/index.php?m=dev&f=restapi&t=json',
      '/index.php?m=dev&f=api&t=json',
    ]

    for (const url of urls) {
      console.log(`尝试: ${url}`)
      try {
        const res = await axios.get(`${BASE_URL}${url}`, {
          headers: { Token: token },
          timeout: 10000,
        })

        console.log('  状态:', res.status)
        console.log('  类型:', res.headers['content-type'])

        if (typeof res.data === 'object') {
          console.log('✅ JSON 数据!')
          console.log('  结构:', Object.keys(res.data))

          // 保存数据
          const fs = await import('fs')
          fs.writeFileSync('api-data.json', JSON.stringify(res.data, null, 2))
          console.log('  已保存到 api-data.json')

          // 显示部分内容
          console.log('\n  内容片段:')
          console.log(JSON.stringify(res.data, null, 2).slice(0, 2000))
          return
        }
      } catch (e: any) {
        console.log('  ❌ 失败:', e.response?.status || e.message)
      }
      console.log()
    }

    console.log('所有尝试都失败了')

  } catch (error: any) {
    console.error('❌ 错误:', error.message)
  }
}

fetchApiJson()
