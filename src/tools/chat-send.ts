import { z } from 'zod'
import { toolRegistry } from '../core/tool-registry.js'
import type { Tool } from '../core/tool-registry.js'
import { runAgent, DEFAULT_AGENT_TOOLS, defaultSystemPrompt } from '../agent/chat-agent.js'
import type { ChatMessage } from '../llm/client.js'

/**
 * chat_send：对话窗口的入口工具。
 *
 * 调用方负责持久化对话——这里只做"一次 agent 跑"：
 *   输入：完整消息历史（含当前用户消息）+ 配置
 *   输出：本轮新增的 assistant/tool 消息列表（按时间顺序）
 *
 * 不接入写工具（log_task_effort 等）。白名单见 [[DEFAULT_AGENT_TOOLS]]。
 */

const messageSchema: z.ZodType<ChatMessage> = z.object({
  role: z.enum(['system', 'user', 'assistant', 'tool']),
  content: z.string(),
  tool_calls: z.array(z.object({
    id: z.string(),
    type: z.literal('function'),
    function: z.object({
      name: z.string(),
      arguments: z.string(),
    }),
  })).optional(),
  tool_call_id: z.string().optional(),
  name: z.string().optional(),
}) as any

const inputSchema = z.object({
  /** 完整消息历史，最后一条应该是 user。系统提示会自动加，不要传 system */
  messages: z.array(messageSchema).min(1),
  /** 助手显示名（影响 system prompt 里的自我介绍）。默认 Jarvis */
  assistantName: z.string().optional(),
  /** 最大工具调用轮数，默认 8 */
  maxIterations: z.number().int().min(1).max(20).optional(),
  /** 温度，默认 0.3 */
  temperature: z.number().min(0).max(2).optional(),
  /** 覆盖默认工具白名单。一般用 default；调试时可显式传 */
  allowedTools: z.array(z.string()).optional(),
})

async function execute(input: z.infer<typeof inputSchema>) {
  const allowedTools = input.allowedTools && input.allowedTools.length > 0
    ? input.allowedTools
    : [...DEFAULT_AGENT_TOOLS]

  // 如果调用方没传 system，注入默认 prompt 作为第一条；若已有 system 则尊重之
  const hasSystem = input.messages[0]?.role === 'system'
  const systemPrompt = hasSystem ? undefined : defaultSystemPrompt(input.assistantName)

  const result = await runAgent({
    messages: input.messages,
    allowedTools,
    maxIterations: input.maxIterations,
    temperature: input.temperature,
    systemPrompt,
  })

  return {
    newMessages: result.newMessages,
    steps: result.steps,
    tokensIn: result.tokensIn,
    tokensOut: result.tokensOut,
    truncated: result.truncated,
    allowedTools,
  }
}

export const chatSendTool: Tool = {
  metadata: {
    name: 'chat_send',
    description: '对话入口：跑一轮 chat agent loop。输入完整消息历史（最后一条 user），返回新增的 assistant 与 tool 消息。',
    category: 'chat',
    version: '1.0.0',
    inputSchema,
    examples: [
      {
        input: {
          messages: [{ role: 'user', content: '今天有哪些任务要做？' }],
        },
        description: '最小调用：单条 user 消息',
      },
    ],
  },
  execute: execute as any,
}

toolRegistry.register(chatSendTool)
