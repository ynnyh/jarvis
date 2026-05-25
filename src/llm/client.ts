/**
 * LLM 客户端。两种 wire 协议都支持：
 *
 *   wireApi='chat'      → POST /v1/chat/completions
 *                        OpenAI Chat Completions 规范，DeepSeek/Moonshot/Qwen/各种国产模型都兼容。
 *
 *   wireApi='responses' → POST /v1/responses
 *                        OpenAI Responses API（Codex CLI 用的协议）。请求/响应结构和 Chat
 *                        Completions 完全不同：messages → input、choices[0].message → output[]、
 *                        function 工具调用从嵌套结构变成扁平 type=function_call 项等。
 *                        CC Switch 导入 Codex provider 时会写 wireApi='responses'。
 *
 * 对外 ChatMessage / ChatRequest / ChatResponse 都用 OpenAI Chat Completions 风格——
 * 上层（agent loop / ask-llm）不需要关心 wire 协议差异。Responses 适配只在本文件内。
 *
 * 不在这里做 prompt 拼装、retry、流式、缓存——这些都是上层的事。
 * 不支持流式（现有调用方都是非流式）。
 */

import { getLlmCredentials } from '../config/settings.js'

export interface ChatMessage {
  role: 'system' | 'user' | 'assistant' | 'tool'
  content: string
  /** assistant 消息发起的 tool calls。仅 assistant 角色可用 */
  tool_calls?: ToolCall[]
  /** tool 消息携带的 call id，必须匹配某个 assistant.tool_calls[i].id */
  tool_call_id?: string
  /** tool 消息可选的工具名（部分厂商需要） */
  name?: string
}

/** OpenAI 风格的 tool call。type 当前只有 "function" */
export interface ToolCall {
  id: string
  type: 'function'
  function: {
    name: string
    /** JSON 字符串。模型可能产出不合法 JSON，调用方需 try/catch */
    arguments: string
  }
}

/** OpenAI tools 字段格式（Chat Completions 风格，内部按需转 Responses 扁平结构） */
export interface ToolDefinition {
  type: 'function'
  function: {
    name: string
    description: string
    parameters: Record<string, any>  // JSON Schema
  }
}

export interface ChatRequest {
  messages: ChatMessage[]
  /** 0~2，默认 0.3（偏确定性，日报这种场景不希望发散） */
  temperature?: number
  /** 单次最大返回 tokens。默认 1024，长日报可调到 2048+ */
  maxTokens?: number
  /** 覆盖默认 model（从 config 读），少数场景需要强制用别的 model */
  model?: string
  /** 超时毫秒，默认 60s。LLM 响应慢，DeepSeek 偶尔要 30s+ */
  timeoutMs?: number
  /** 可用工具列表，触发 function calling */
  tools?: ToolDefinition[]
  /** "auto"（默认）/ "none" / 指定函数。仅在 tools 存在时有意义 */
  toolChoice?: 'auto' | 'none' | { type: 'function'; function: { name: string } }
}

export interface ChatResponse {
  text: string
  /** 模型决定调用的工具。无则空数组 */
  toolCalls: ToolCall[]
  /** stop / tool_calls / length 等 */
  finishReason: string
  /** 入参 tokens（OpenAI 兼容字段 usage.prompt_tokens） */
  tokensIn: number
  /** 输出 tokens（usage.completion_tokens） */
  tokensOut: number
  model: string
  /** 厂商原始响应（debug 用，可能很长） */
  raw?: unknown
}

export class LlmClient {
  async chat(req: ChatRequest): Promise<ChatResponse> {
    const cred = getLlmCredentials()
    if (!cred.apiKey) {
      throw new Error('LLM apiKey 未配置（检查 ~/.jarvis/config.json 的 llm.apiKey 或 env LLM_API_KEY）')
    }
    if (!cred.baseUrl) {
      throw new Error('LLM baseUrl 未配置')
    }
    if (cred.wireApi === 'responses') {
      return chatViaResponses(req, cred)
    }
    return chatViaChatCompletions(req, cred)
  }
}

let _client: LlmClient | null = null
export function getLlmClient(): LlmClient {
  if (!_client) _client = new LlmClient()
  return _client
}

/**
 * 拼接最终请求 URL。endpoint 形如 'chat/completions' 或 'responses'（不带前导 /）。
 *
 * 厂商 baseUrl 约定五花八门：
 *   - DeepSeek 教写 `https://api.deepseek.com`（裸 host，需要我们补 /v1）
 *   - OpenAI / Codex CLI 写 `https://api.openai.com/v1`（已带 /v1）
 *   - 自建代理 / Codex 风格反代写 `http://host:port/codex` 或 `/openai` 等自定义前缀
 *     —— 这种 baseUrl 已经是"完整 API 前缀"，再补 /v1 就会 404
 *
 * 启发式：URL 的 pathname 只有 `/` 时认为是裸 host，补 `/v1/<endpoint>`；
 *        否则认为 baseUrl 已含完整前缀，直接 append `/<endpoint>`。
 */
