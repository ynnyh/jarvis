/**
 * Chat agent loop。
 *
 * 给定可用工具列表 + 对话历史，循环调 LLM 直到模型不再请求工具调用，
 * 中间所有 tool_calls 都通过 toolRegistry.execute 真正执行，结果作为
 * role=tool 的消息插回对话历史。
 *
 * 设计取舍：
 * - 一次拉所有可用工具的 schema 转 JSON Schema。工具不多（<20），成本可忽略。
 * - maxIterations 默认 8。绝大多数对话 1~3 轮就收敛；8 是兜底防失控。
 * - tool 执行失败时把 error message 作为 tool 消息发回 LLM，让模型自己决定要不要重试/换工具。
 *   不要 throw 出整个 loop——一次工具失败不该让整个回复失败。
 * - 返回完整的"新增"消息列表（不含输入），调用方追加到对话末尾保存即可。
 */

import { z } from 'zod'
import { getLlmClient, type ChatMessage, type ToolDefinition, type ToolCall } from '../llm/client.js'
import { toolRegistry, type ToolMetadata } from '../core/tool-registry.js'

export interface AgentStep {
  /** assistant 的这一次回复（可能只有文字、只有工具调用、或两者都有） */
  assistantMessage: ChatMessage
  /** 这一轮执行的工具调用结果。空数组说明 LLM 决定不调工具，应当是最后一步 */
  toolResults: ChatMessage[]
}

export interface RunAgentOptions {
  /** 输入消息历史（不含本轮新增的） */
  messages: ChatMessage[]
  /** 允许调用的工具名白名单。不在白名单的工具即使注册了也不暴露给 LLM */
  allowedTools: string[]
  /** 最大循环轮数，默认 8 */
  maxIterations?: number
  /** 温度，默认 0.3 */
  temperature?: number
  /** 单次最大 tokens，默认 2048（对话比一次性日报需要更宽裕） */
  maxTokens?: number
  /** 可选 system prompt 前缀。如已在 messages 里，不传 */
  systemPrompt?: string
}

export interface RunAgentResult {
  /** 本轮新增的所有消息（assistant 回复 + tool 消息们），按顺序 */
  newMessages: ChatMessage[]
  /** 每个 step 的明细，调试/UI 展示用 */
  steps: AgentStep[]
  /** 累计 token 用量 */
  tokensIn: number
  tokensOut: number
  /** 是否因 maxIterations 截断 */
  truncated: boolean
}

export async function runAgent(opts: RunAgentOptions): Promise<RunAgentResult> {
  const maxIter = opts.maxIterations ?? 8
  const temperature = opts.temperature ?? 0.3
  const maxTokens = opts.maxTokens ?? 2048

  // 构造对话历史的工作副本（不污染调用方）
  const messages: ChatMessage[] = []
  if (opts.systemPrompt) {
    messages.push({ role: 'system', content: opts.systemPrompt })
  }
  messages.push(...opts.messages)

  // 把白名单工具的 schema 转成 OpenAI tools 数组
  const tools = buildToolDefinitions(opts.allowedTools)

  const client = getLlmClient()
  const steps: AgentStep[] = []
  const newMessages: ChatMessage[] = []
  let tokensIn = 0
  let tokensOut = 0
  let truncated = false

  for (let i = 0; i < maxIter; i++) {
    const res = await client.chat({
      messages,
      tools: tools.length > 0 ? tools : undefined,
      temperature,
      maxTokens,
    })
    tokensIn += res.tokensIn
    tokensOut += res.tokensOut

    const assistantMsg: ChatMessage = {
      role: 'assistant',
      content: res.text,
      ...(res.toolCalls.length > 0 ? { tool_calls: res.toolCalls } : {}),
    }
    messages.push(assistantMsg)
    newMessages.push(assistantMsg)

    // 没有 tool calls：本轮结束
    if (res.toolCalls.length === 0) {
      steps.push({ assistantMessage: assistantMsg, toolResults: [] })
      return { newMessages, steps, tokensIn, tokensOut, truncated }
    }

    // 执行所有 tool calls（顺序执行，避免多个工具同时打禅道把 token 用完）
    const toolResults: ChatMessage[] = []
    for (const call of res.toolCalls) {
      const toolMsg = await executeToolCall(call, opts.allowedTools)
      toolResults.push(toolMsg)
      messages.push(toolMsg)
      newMessages.push(toolMsg)
    }
    steps.push({ assistantMessage: assistantMsg, toolResults })
  }

  // 跑到最大轮数还没结束——硬停
  truncated = true
  const stopMsg: ChatMessage = {
    role: 'assistant',
    content: `（达到最大工具调用轮数 ${maxIter}，强制停止。可能任务过于复杂，或工具结果反复无法收敛。）`,
  }
  newMessages.push(stopMsg)
  return { newMessages, steps, tokensIn, tokensOut, truncated }
}

