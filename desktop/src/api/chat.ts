// 聊天链路共享类型与 invoke 封装。
//
// 从 ChatApp.vue 原样搬出（方案 B1-5），供聊天大窗（ChatApp.vue）与贴身迷你
// 面板（PetChat.vue）复用 —— 两处消费同一套 conversations + chat_send_stream
// 协议，类型只留一份避免漂移。只做搬运，不改任何行为。

import { invoke } from '@tauri-apps/api/core'

// ===== 会话持久化结构（对应 Rust conversations.rs，camelCase 序列化） =====

export interface ConversationMeta {
  id: string
  title: string
  createdAt: number
  updatedAt: number
  messageCount: number
}

export interface ChatMessage {
  role: 'system' | 'user' | 'assistant' | 'tool'
  content: string
  /** OpenAI 兼容字段：assistant 发起的工具调用 */
  tool_calls?: Array<{
    id: string
    type: 'function'
    function: { name: string; arguments: string }
  }>
  /** OpenAI 兼容字段：tool 消息对应的 call id */
  tool_call_id?: string
  /** OpenAI 兼容字段：tool 消息的工具名（可选） */
  name?: string
  /** 本地展示用 */
  createdAt: number
  /** 老格式兼容：占位 UI 阶段用过的 camelCase 字段，读老对话用 */
  toolCalls?: any
  toolCallId?: string
  pendingWrite?: PendingWrite
  writeStatus?: 'pending' | 'writing' | 'done' | 'cancelled' | 'failed'
  writeError?: string
  /** 成功后的补充说明（如发版触发的 queueId/buildNumber） */
  writeNote?: string
}

export interface Conversation {
  id: string
  title: string
  createdAt: number
  updatedAt: number
  messages: ChatMessage[]
}

export interface LogTaskEffortPayload {
  taskId: string
  hours: number
  work: string
  date?: string
}

export interface McpDeployPayload {
  server: string
  tool: string
  args: Record<string, any>
}

export type PendingWrite =
  | { kind: 'log-task-effort'; payload: LogTaskEffortPayload; summary: string }
  | { kind: 'mcp-deploy'; payload: McpDeployPayload; summary: string }

// ===== chat_send_stream 流式协议 =====

/**
 * 后端 chat.rs 广播的 StreamEvent（Rust chat_agent::StreamEvent，
 * serde tag="type" + camelCase）。avatar 窗与聊天窗都会收到同一广播。
 */
export interface ChatStreamEvent {
  type: 'delta' | 'assistant' | 'tool' | 'done'
  text?: string
  content?: string
  iteration?: number
  hasToolCalls?: boolean
  name?: string
  preview?: string
  tokensIn?: number
  tokensOut?: number
  truncated?: boolean
}

/** chat_send_stream 入参（对应 Rust ChatSendInput，字段 camelCase） */
export interface ChatSendInput {
  /** 全量历史。喂 LLM 前先过 stripLocalFields 去掉本地展示字段；
   *  unknown 而非 any——协议字段本就无静态形状，避免 lint any 告警 */
  messages: Array<Record<string, unknown>>
  assistantName?: string
  userTitle?: string
  conversationId?: string
}

export interface ChatSendResult {
  newMessages: ChatMessage[]
  steps?: number
  tokensIn: number
  tokensOut: number
  truncated: boolean
  allowedTools?: string[]
}

/** chat_send_stream 类型化 wrapper：参数名收口在这一处，防止调用方拼错 */
export function chatSendStream(input: ChatSendInput): Promise<ChatSendResult> {
  return invoke<ChatSendResult>('chat_send_stream', { input })
}

// ===== 共享小工具 =====

/** 会话 id：timestamp + 短随机，与 Rust 侧 validate_id 白名单兼容 */
export function generateConversationId(): string {
  const d = new Date()
  const pad = (n: number) => String(n).padStart(2, '0')
  const ts = `${d.getFullYear()}${pad(d.getMonth() + 1)}${pad(d.getDate())}-${pad(d.getHours())}${pad(d.getMinutes())}${pad(d.getSeconds())}`
  const rand = Math.random().toString(36).slice(2, 6)
  return `${ts}-${rand}`
}

/** 喂 LLM 的消息只保留协议字段，去掉本地 createdAt 等 */
export function stripLocalFields(m: ChatMessage): Record<string, any> {
  const out: Record<string, any> = { role: m.role, content: m.content }
  if (m.tool_calls) out.tool_calls = m.tool_calls
  if (m.toolCallId) out.tool_call_id = m.toolCallId  // 兼容老格式
  if ((m as any).tool_call_id) out.tool_call_id = (m as any).tool_call_id
  if ((m as any).name) out.name = (m as any).name
  return out
}
