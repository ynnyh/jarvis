import axios from 'axios'
import dotenv from 'dotenv'

dotenv.config()

const BASE_URL = process.env.ZENTAO_BASE_URL || ''
const ACCOUNT = process.env.ZENTAO_ACCOUNT || ''
const PASSWORD = process.env.ZENTAO_PASSWORD || ''

async function find76Tasks() {
  console.log('=====================================')
  console.log('  寻找 76 个任务')
  console.log('=====================================')
  console.log()

  const tokenRes = await axios.post(`${BASE_URL}/api.php/v1/tokens`, {
    account: ACCOUNT,
    password: PASSWORD,
  })
  const token = tokenRes.data.token

  // 获取所有执行
  const allExecutions: any[] = []
  let page = 1
  let totalPages = 1

  while (page <= totalPages) {
    const execRes = await axios.get(`${BASE_URL}/api.php/v1/executions`, {
      headers: { Token: token },
      params: { status: 'all', recPerPage: 100, page },
    })
    const executions = execRes.data.executions || []
    allExecutions.push(...executions)
    if (page === 1) {
      totalPages = Math.ceil((execRes.data.total || 0) / (execRes.data.limit || 20))
    }
    page++
  }

  console.log(`共 ${allExecutions.length} 个执行`)
  console.log()

  // 尝试不同的组合
  const combinations = [
    { browseType: 'assignedtome', status: null },
    { browseType: 'myinvolved', status: null },
    { browseType: 'unclosed', status: null },
    { browseType: 'unfinished', status: null },
    { browseType: 'needconfirm', status: null },
    { browseType: 'delayed', status: null },
    { assignedTo: ACCOUNT, status: 'doing' },
    { assignedTo: ACCOUNT, status: 'wait' },
    { assignedTo: ACCOUNT, status: 'doing', browseType: 'unclosed' },
  ]

  for (const combo of combinations) {
    let count = 0
    const titles: string[] = []

    for (const exec of allExecutions) {
      try {
        const params: any = { module: 0, recPerPage: 100, page: 1 }
        if (combo.browseType) params.browseType = combo.browseType
        if (combo.assignedTo) params.assignedTo = combo.assignedTo
        if (combo.status) params.status = combo.status

        const res = await axios.get(`${BASE_URL}/api.php/v1/executions/${exec.id}/tasks`, {
          headers: { Token: token },
          params,
        })

        const tasks = res.data.tasks || []
        count += res.data.total || tasks.length

        if (titles.length < 3 && tasks.length > 0) {
          titles.push(...tasks.slice(0, 3 - titles.length).map((t: any) => t.name))
        }
      } catch (e) {
        // ignore
      }
    }

    console.log(`组合: ${JSON.stringify(combo)}`)
    console.log(`  count=${count}`)
    titles.slice(0, 3).forEach((title, i) => {
      console.log(`  ${i + 1}. ${title}`)
    })
    console.log()
  }

  // 尝试只统计特定状态的组合
  console.log('=====================================')
  console.log('  尝试 doing + wait (排除 done/closed/cancel)')
  console.log('=====================================')
  console.log()

  let doingWaitCount = 0
  for (const exec of allExecutions) {
    try {
      // doing
      const doingRes = await axios.get(`${BASE_URL}/api.php/v1/executions/${exec.id}/tasks`, {
        headers: { Token: token },
        params: { assignedTo: ACCOUNT, status: 'doing', module: 0, recPerPage: 100 },
      })
      doingWaitCount += doingRes.data.total || 0

      // wait
      const waitRes = await axios.get(`${BASE_URL}/api.php/v1/executions/${exec.id}/tasks`, {
        headers: { Token: token },
        params: { assignedTo: ACCOUNT, status: 'wait', module: 0, recPerPage: 100 },
      })
      doingWaitCount += waitRes.data.total || 0
    } catch (e) {
      // ignore
    }
  }

  console.log(`doing + wait 总数: ${doingWaitCount}`)
}

find76Tasks().catch(console.error)
