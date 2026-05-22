import { zentaoProvider } from '../src/providers/zentao/zentao-provider.js'

async function test() {
  await zentaoProvider.authenticate()
  const tasks = await zentaoProvider.getMyTasks()

  console.log('\n=== 过滤后的任务 ===')
  console.log(`总计: ${tasks.length} 个任务（不含 closed）\n`)

  tasks.forEach((t, i) => {
    console.log(`${i + 1}. [${t.status}] ${t.name}`)
    console.log(`   截止: ${t.deadline || '无'} | 预计: ${t.estimate || 0}h`)
  })
}

test().catch(console.error)
