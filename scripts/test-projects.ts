import axios from 'axios'
import dotenv from 'dotenv'

dotenv.config()

const BASE_URL = process.env.ZENTAO_BASE_URL || ''
const ACCOUNT = process.env.ZENTAO_ACCOUNT || ''
const PASSWORD = process.env.ZENTAO_PASSWORD || ''

async function testProjects() {
  console.log('=====================================')
  console.log('  获取禅道项目列表')
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

    // 2. 获取项目列表
    console.log('【步骤 2】获取项目列表...')
    const projectsRes = await axios.get(`${BASE_URL}/api.php/v1/projects`, {
      headers: { Token: token },
      params: { page: 1, limit: 100 },
    })

    console.log('响应结构:', Object.keys(projectsRes.data))
    console.log('page:', projectsRes.data.page)
    console.log('total:', projectsRes.data.total)
    console.log('limit:', projectsRes.data.limit)
    console.log('projects 数量:', projectsRes.data.projects?.length)
    console.log()

    // 3. 获取所有项目（分页）
    console.log('【步骤 3】获取所有项目...')
    const allProjects: any[] = []
    let page = 1
    let totalPages = 1

    while (page <= totalPages) {
      const res = await axios.get(`${BASE_URL}/api.php/v1/projects`, {
        headers: { Token: token },
        params: { page, limit: 100 },
      })

      const projects = res.data.projects || []
      allProjects.push(...projects)

      if (page === 1) {
        totalPages = Math.ceil((res.data.total || 0) / (res.data.limit || 20))
        console.log(`总项目数: ${res.data.total}, 每页: ${res.data.limit}, 总页数: ${totalPages}`)
      }

      console.log(`  第 ${page}/${totalPages} 页: +${projects.length} 个项目`)
      page++
    }

    console.log()
    console.log('=====================================')
    console.log(`  项目列表 (${allProjects.length} 个)`)
    console.log('=====================================')
    console.log()

    allProjects.forEach((p, i) => {
      console.log(`${i + 1}. [${p.status}] ${p.name}`)
      console.log(`   ID: ${p.id} | 编号: ${p.code || '无'}`)
      console.log(`   模型: ${p.model} | 类型: ${p.type}`)
      console.log(`   时间: ${p.begin} ~ ${p.end}`)
      console.log(`   进度: ${p.progress}%`)
      console.log()
    })

  } catch (error: any) {
    console.error('❌ 错误:', error.message)
    if (error.response) {
      console.error('状态码:', error.response.status)
      console.error('响应:', error.response.data)
    }
  }
}

testProjects()