function buildEndpointUrl(rawBase: string, endpoint: string): string {
  const trimmed = rawBase.replace(/\/+$/, '')
  let pathname = '/'
  try {
    pathname = new URL(trimmed).pathname || '/'
  } catch {
    // baseUrl 不是合法 URL —— 让 fetch 自己报错，这里保持原行为补 /v1
  }
  const hasCustomPrefix = pathname !== '/' && pathname !== ''
  return hasCustomPrefix
    ? `${trimmed}/${endpoint}`
    : `${trimmed}/v1/${endpoint}`
}

// ============================================================================
// Chat Completions 实现（/v1/chat/completions）
// ============================================================================

async function chatViaChatCompletions(
  req: ChatRequest,
  cred: ReturnType<typeof getLlmCredentials>,
): Promise<ChatResponse> {
  const url = buildEndpointUrl(cred.baseUrl, 'chat/completions')
  const model = req.model || cred.model
  const timeoutMs = req.timeoutMs ?? 60_000

  const controller = new AbortController()
  const timer = setTimeout(() => controller.abort(), timeoutMs)
  try {
    const body: Record<string, any> = {
      model,
      messages: req.messages,
      temperature: req.temperature ?? 0.3,
      max_tokens: req.maxTokens ?? 1024,
    }
    if (req.tools && req.tools.length > 0) {
      body.tools = req.tools
      body.tool_choice = req.toolChoice ?? 'auto'
    }
    const response = await fetch(url, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'Authorization': `Bearer ${cred.apiKey}`,
      },
      body: JSON.stringify(body),
      signal: controller.signal,
    })

    const text = await response.text()
    if (!response.ok) {
      throw new Error(`LLM HTTP ${response.status}: ${text.slice(0, 400)}`)
    }

    let data: any
    try {
      data = JSON.parse(text)
    } catch {
      throw new Error(`LLM 返回非 JSON: ${text.slice(0, 200)}`)
    }

    const choice = data?.choices?.[0]
    const message = choice?.message
    if (!message) {
      throw new Error(`LLM 响应缺 choices[0].message: ${text.slice(0, 200)}`)
    }
    // content 在 tool_calls 模式下可能是 null/空——这是 OpenAI 规范允许的
    const content: string = typeof message.content === 'string' ? message.content : ''
    const toolCalls: ToolCall[] = Array.isArray(message.tool_calls)
      ? message.tool_calls.filter((tc: any) => tc?.type === 'function' && tc?.function?.name)
      : []
    if (!content && toolCalls.length === 0) {
      throw new Error(`LLM 响应既无 content 也无 tool_calls: ${text.slice(0, 200)}`)
    }

    return {
      text: content,
      toolCalls,
      finishReason: choice?.finish_reason ?? 'stop',
      tokensIn: Number(data?.usage?.prompt_tokens) || 0,
      tokensOut: Number(data?.usage?.completion_tokens) || 0,
      model: data?.model || model,
      raw: data,
    }
  } catch (e: any) {
    if (e?.name === 'AbortError') {
      throw new Error(`LLM 请求超时（${timeoutMs}ms）`)
    }
    throw e
  } finally {
    clearTimeout(timer)
  }
}

// ============================================================================
// Responses API 实现（/v1/responses）
// ============================================================================

/**
 * 把内部 ChatMessage[] 转成 Responses API 的 input 数组。
 *
 * 规则：
 * - system/user/assistant(纯文本) → {type:"message", role, content}
 * - assistant 带 tool_calls → 先 emit message（若 content 非空），然后每个 tool call emit 一个
 *   {type:"function_call", call_id, name, arguments}
 * - tool 角色 → {type:"function_call_output", call_id, output}
 */
function messagesToResponsesInput(messages: ChatMessage[]): any[] {
  const out: any[] = []
  for (const m of messages) {
    if (m.role === 'tool') {
      if (!m.tool_call_id) {
        // Responses API 必须有 call_id 才能关联，缺了上游会报错；这里早抛便于定位
        throw new Error('tool 消息缺少 tool_call_id，Responses API 无法定位调用')
      }
      out.push({
        type: 'function_call_output',
        call_id: m.tool_call_id,
        output: m.content,
      })
      continue
    }
    if (m.role === 'assistant' && m.tool_calls && m.tool_calls.length > 0) {
      // 助手既可能 emit 文本，也可能 emit 一/多个工具调用——按顺序输出
      if (m.content && m.content.trim()) {
        out.push({ type: 'message', role: 'assistant', content: m.content })
      }
      for (const tc of m.tool_calls) {
        out.push({
          type: 'function_call',
          call_id: tc.id,
          name: tc.function.name,
          arguments: tc.function.arguments,
        })
      }
      continue
    }
    // system / user / assistant 纯文本
    out.push({ type: 'message', role: m.role, content: m.content })
  }
  return out
}

