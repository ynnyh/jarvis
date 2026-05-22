#!/usr/bin/env node

// 加载 .env（必须在导入业务模块之前）
import 'dotenv/config'

// 必须先导入 index.ts 完成所有初始化
import '../index.js'

import { toolRegistry } from '../core/tool-registry.js'
import { eventBus } from '../events/event-bus.js'
import { memoryStore } from '../memory/memory-store.js'
import { actionEngine } from '../actions/action-engine.js'
import { agentScheduler } from '../scheduler/agent-scheduler.js'
import { contextBuilder } from '../ai/context-builder.js'
import { agentState } from '../core/agent-state.js'
import { GitProvider } from '../providers/git/git-provider.js'
import { closeSharedTencentCodeMcpClient } from '../mcp/tencentcode-client.js'

// 当作为 `tool` 命令运行时，stdout 需要保持为纯 JSON 给上游解析。
// 因此事件监听器在 tool 模式下走 stderr，正常模式走 stdout。
const isJsonOutCommand = process.argv[2] === 'tool'
const logEvent = isJsonOutCommand ? console.error : console.log

// 注册事件监听器，用于演示
const unsubscribeMessage = eventBus.on('agent:message', (payload) => {
  const icons: Record<string, string> = {
    info: 'ℹ️',
    warning: '⚠️',
    success: '✅',
    error: '❌',
  }
  logEvent(`${icons[payload.type || 'info'] || '•'} ${payload.content}`)
})

const unsubscribeNotify = eventBus.on('agent:notify', (payload) => {
  logEvent(`\n🔔 [${payload.priority.toUpperCase()}] ${payload.title}`)
  logEvent(`   ${payload.body}\n`)
})

