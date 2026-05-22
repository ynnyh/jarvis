import { zentaoTokenProvider } from '../src/providers/zentao/zentao-token-provider.js'

async function countTasks() {
  console.log('=====================================')
  console.log('  统计我的任务数量')
  console.log('=====================================')
  console.log()

  // 方法 1: 使用 assignedTo 参数
  console.log('【方法 1】使用 assignedTo 参数')
  console.log('-------------------------------------')
  const tasksByAssignedTo = await zentaoTokenProvider.getMyTasks()
  console.log(`✅ 找到 ${tasksByAssignedTo.length} 个任务`)
  console.log()

  // 方法 2: 使用 browseType=myinvolved
  console.log('【方法 2】使用 browseType=myinvolved')
  console.log('-------------------------------------')
  const tasksByBrowseType = await zentaoTokenProvider.getMyTasksByBrowseType('myinvolved')
  console.log(`✅ 找到 ${tasksByBrowseType.length} 个任务`)
  console.log()

  // 方法 3: 使用 browseType=unclosed
  console.log('【方法 3】使用 browseType=unclosed')
  console.log('-------------------------------------')
  const tasksByUnclosed = await zentaoTokenProvider.getMyTasksByBrowseType('unclosed')
  console.log(`✅ 找到 ${tasksByUnclosed.length} 个任务`)
  console.log()

  // 统计状态分布
  console.log('=====================================')
  console.log('  任务状态分布 (assignedTo 方式)')
  console.log('=====================================')
  console.log()

  const statusCount: Record<string, number> = {}
  tasksByAssignedTo.forEach(t => {
    statusCount[t.status] = (statusCount[t.status] || 0) + 1
  })

  Object.entries(statusCount).forEach(([status, count]) => {
    console.log(`  ${status}: ${count} 个`)
  })
  console.log()

  // 显示前 20 个任务
  console.log('=====================================')
  console.log('  我的任务列表 (前 20 个)')
  console.log('=====================================')
  console.log()

  tasksByAssignedTo.slice(0, 20).forEach((t, i) => {
    console.log(`${i + 1}. [${t.status}] ${t.name}`)
    console.log(`   ID: ${t.id} | 优先级: ${t.pri} | 截止: ${t.deadline || '无'}`)
    console.log(`   执行: ${t.execution}`)
    console.log()
  })

  console.log('=====================================')
  console.log('  统计完成')
  console.log('=====================================')
  console.log()
  console.log(`总计: ${tasksByAssignedTo.length} 个任务（已过滤 closed）`)
}

countTasks().catch(console.error)
