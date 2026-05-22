import axios from 'axios'
import dotenv from 'dotenv'

dotenv.config()

const BASE_URL = process.env.ZENTAO_BASE_URL || ''
const ACCOUNT = process.env.ZENTAO_ACCOUNT || ''
const PASSWORD = process.env.ZENTAO_PASSWORD || ''

async function enumBrowseType() {
  console.log('=====================================')
  console.log('  browseType 枚举测试')
  console.log('=====================================')
  console.log()

  // 获取 Token
  const tokenRes = await axios.post(`${BASE_URL}/api.php/v1/tokens`, {
    account: ACCOUNT,
    password: PASSWORD,
  })
  const token = tokenRes.data.token
  console.log('✅ Token 获取成功')
  console.log()

  // 使用一个固定的执行 ID 进行测试（289）
  const executionId = 289

  // 所有可能的 browseType 值
  const browseTypes = [
    'all',
    'assignedTo',
    'assignedtome',
    'doing',
    'wait',
    'unclosed',
    'unfinished',
    'myinvolved',
    'involved',
    'openedByMe',
    'finishedByMe',
    'closedByMe',
    'needconfirm',
    'assignedByMe',
    'delayed',
    'done',
    'closed',
    'cancel',
    'changed',
  ]

  console.log('=====================================')
  console.log('  测试结果')
  console.log('=====================================')
  console.log()

  const results: Array<{ browseType: string; count: number; titles: string[]; url: string }> = []

  for (const browseType of browseTypes) {
    try {
      const url = `${BASE_URL}/api.php/v1/executions/${executionId}/tasks`
      const res = await axios.get(url, {
        headers: { Token: token },
        params: {
          browseType,
          module: 0,
          recPerPage: 100,
          page: 1,
        },
      })

      const tasks = res.data.tasks || []
      const count = res.data.total || tasks.length
      const titles = tasks.slice(0, 3).map((t: any) => t.name)

      results.push({ browseType, count, titles, url: `${url}?browseType=${browseType}&module=0` })

      console.log(`browseType=${browseType}`)
      console.log(`  count=${count}`)
      titles.forEach((title, i) => {
        console.log(`  ${i + 1}. ${title}`)
      })
      console.log()
    } catch (e: any) {
      console.log(`browseType=${browseType}`)
      console.log(`  ❌ 错误: ${e.response?.status || e.message}`)
      console.log()
    }
  }

  // 排序输出
  console.log('=====================================')
  console.log('  排序结果（按任务数量降序）')
  console.log('=====================================')
  console.log()

  results.sort((a, b) => b.count - a.count)

  results.forEach((r, i) => {
    console.log(`${i + 1}. browseType=${r.browseType} | count=${r.count}`)
    r.titles.forEach((title, j) => {
      console.log(`   ${j + 1}. ${title}`)
    })
  })

  // 找到最接近 76 的结果
  console.log()
  console.log('=====================================')
  console.log('  最接近工作台 76 条的结果')
  console.log('=====================================')
  console.log()

  const closest = results.reduce((prev, curr) => {
    return Math.abs(curr.count - 76) < Math.abs(prev.count - 76) ? curr : prev
  })

  console.log(`最接近的 browseType: ${closest.browseType}`)
  console.log(`任务数量: ${closest.count}`)
  console.log(`差值: ${Math.abs(closest.count - 76)}`)
  console.log()
  console.log('前3个任务:')
  closest.titles.forEach((title, i) => {
    console.log(`  ${i + 1}. ${title}`)
  })
}

enumBrowseType().catch(console.error)
