/**
 * OpenAI 兼容的 chat completions 客户端。
 *
 * DeepSeek / OpenAI / Moonshot / 大部分国产模型都兼容 `/v1/chat/completions`，
 * 这层只做最薄的转发 + 错误规整。零依赖（fetch + AbortController）。
 *
 * 不在这里做 prompt 拼装、retry、流式、缓存——这些都是上层的事。
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

/** OpenAI tools 字段格式 */
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

    const baseUrl = cred.baseUrl.replace(/\/+$/, '')
    const url = `${baseUrl}/v1/chat/completions`
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
        // 既无文本又无工具调用——异常，抛错让上层决定回退
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
}

let _client: LlmClient | null = null
export function getLlmClient(): LlmClient {
  if (!_client) _client = new LlmClient()
  return _client
}
