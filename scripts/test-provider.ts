import { zentaoProvider, ZenTaoTask } from '../src/providers/zentao/zentao-provider.js'

async function test() {
  console.log('=====================================')
  console.log('  ZenTaoProvider 测试')
  console.log('=====================================')
  console.log()

  try {
    // 1. 认证
    console.log('【测试 1】认证')
    console.log('-------------------------------------')
    const token = await zentaoProvider.authenticate()
    console.log(`✅ 认证成功，Token: ${token.slice(0, 15)}...`)
    console.log()

    // 2. 获取执行列表
    console.log('【测试 2】获取执行列表')
    console.log('-------------------------------------')
    const executions = await zentaoProvider.getExecutions('doing')
    console.log(`✅ 获取到 ${executions.length} 个进行中执行`)
    if (executions.length > 0) {
      console.log('\n前 5 个执行:')
      executions.slice(0, 5).forEach((e, i) => {
        console.log(`  ${i + 1}. [${e.status}] ${e.name} (ID: ${e.id})`)
      })
    }
    console.log()

    // 3. 获取某个执行的任务
    if (executions.length > 0) {
      const firstExec = executions[0]
      console.log(`【测试 3】获取执行 "${firstExec.name}" 的任务`)
      console.log('-------------------------------------')
      const tasks = await zentaoProvider.getTasksByExecution(firstExec.id)
      console.log(`✅ 获取到 ${tasks.length} 个任务`)
      if (tasks.length > 0) {
        console.log('\n前 3 个任务:')
        tasks.slice(0, 3).forEach((t: ZenTaoTask, i: number) => {
          const assignee = typeof t.assignedTo === 'string' ? t.assignedTo : t.assignedTo?.realname || '无'
          console.log(`  ${i + 1}. [${t.status}] ${t.name}`)
          console.log(`     指派给: ${assignee} | 截止: ${t.deadline || '无'}`)
        })
      }
      console.log()
    }

    // 4. 获取我的任务
    console.log('【测试 4】获取我的任务')
    console.log('-------------------------------------')
    const myTasks = await zentaoProvider.getMyTasks()
    console.log(`✅ 获取到 ${myTasks.length} 个我的任务`)
    if (myTasks.length > 0) {
      console.log('\n我的任务列表:')
      myTasks.slice(0, 5).forEach((t: ZenTaoTask, i: number) => {
        console.log(`  ${i + 1}. [${t.status}] ${t.name}`)
        console.log(`     截止: ${t.deadline || '无'} | 预计: ${t.estimate || 0}h`)
      })
    }
    console.log()

    // 5. 获取任务详情
    console.log('【测试 5】获取任务详情')
    console.log('-------------------------------------')
    if (myTasks.length > 0) {
      const detail = await zentaoProvider.getTaskDetail(myTasks[0].id)
      console.log(`✅ 任务详情:`)
      console.log(`   名称: ${detail.name}`)
      console.log(`   状态: ${detail.status}`)
      console.log(`   优先级: ${detail.pri}`)
      console.log(`   预计工时: ${detail.estimate || 0}h`)
      console.log(`   已消耗: ${detail.consumed || 0}h`)
    } else {
      console.log('ℹ️  没有任务可获取详情')
    }
    console.log()

    console.log('=====================================')
    console.log('  所有测试通过 ✅')
    console.log('=====================================')

  } catch (error: any) {
    console.error()
    console.error('=====================================')
    console.error('  测试失败 ❌')
    console.error('=====================================')
    console.error(`错误: ${error.message}`)
    if (error.response) {
      console.error(`状态码: ${error.response.status}`)
      console.error(`响应数据:`, JSON.stringify(error.response.data, null, 2))
    }
    process.exit(1)
  }
}

test()
