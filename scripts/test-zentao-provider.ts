import { zentaoProvider } from '../src/providers/zentao/zentao-provider.js'

async function test() {
  console.log('=====================================')
  console.log('  ZenTaoProvider 测试')
  console.log('=====================================')
  console.log()

  try {
    // 1. 认证
    console.log('【测试 1】认证')
    await zentaoProvider.authenticate()
    console.log('✅ 认证成功\n')

    // 2. 获取执行列表
    console.log('【测试 2】获取执行列表')
    const executions = await zentaoProvider.getExecutions()
    console.log(`✅ 获取到 ${executions.length} 个执行`)
    executions.slice(0, 3).forEach(e => {
      console.log(`   [${e.status}] ${e.name}`)
    })
    console.log()

    // 3. 获取我的任务
    console.log('【测试 3】获取我的任务')
    const myTasks = await zentaoProvider.getMyTasks()
    console.log(`✅ 获取到 ${myTasks.length} 个指派给我的任务`)
    if (myTasks.length > 0) {
      myTasks.forEach(t => {
        const assignee = typeof t.assignedTo === 'string' ? t.assignedTo : t.assignedTo?.account
        console.log(`   📌 [${t.status}] ${t.name} (指派给: ${assignee})`)
      })
    } else {
      console.log('   ℹ️ 当前没有被指派给我的任务')
    }
    console.log()

    // 4. 获取任务详情（如果有任务）
    if (myTasks.length > 0) {
      console.log('【测试 4】获取任务详情')
      const detail = await zentaoProvider.getTaskDetail(myTasks[0].id)
      console.log(`✅ 任务详情: ${detail.name}`)
      console.log(`   状态: ${detail.status}`)
      console.log(`   优先级: ${detail.pri}`)
      console.log()
    }

    console.log('=====================================')
    console.log('  测试完成 ✅')
    console.log('=====================================')

  } catch (error: any) {
    console.error('测试失败:', error.message)
    process.exit(1)
  }
}

test()