async function main() {
  const command = process.argv[2]

  // 工具命令的 stdout 必须保持为纯 JSON 供 Tauri/外部程序解析，
  // 因此把所有装饰性输出统一走 stderr。
  const info = isJsonOutCommand ? console.error : console.log

  info('🤖 Jarvis Agent Core v3.0')
  info(`状态: ${agentState.getState()}\n`)

  switch (command) {
    case 'tools':
      console.log('📦 已注册 Tools:')
      for (const tool of toolRegistry.list()) {
        console.log(`  • ${tool.name} (${tool.category}) - ${tool.description}`)
      }
      break

    case 'tool':
      const toolName = process.argv[3]
      const toolInput = process.argv[4] ? JSON.parse(process.argv[4]) : {}
      console.error(`🔧 执行 Tool: ${toolName}`)
      try {
        const result = await toolRegistry.execute(toolName, toolInput)
        // 纯 JSON 输出到 stdout（无前缀），方便上游解析
        console.log(JSON.stringify(result))
      } catch (err) {
        console.error('错误:', err instanceof Error ? err.message : String(err))
        process.exitCode = 1
      }
      break

    case 'actions':
      console.log('🎬 已注册 Actions:')
      for (const action of actionEngine.list()) {
        console.log(`  • ${action.id} - ${action.description}`)
        console.log(`    步骤: ${action.steps.map(s => s.tool).join(' → ')}`)
      }
      break

    case 'action':
      const actionId = process.argv[3]
      console.log(`🎬 执行 Action: ${actionId}`)
      try {
        const result = await actionEngine.execute(actionId)
        console.log('✅ Action 完成')
        console.log('步骤结果:')
        for (const step of result.stepResults) {
          const status = step.success ? '✅' : '❌'
          console.log(`  ${status} ${step.tool} (${step.duration}ms)`)
        }
      } catch (err) {
        console.error('❌ Action 失败:', err instanceof Error ? err.message : String(err))
      }
      break

    case 'memory':
      const subCommand = process.argv[3]
      if (subCommand === 'add') {
        const entry = memoryStore.add({
          type: (process.argv[4] as any) || 'analysis',
          content: process.argv[5] || '',
          tags: process.argv[6] ? process.argv[6].split(',') : [],
          importance: parseInt(process.argv[7] || '5'),
        })
        console.log('✅ 记忆已添加:', entry.id)
      } else if (subCommand === 'list') {
        const memories = memoryStore.query({})
        console.log('🧠 记忆列表:')
        for (const mem of memories) {
          console.log(`  [${mem.type}] ${mem.content.slice(0, 50)}... (重要性: ${mem.importance})`)
        }
      } else if (subCommand === 'stats') {
        const stats = memoryStore.getStats()
        console.log('📊 记忆统计:')
        console.log(`  总计: ${stats.total}`)
        for (const [type, count] of Object.entries(stats.byType)) {
          console.log(`  ${type}: ${count}`)
        }
      }
      break

    case 'context':
      console.log('📝 构建 AI 上下文...')
      const context = await contextBuilder.buildPrompt()
      console.log(context)
      break

    case 'git':
      const git = new GitProvider()
      if (!git.isRepo()) {
        console.log('❌ 当前目录不是 Git 仓库')
        break
      }
      console.log('📁 Git 信息:')
      const info = git.getRepoInfo()
      console.log(`  分支: ${info.branch}`)
      console.log(`  提交数: ${info.commitCount}`)
      console.log(`  远程: ${info.remoteUrl || '无'}`)

      const status = git.getStatus()
      console.log(`\n  修改: ${status.modified.length}`)
      console.log(`  新增: ${status.added.length}`)
      console.log(`  未跟踪: ${status.untracked.length}`)

      const commits = git.getRecentCommits(3)
      console.log('\n  最近提交:')
      for (const c of commits) {
        console.log(`    ${c.shortHash} - ${c.message} (${c.author})`)
      }
      break

    case 'state':
      const stats = agentState.getStats()
      console.log('🎛️ Agent 状态:')
      console.log(`  当前: ${stats.current}`)
      console.log(`  持续时间: ${stats.duration}ms`)
      console.log(`  历史记录: ${stats.historyCount}`)
      console.log('  状态统计:')
      for (const [state, count] of Object.entries(stats.stateCounts)) {
        console.log(`    ${state}: ${count}`)
      }
      break

    case 'scheduler':
      const schedulerStatus = agentScheduler.getStatus()
      console.log('⏰ 调度器状态:')
      console.log(`  运行中: ${schedulerStatus.running}`)
      console.log(`  任务数: ${schedulerStatus.taskCount}`)
      console.log(`  活跃任务: ${schedulerStatus.activeTasks}`)
      console.log('\n  任务列表:')
      for (const task of agentScheduler.list()) {
        const status = task.enabled ? '🟢' : '⚪'
        console.log(`  ${status} ${task.name} (${task.cron}) - 执行 ${task.runCount} 次`)
      }
      break

    case 'start':
      console.log('🚀 启动 Agent...')
      agentScheduler.start()
      console.log('按 Ctrl+C 停止')
      // 保持运行
      await new Promise(() => {})
      break

    default:
      console.log(`
🤖 Jarvis Agent Core v3.0

用法:
  agent-core tools              列出所有 Tools
  agent-core tool <name> [input] 执行指定 Tool
  agent-core actions            列出所有 Actions
  agent-core action <id>        执行指定 Action
  agent-core memory add <type> <content> [tags] [importance] 添加记忆
  agent-core memory list        列出记忆
  agent-core memory stats       记忆统计
  agent-core context            构建 AI 上下文
  agent-core git                显示 Git 信息
  agent-core state              显示 Agent 状态
  agent-core scheduler          显示调度器状态
  agent-core start              启动 Agent 调度器
`)
  }

  unsubscribeMessage()
  unsubscribeNotify()

  // 关闭共享 MCP 子进程，避免 CLI 退出后留下僵尸 tencentcode-mcp 进程
  await closeSharedTencentCodeMcpClient()

  // 一次性命令完成后必须显式退出。
  // 否则 src/index.ts 注册的 agentScheduler 内部 setInterval 会让 Node 永远不退出，
  // 上游 Tauri 的 Command.output() 也会一直阻塞（前端表现为转圈不返回）。
  // 'start' 是常驻命令，不能 exit。
  if (command !== 'start') {
    process.exit(process.exitCode ?? 0)
  }
}

main().catch((err) => {
  console.error(err)
  process.exit(1)
})
