import axios from 'axios'
import dotenv from 'dotenv'

dotenv.config()

const BASE_URL = process.env.ZENTAO_BASE_URL || ''
const ACCOUNT = process.env.ZENTAO_ACCOUNT || ''
const PASSWORD = process.env.ZENTAO_PASSWORD || ''

async function parseApiData() {
  console.log('=====================================')
  console.log('  解析禅道 API 数据')
  console.log('=====================================')
  console.log()

  try {
    // 1. 获取 Token
    const tokenRes = await axios.post(`${BASE_URL}/api.php/v1/tokens`, {
      account: ACCOUNT,
      password: PASSWORD,
    })
    const token = tokenRes.data.token

    // 2. 获取 API JSON
    const res = await axios.get(`${BASE_URL}/dev-api-restapi.json`, {
      headers: { Token: token },
    })

    // 3. 解析数据
    const responseData = res.data
    const apiData = JSON.parse(responseData.data)

    console.log('API 文档标题:', apiData.title)
    console.log('选中模块:', apiData.selectedModule)
    console.log()

    // 检查是否有 apiList
    if (apiData.apiList) {
      console.log('【API 列表】')
      console.log(`数量: ${apiData.apiList.length}`)
      console.log()

      apiData.apiList.forEach((api: any, i: number) => {
        console.log(`${i + 1}. ${api.method} ${api.path}`)
        console.log(`   标题: ${api.title}`)
        console.log()
      })
    } else {
      console.log('【单个 API】')
      console.log(`方法: ${apiData.api?.method}`)
      console.log(`路径: ${apiData.api?.path}`)
      console.log(`标题: ${apiData.api?.title}`)
    }

    // 4. 尝试获取更多 API（分页）
    console.log()
    console.log('【尝试获取更多 API】')
    console.log()

    const pages = [2, 3, 4, 5]
    for (const page of pages) {
      try {
        const pageRes = await axios.get(`${BASE_URL}/dev-api-restapi.json`, {
          headers: { Token: token },
          params: { pageID: page },
        })

        const pageData = JSON.parse(pageRes.data.data)
        if (pageData.api) {
          console.log(`页面 ${page}:`)
          console.log(`  ${pageData.api.method} ${pageData.api.path} - ${pageData.api.title}`)
        }
      } catch (e) {
        console.log(`页面 ${page}: 无数据`)
      }
    }

  } catch (error: any) {
    console.error('❌ 错误:', error.message)
  }
}

parseApiData()
