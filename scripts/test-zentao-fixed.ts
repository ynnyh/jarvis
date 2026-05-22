import { zentaoTokenProvider } from '../src/providers/zentao/zentao-token-provider.js'

async function test() {
  console.log('=====================================')
  console.log('  ZenTaoTokenProvider 修复后测试')
  console.log('=====================================')
  console.log()

  try {
    const tasks = await zentaoTokenProvider.getMyTasks()
    console.log(`\n✅ 共找到 ${tasks.length} 个指派给我的任务`)

    tasks.forEach((t, i) => {
      console.log(`\n${i + 1}. [${t.status}] ${t.name}`)
      console.log(`   执行: ${t.execution}`)
      console.log(`   优先级: ${t.pri} | 截止: ${t.deadline || '无'}`)
    })

  } catch (error: any) {
    console.error('测试失败:', error.message)
    if (error.response) {
      console.error('状态码:', error.response.status)
      console.error('响应:', error.response.data)
    }
  }
}

test()