/**
 * Chat Completions 风格的 tools → Responses 扁平结构。
 *   {type:"function", function:{name,description,parameters}}
 *   → {type:"function", name, description, parameters}
 */
function toolsToResponsesFormat(tools: ToolDefinition[]): any[] {
  return tools.map(t => ({
    type: 'function',
    name: t.function.name,
    description: t.function.description,
    parameters: t.function.parameters,
  }))
}

function toolChoiceToResponses(
  tc: ChatRequest['toolChoice'],
): 'auto' | 'none' | 'required' | { type: 'function'; name: string } {
  if (!tc || tc === 'auto') return 'auto'
  if (tc === 'none') return 'none'
  if (typeof tc === 'object' && tc.type === 'function') {
    return { type: 'function', name: tc.function.name }
  }
  return 'auto'
}

async function chatViaResponses(
  req: ChatRequest,
  cred: ReturnType<typeof getLlmCredentials>,
): Promise<ChatResponse> {
  const url = buildEndpointUrl(cred.baseUrl, 'responses')
  const model = req.model || cred.model
  const timeoutMs = req.timeoutMs ?? 60_000

  const controller = new AbortController()
  const timer = setTimeout(() => controller.abort(), timeoutMs)
  try {
    const body: Record<string, any> = {
      model,
      input: messagesToResponsesInput(req.messages),
      temperature: req.temperature ?? 0.3,
      max_output_tokens: req.maxTokens ?? 1024,
    }
    if (req.tools && req.tools.length > 0) {
      body.tools = toolsToResponsesFormat(req.tools)
      body.tool_choice = toolChoiceToResponses(req.toolChoice)
    }
    const response = await fetch(url, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'Authorization': `Bearer ${cred.apiKey}`,
      },
      body: JSON.stringify(body),
      signal: controller.signal,
    })

    const text = await response.text()
    if (!response.ok) {
      throw new Error(`LLM HTTP ${response.status}: ${text.slice(0, 400)}`)
    }

    let data: any
    try {
      data = JSON.parse(text)
    } catch {
      throw new Error(`LLM 返回非 JSON: ${text.slice(0, 200)}`)
    }

    return parseResponsesOutput(data, model)
  } catch (e: any) {
    if (e?.name === 'AbortError') {
      throw new Error(`LLM 请求超时（${timeoutMs}ms）`)
    }
    throw e
  } finally {
    clearTimeout(timer)
  }
}

/**
 * 解析 Responses API 响应。output 是 item 数组，里面有 message 和 function_call 两类。
 *
 *   - message item: { type:"message", role:"assistant", content:[ {type:"output_text", text} ] }
 *     content 也可能是字符串（部分实现），都兼容
 *   - function_call item: { type:"function_call", call_id, name, arguments, status }
 *     这里映射回 Chat Completions 的 ToolCall.id = call_id
 *
 * usage 字段名是 input_tokens / output_tokens（不是 prompt_tokens）
 */
function parseResponsesOutput(data: any, fallbackModel: string): ChatResponse {
  const items: any[] = Array.isArray(data?.output) ? data.output : []
  let text = ''
  const toolCalls: ToolCall[] = []

  for (const item of items) {
    if (!item || typeof item !== 'object') continue
    const itemType = item.type
    if (itemType === 'message') {
      // content 可能是字符串或数组
      if (typeof item.content === 'string') {
        text += item.content
      } else if (Array.isArray(item.content)) {
        for (const part of item.content) {
          if (part?.type === 'output_text' && typeof part.text === 'string') {
            text += part.text
          } else if (typeof part === 'string') {
            text += part
          }
        }
      }
    } else if (itemType === 'function_call') {
      const name = item.name
      const args = typeof item.arguments === 'string' ? item.arguments : JSON.stringify(item.arguments ?? {})
      const callId = item.call_id || item.id
      if (name && callId) {
        toolCalls.push({
          id: callId,
          type: 'function',
          function: { name, arguments: args },
        })
      }
    }
    // 其它 item 类型（reasoning / refusal / web_search_call 等）当前不处理，直接忽略
  }

  // 兜底：data.output_text 字段（OpenAI 文档说明的"便利字段"，部分实现单独出）
  if (!text && typeof data?.output_text === 'string') {
    text = data.output_text
  }

  if (!text && toolCalls.length === 0) {
    throw new Error(`Responses 响应既无文本也无 tool_calls: ${JSON.stringify(data).slice(0, 300)}`)
  }

  const finishReason = toolCalls.length > 0
    ? 'tool_calls'
    : (data?.status === 'incomplete' ? 'length' : 'stop')

  return {
    text,
    toolCalls,
    finishReason,
    tokensIn: Number(data?.usage?.input_tokens) || 0,
    tokensOut: Number(data?.usage?.output_tokens) || 0,
    model: data?.model || fallbackModel,
    raw: data,
  }
}
