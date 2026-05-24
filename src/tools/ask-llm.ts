import { z } from 'zod'
import { toolRegistry } from '../core/tool-registry.js'
import type { Tool } from '../core/tool-registry.js'
import { getLlmClient } from '../llm/client.js'

const messageSchema = z.object({
  role: z.enum(['system', 'user', 'assistant']),
  content: z.string().min(1),
})

const inputSchema = z.object({
  messages: z.array(messageSchema).min(1).describe('对话消息列表（system/user/assistant 角色）'),
  temperature: z.number().min(0).max(2).optional().describe('0~2，默认 0.3'),
  maxTokens: z.number().int().positive().max(8192).optional().describe('单次最大输出 tokens，默认 1024'),
  model: z.string().optional().describe('覆盖默认 model'),
})

async function execute(input: z.infer<typeof inputSchema>) {
  const client = getLlmClient()
  const result = await client.chat({
    messages: input.messages,
    temperature: input.temperature,
    maxTokens: input.maxTokens,
    model: input.model,
  })
  return {
    text: result.text,
    tokensIn: result.tokensIn,
    tokensOut: result.tokensOut,
    model: result.model,
  }
}

export const askLlmTool: Tool = {
  metadata: {
    name: 'ask-llm',
    description: '调用配置好的 LLM（OpenAI 兼容，默认 DeepSeek）做一次 chat completion。返回文本+token 计数。',
    category: 'llm',
    version: '1.0.0',
    inputSchema,
    examples: [
      {
        input: {
          messages: [
            { role: 'system', content: '你是简洁的助手。' },
            { role: 'user', content: '一句话告诉我今天周几。' },
          ],
        },
        description: '最小调用：system + user 两条消息',
      },
    ],
  },
  execute: execute as any,
}

toolRegistry.register(askLlmTool)