/**
 * 执行单个 tool call。失败时把 error 作为 tool 消息返回（不抛），
 * 让 LLM 自己看到错误并决定下一步。
 */
async function executeToolCall(call: ToolCall, allowed: string[]): Promise<ChatMessage> {
  const name = call.function.name

  // 不在白名单的工具：直接拒绝，告诉 LLM 这工具不可用
  if (!allowed.includes(name)) {
    return {
      role: 'tool',
      tool_call_id: call.id,
      name,
      content: JSON.stringify({ error: `工具 ${name} 不在允许列表中` }),
    }
  }

  // 解析参数
  let args: Record<string, unknown>
  try {
    args = call.function.arguments ? JSON.parse(call.function.arguments) : {}
  } catch (e: any) {
    return {
      role: 'tool',
      tool_call_id: call.id,
      name,
      content: JSON.stringify({ error: `参数 JSON 解析失败: ${e?.message || e}` }),
    }
  }

  // 执行
  try {
    const result = await toolRegistry.execute(name, args)
    return {
      role: 'tool',
      tool_call_id: call.id,
      name,
      // 限长：单次工具结果大于 12KB 就截断。LLM context window 不是无限。
      content: truncateForContext(stringify(result), 12_000),
    }
  } catch (e: any) {
    return {
      role: 'tool',
      tool_call_id: call.id,
      name,
      content: JSON.stringify({ error: e?.message || String(e) }),
    }
  }
}

function stringify(v: unknown): string {
  if (typeof v === 'string') return v
  try {
    return JSON.stringify(v, null, 2)
  } catch {
    return String(v)
  }
}

function truncateForContext(s: string, max: number): string {
  if (s.length <= max) return s
  return s.slice(0, max) + `\n…（结果过长，已截断到 ${max} 字符）`
}

/**
 * 从工具注册表里挑出白名单工具，转成 OpenAI tools 格式。
 */
function buildToolDefinitions(allowed: string[]): ToolDefinition[] {
  const out: ToolDefinition[] = []
  for (const name of allowed) {
    const tool = toolRegistry.get(name)
    if (!tool) continue
    out.push({
      type: 'function',
      function: {
        name: tool.metadata.name,
        description: tool.metadata.description,
        parameters: zodToParametersSchema(tool.metadata.inputSchema),
      },
    })
  }
  return out
}

/**
 * 把 zod ObjectSchema 转成 OpenAI function parameters JSON Schema。
 *
 * 用 zod 4 自带的 toJSONSchema，再剥掉 $schema 字段（OpenAI 不要这个）。
 * 失败回退到一个空对象 schema，让 LLM 调用时不传参——保稳定。
 */
function zodToParametersSchema(schema: z.ZodObject<any>): Record<string, any> {
  try {
    const json = (z as any).toJSONSchema(schema) as any
    delete json.$schema
    // OpenAI 要求 parameters 至少是 object 类型
    if (json.type !== 'object') {
      return { type: 'object', properties: {}, additionalProperties: false }
    }
    return json
  } catch (e) {
    console.warn('zod → JSON Schema 转换失败，退回空 schema:', e)
    return { type: 'object', properties: {}, additionalProperties: false }
  }
}

/**
 * 默认对外暴露的只读工具白名单。
 * 写工具（log_task_effort）不在这里——闸门红线："Doing → Done/Closed 永远不自动做"，
 * 写工时虽然不动状态，但仍走 V2 的"agent 申请 → 用户确认"流程。
 */
export const DEFAULT_AGENT_TOOLS = [
  'get_tasks',
  'get_today_tasks',
  'get_task_detail',
  'get_task_commits',
  'analyze_risk',
  'get_daily_review',
] as const

/**
 * Chat agent 的默认 system prompt。可由调用方覆盖。
 */
export function defaultSystemPrompt(assistantName = 'Jarvis'): string {
  return [
    `你是 ${assistantName}，用户的个人任务助手。`,
    '你可以调用工具查询禅道任务、本地 commit、今日复盘、风险分析等。',
    '原则：',
    '1. 用户问到任务/工时/风险/复盘等具体业务问题时，先调相关工具拿真实数据，再回答。不要凭空编。',
    '2. 工具不可用或失败时，明确告诉用户失败原因，不要装作有数据。',
    '3. 回答要简洁直接。日报、风险类的输出去技术化——不要出现 commit/sha/repo 这种词，用项目名 + 任务名组织。',
    '4. 你只能读取数据，不能写回禅道（包括改任务状态、写工时）。用户如有写回需求，告诉他用 UI 上的对应入口。',
  ].join('\n')
}
