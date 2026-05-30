<script setup lang="ts">
// Chat 主窗口：侧栏会话列表 + 右侧消息流 + 底部输入框。
//
// 持久化全部走 Rust：conversations_list/load/save/delete。
// 发送消息这一步现在只是占位（追加 user 消息+保存+伪 assistant 回复），
// #47/#48 接入 agent 之后会用真实 LLM/工具调用替换 sendMessage 末尾的占位。
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue'
import type { Directive } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { invoke } from '@tauri-apps/api/core'
import { useConfigStore } from './stores/config'
import ErrorBoundary from './components/ErrorBoundary.vue'

// rename input 出现时自动 focus + select。Vue 3 <script setup> 里 v 前缀的常量
// 自动被识别为模板里的 v-focus 指令
const vFocus: Directive<HTMLInputElement> = {
  mounted(el) { el.focus(); el.select() },
}

const configStore = useConfigStore()

// ===== 类型 =====
interface ConversationMeta {
  id: string
  title: string
  createdAt: number
  updatedAt: number
  messageCount: number
}
interface ChatMessage {
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
}
interface Conversation {
  id: string
  title: string
  createdAt: number
  updatedAt: number
  messages: ChatMessage[]
}
interface PendingWrite {
  kind: 'log-task-effort'
  payload: {
    taskId: string
    hours: number
    work: string
    date?: string
  }
  summary: string
}

// ===== 状态 =====
const conversations = ref<ConversationMeta[]>([])
const currentId = ref<string | null>(null)
const currentConversation = ref<Conversation | null>(null)
const inputText = ref('')
const isSending = ref(false)
const renamingId = ref<string | null>(null)
const renamingValue = ref('')
const messagesEl = ref<HTMLElement | null>(null)
/** 已展开的 tool 消息索引（按当前对话内的下标）。切换对话时清空 */
const expandedToolMsgs = ref<Set<number>>(new Set())

const sortedConversations = computed(() =>
  [...conversations.value].sort((a, b) => b.updatedAt - a.updatedAt),
)

// ===== 数据加载 =====
async function refreshList() {
  try {
    conversations.value = await invoke<ConversationMeta[]>('conversations_list')
  } catch (e) {
    console.error('加载会话列表失败:', e)
    conversations.value = []
  }
}

async function selectConversation(id: string) {
  if (currentId.value === id && currentConversation.value) return
  try {
    const conv = await invoke<Conversation>('conversations_load', { id })
    currentId.value = id
    currentConversation.value = conv
    expandedToolMsgs.value = new Set()
    await nextTick()
    scrollToBottom()
  } catch (e) {
    console.error('加载会话失败:', e)
  }
}

function newConversation() {
  // 不立刻落盘——等用户发第一条消息时再 save。空会话不污染侧栏。
  const id = generateId()
  const now = Date.now()
  currentId.value = id
  currentConversation.value = {
    id, title: '新对话', createdAt: now, updatedAt: now, messages: [],
  }
  inputText.value = ''
}

function pendingWriteFromToolMessage(m: ChatMessage): PendingWrite | null {
  if (m.role !== 'tool' || m.name !== 'prepare-log-task-effort') return null
  try {
    const parsed = JSON.parse(m.content)
    if (!parsed?.pendingWrite || parsed.kind !== 'log-task-effort') return null
    const payload = parsed.payload ?? {}
    if (!payload.taskId || !payload.hours || !payload.work) return null
    return {
      kind: 'log-task-effort',
      payload: {
        taskId: String(payload.taskId),
        hours: Number(payload.hours),
        work: String(payload.work),
        date: payload.date ? String(payload.date) : undefined,
      },
      summary: String(parsed.summary || ''),
    }
  } catch {
    return null
  }
}

async function confirmPendingWrite(msg: ChatMessage) {
  if (!currentConversation.value || !msg.pendingWrite) return
  if (msg.writeStatus !== 'pending' && msg.writeStatus !== 'failed') return
  msg.writeStatus = 'writing'
  msg.writeError = undefined
  try {
    const r = await invoke<{ success: boolean; data?: any; error?: string }>('tool_execute', {
      name: 'log-task-effort',
      input: msg.pendingWrite.payload,
    })
    if (!r.success) {
      msg.writeStatus = 'failed'
      msg.writeError = r.error || '写入失败'
    } else {
      msg.writeStatus = 'done'
    }
  } catch (e: any) {
    msg.writeStatus = 'failed'
    msg.writeError = String(e?.message ?? e)
  }
  currentConversation.value.updatedAt = Date.now()
  await invoke('conversations_save', { conversation: currentConversation.value })
  await nextTick()
  scrollToBottom()
}

async function cancelPendingWrite(msg: ChatMessage) {
  if (!currentConversation.value || !msg.pendingWrite || msg.writeStatus !== 'pending') return
  msg.writeStatus = 'cancelled'
  currentConversation.value.updatedAt = Date.now()
  await invoke('conversations_save', { conversation: currentConversation.value })
}

async function deleteConversation(id: string) {
  if (!confirm('确定删除这个对话？不可恢复')) return
  try {
    await invoke('conversations_delete', { id })
    conversations.value = conversations.value.filter(c => c.id !== id)
    if (currentId.value === id) {
      currentId.value = null
      currentConversation.value = null
    }
  } catch (e) {
    console.error('删除失败:', e)
  }
}

function startRename(meta: ConversationMeta) {
  renamingId.value = meta.id
  renamingValue.value = meta.title
}
async function commitRename() {
  if (!renamingId.value) return
  const id = renamingId.value
  const title = renamingValue.value.trim() || '未命名'
  renamingId.value = null
  // 改在已加载的 currentConversation 上，没加载则单独 load+save
  let conv: Conversation
  if (currentConversation.value && currentConversation.value.id === id) {
    currentConversation.value.title = title
    currentConversation.value.updatedAt = Date.now()
    conv = currentConversation.value
  } else {
    conv = await invoke<Conversation>('conversations_load', { id })
    conv.title = title
    conv.updatedAt = Date.now()
  }
  try {
    await invoke('conversations_save', { conversation: conv })
    await refreshList()
  } catch (e) {
    console.error('重命名失败:', e)
  }
}

// ===== 发送消息 → 调 chat_send 工具跑 agent loop =====
async function sendMessage() {
  const text = inputText.value.trim()
  if (!text || isSending.value || !currentConversation.value) return

  const conv = currentConversation.value
  const now = Date.now()

  // 1. 追加 user
  conv.messages.push({ role: 'user', content: text, createdAt: now })
  // 第一条消息自动生成标题
  if (conv.messages.filter(m => m.role === 'user').length === 1) {
    conv.title = text.slice(0, 20) || '新对话'
  }
  conv.updatedAt = now
  inputText.value = ''
  isSending.value = true
  await nextTick()
  scrollToBottom()

  try {
    await invoke('conversations_save', { conversation: conv })
    await refreshList()

    // 2. 跑 agent。喂 LLM 的消息只保留 role + content + tool 字段，去掉 createdAt 等本地字段
    const llmMessages = conv.messages.map(m => stripLocalFields(m))
    const r = await invoke<{
      success: boolean
      data?: { newMessages: any[]; tokensIn: number; tokensOut: number; truncated: boolean }
      error?: string
    }>('tool_execute', {
      name: 'chat_send',
      input: {
        messages: llmMessages,
        assistantName: configStore.config.assistantName,
        userTitle: configStore.config.userTitle,
      },
    })

    if (!r.success || !r.data) {
      conv.messages.push({
        role: 'assistant',
        content: `（调用失败：${r.error || '未知错误'}。检查 LLM 配置是否填好。）`,
        createdAt: Date.now(),
      })
    } else {
      // 把 agent 新生成的所有消息（assistant + tool）追加到对话，并加上本地 createdAt
      const baseTs = Date.now()
      for (let i = 0; i < r.data.newMessages.length; i++) {
        const m = r.data.newMessages[i]
        const next: ChatMessage = { ...m, createdAt: baseTs + i }
        const pendingWrite = pendingWriteFromToolMessage(next)
        if (pendingWrite) {
          next.pendingWrite = pendingWrite
          next.writeStatus = 'pending'
        }
        conv.messages.push(next)
      }
    }
    conv.updatedAt = Date.now()
    await invoke('conversations_save', { conversation: conv })
    await refreshList()
    await nextTick()
    scrollToBottom()
  } catch (e: any) {
    console.error('发送失败:', e)
    conv.messages.push({
      role: 'assistant',
      content: `（系统错误：${String(e?.message ?? e)}）`,
      createdAt: Date.now(),
    })
    await invoke('conversations_save', { conversation: conv })
    await nextTick()
    scrollToBottom()
  } finally {
    isSending.value = false
  }
}

// ===== 工具 =====
function generateId(): string {
  const d = new Date()
  const pad = (n: number) => String(n).padStart(2, '0')
  const ts = `${d.getFullYear()}${pad(d.getMonth() + 1)}${pad(d.getDate())}-${pad(d.getHours())}${pad(d.getMinutes())}${pad(d.getSeconds())}`
  const rand = Math.random().toString(36).slice(2, 6)
  return `${ts}-${rand}`
}

/** 喂 LLM 的消息只保留协议字段，去掉本地 createdAt 等 */
function stripLocalFields(m: ChatMessage): Record<string, any> {
  const out: Record<string, any> = { role: m.role, content: m.content }
  if (m.tool_calls) out.tool_calls = m.tool_calls
  if (m.toolCallId) out.tool_call_id = m.toolCallId  // 兼容老格式
  if ((m as any).tool_call_id) out.tool_call_id = (m as any).tool_call_id
  if ((m as any).name) out.name = (m as any).name
  return out
}

function scrollToBottom() {
  if (messagesEl.value) messagesEl.value.scrollTop = messagesEl.value.scrollHeight
}

function formatTime(ts: number): string {
  const d = new Date(ts)
  return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`
}

/** Tool 消息默认折叠。展示一行摘要 + 尺寸；展开时 pretty-print JSON */
function toolMsgPreview(content: string): string {
  const sz = formatSize(content.length)
  // 试着解 JSON 给个简洁摘要
  try {
    const v = JSON.parse(content)
    if (Array.isArray(v)) return `📦 数组 · ${v.length} 项 · ${sz}`
    if (v && typeof v === 'object') {
      if (typeof v.error === 'string') return `❌ ${v.error.slice(0, 60)} · ${sz}`
      const keys = Object.keys(v).slice(0, 4).join(', ')
      return `📦 {${keys}${Object.keys(v).length > 4 ? ', …' : ''}} · ${sz}`
    }
    return `📦 ${String(v).slice(0, 60)} · ${sz}`
  } catch {
    return `📄 ${content.split('\n')[0].slice(0, 60)} · ${sz}`
  }
}

function toolMsgFormatted(content: string): string {
  try {
    const v = JSON.parse(content)
    return JSON.stringify(v, null, 2)
  } catch {
    return content
  }
}

function formatSize(n: number): string {
  if (n < 1024) return `${n} 字`
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)}KB`
  return `${(n / 1024 / 1024).toFixed(1)}MB`
}

function toggleToolExpanded(idx: number) {
  const s = new Set(expandedToolMsgs.value)
  if (s.has(idx)) s.delete(idx)
  else s.add(idx)
  expandedToolMsgs.value = s
}

function onInputKeydown(e: KeyboardEvent) {
  // Enter 发送 / Shift+Enter 换行
  if (e.key === 'Enter' && !e.shiftKey) {
    e.preventDefault()
    sendMessage()
  }
}

// ===== 关闭窗口拦截 =====
async function handleClose() {
  await invoke('chat_close')
}

let cleanup: (() => void) | null = null
onMounted(async () => {
  await configStore.load()
  document.title = `${configStore.config.assistantName} · 对话`
  await refreshList()
  // 自动选最近一个；没有就建新的
  if (sortedConversations.value.length > 0) {
    await selectConversation(sortedConversations.value[0].id)
  } else {
    newConversation()
  }
  const win = getCurrentWindow()
  cleanup = await win.onCloseRequested(async (e) => {
    e.preventDefault()
    await handleClose()
  })
})
onUnmounted(() => { cleanup?.() })

watch(() => configStore.config.assistantName, (n) => {
  if (n) document.title = `${n} · 对话`
})
</script>

<template>
  <ErrorBoundary>
  <div class="chat-root">
    <!-- 头部（可拖动） -->
    <header class="chat-header">
      <span class="title">{{ configStore.config.assistantName }} · 对话</span>
      <button class="close-btn" @click="handleClose" title="切回小窗">×</button>
    </header>

    <div class="chat-body">
      <!-- 左侧栏 -->
      <aside class="sidebar">
        <button class="new-btn" @click="newConversation">+ 新对话</button>
        <ul class="conv-list">
          <li v-for="meta in sortedConversations" :key="meta.id"
            class="conv-item"
            :class="{ active: meta.id === currentId }"
            @click="selectConversation(meta.id)">
            <div v-if="renamingId === meta.id" class="rename-row">
              <input class="rename-input" v-model="renamingValue"
                @keydown.enter="commitRename"
                @keydown.esc="renamingId = null"
                @blur="commitRename"
                @click.stop
                v-focus />
            </div>
            <template v-else>
              <div class="conv-title" @dblclick.stop="startRename(meta)">{{ meta.title }}</div>
              <div class="conv-meta">{{ meta.messageCount }} 条 · {{ formatTime(meta.updatedAt) }}</div>
            </template>
            <button class="conv-del" @click.stop="deleteConversation(meta.id)" title="删除">×</button>
          </li>
          <li v-if="sortedConversations.length === 0" class="empty-hint">还没有对话</li>
        </ul>
      </aside>

      <!-- 右侧主区 -->
      <main class="main-pane">
        <div v-if="!currentConversation" class="empty-state">
          <p>选择左侧对话或新建一个</p>
        </div>
        <template v-else>
          <div ref="messagesEl" class="messages">
            <div v-if="currentConversation.messages.length === 0" class="empty-state">
              <p>跟 {{ configStore.config.assistantName }} 聊点什么？</p>
              <p class="hint">例如："今天有哪些任务要做？"、"分析下我现在的风险"</p>
            </div>
            <div v-for="(msg, i) in currentConversation.messages" :key="msg.createdAt"
              class="msg" :class="`msg-${msg.role}`">
              <!-- tool 消息：折叠+格式化 -->
              <template v-if="msg.role === 'tool'">
                <div v-if="msg.pendingWrite" class="pending-write">
                  <div class="msg-role">
                    <span>待确认写入</span>
                    <span class="msg-time">{{ formatTime(msg.createdAt) }}</span>
                  </div>
                  <pre class="pending-summary">{{ msg.pendingWrite.summary }}</pre>
                  <div class="pending-actions">
                    <button
                      class="pending-btn pending-btn-primary"
                      :disabled="msg.writeStatus === 'writing' || msg.writeStatus === 'done' || msg.writeStatus === 'cancelled'"
                      @click="confirmPendingWrite(msg)"
                    >
                      {{ msg.writeStatus === 'done' ? '已写入' : msg.writeStatus === 'writing' ? '写入中' : msg.writeStatus === 'failed' ? '重试写入' : '确认写入' }}
                    </button>
                    <button
                      class="pending-btn"
                      :disabled="msg.writeStatus !== 'pending' && msg.writeStatus !== 'failed'"
                      @click="cancelPendingWrite(msg)"
                    >
                      {{ msg.writeStatus === 'cancelled' ? '已取消' : '取消' }}
                    </button>
                  </div>
                  <p v-if="msg.writeStatus === 'done'" class="pending-note ok">已写入禅道。</p>
                  <p v-else-if="msg.writeStatus === 'cancelled'" class="pending-note">已取消，这次不会写入。</p>
                  <p v-else-if="msg.writeStatus === 'failed'" class="pending-note fail">{{ msg.writeError }}</p>
                  <p v-else-if="msg.writeStatus === 'writing'" class="pending-note">正在写入禅道…</p>
                </div>
                <template v-else>
                  <div class="msg-role tool-header" @click="toggleToolExpanded(i)">
                    <span class="tool-toggle">{{ expandedToolMsgs.has(i) ? '▾' : '▸' }}</span>
                    <span>🔧 {{ msg.name || '工具' }}</span>
                    <span class="msg-time">{{ formatTime(msg.createdAt) }}</span>
                  </div>
                  <div v-if="!expandedToolMsgs.has(i)" class="msg-content tool-preview">
                    {{ toolMsgPreview(msg.content) }}
                  </div>
                  <pre v-else class="msg-content tool-expanded">{{ toolMsgFormatted(msg.content) }}</pre>
                </template>
              </template>
              <!-- user / assistant -->
              <template v-else>
                <div class="msg-role">
                  {{ msg.role === 'user' ? '我' : msg.role === 'assistant' ? configStore.config.assistantName : msg.role }}
                  <span class="msg-time">{{ formatTime(msg.createdAt) }}</span>
                </div>
                <div class="msg-content" v-if="msg.content">{{ msg.content }}</div>
                <div class="msg-content tool-call-hint"
                  v-else-if="msg.role === 'assistant' && msg.tool_calls && msg.tool_calls.length">
                  正在调用 {{ msg.tool_calls.map(t => t.function.name).join('、') }}…
                </div>
              </template>
            </div>
            <div v-if="isSending" class="msg msg-assistant">
              <div class="msg-role">{{ configStore.config.assistantName }}</div>
              <div class="msg-content typing">思考中…</div>
            </div>
          </div>

          <div class="input-area">
            <textarea
              class="input-box"
              v-model="inputText"
              :disabled="isSending"
              placeholder="Enter 发送 · Shift+Enter 换行"
              rows="3"
              @keydown="onInputKeydown"
            />
            <button class="send-btn" @click="sendMessage"
              :disabled="!inputText.trim() || isSending">
              {{ isSending ? '…' : '发送' }}
            </button>
          </div>
        </template>
      </main>
    </div>
  </div>
  </ErrorBoundary>
</template>

<style scoped>
.chat-root {
  height: 100vh;
  display: flex;
  flex-direction: column;
  background: linear-gradient(135deg, rgba(20, 30, 56, 1), rgba(15, 23, 42, 1));
  color: rgba(255, 255, 255, 0.92);
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif;
}

.chat-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 10px 14px;
  background: rgba(0, 0, 0, 0.25);
  border-bottom: 1px solid rgba(255, 255, 255, 0.06);
  -webkit-app-region: drag;
  user-select: none;
}
.title { font-size: 13px; font-weight: 600; }
.close-btn {
  width: 24px; height: 24px;
  display: inline-flex; align-items: center; justify-content: center;
  font-size: 18px; line-height: 1;
  color: rgba(255, 255, 255, 0.6);
  background: transparent; border: none; border-radius: 6px;
  cursor: pointer;
  -webkit-app-region: no-drag;
}
.close-btn:hover { color: #fff; background: rgba(255, 255, 255, 0.08); }

.chat-body {
  flex: 1;
  display: flex;
  min-height: 0;
}

.sidebar {
  width: 220px;
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  background: rgba(0, 0, 0, 0.18);
  border-right: 1px solid rgba(255, 255, 255, 0.06);
}
.new-btn {
  margin: 10px;
  padding: 8px 12px;
  font-size: 12px;
  color: rgba(0, 212, 255, 0.95);
  background: rgba(0, 212, 255, 0.12);
  border: 1px solid rgba(0, 212, 255, 0.35);
  border-radius: 6px;
  cursor: pointer;
}
.new-btn:hover { background: rgba(0, 212, 255, 0.2); }

.conv-list {
  flex: 1;
  list-style: none;
  margin: 0;
  padding: 0 6px 10px;
  overflow-y: auto;
}
.conv-item {
  position: relative;
  padding: 8px 28px 8px 10px;
  margin-bottom: 2px;
  border-radius: 6px;
  cursor: pointer;
  font-size: 12px;
  transition: background 0.12s;
}
.conv-item:hover { background: rgba(255, 255, 255, 0.05); }
.conv-item.active { background: rgba(0, 212, 255, 0.12); }
.conv-title {
  color: rgba(255, 255, 255, 0.92);
  font-weight: 500;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.conv-meta {
  margin-top: 2px;
  font-size: 10.5px;
  color: rgba(255, 255, 255, 0.4);
}
.conv-del {
  position: absolute;
  top: 50%;
  right: 6px;
  transform: translateY(-50%);
  width: 18px; height: 18px;
  display: inline-flex; align-items: center; justify-content: center;
  font-size: 14px;
  color: rgba(255, 255, 255, 0.35);
  background: transparent;
  border: none;
  border-radius: 4px;
  cursor: pointer;
  opacity: 0;
  transition: opacity 0.12s;
}
.conv-item:hover .conv-del { opacity: 1; }
.conv-del:hover { color: rgba(239, 68, 68, 0.95); background: rgba(239, 68, 68, 0.12); }
.rename-input {
  width: 100%;
  padding: 3px 6px;
  font-size: 12px;
  color: rgba(255, 255, 255, 0.95);
  background: rgba(255, 255, 255, 0.08);
  border: 1px solid rgba(0, 212, 255, 0.5);
  border-radius: 4px;
}
.empty-hint {
  padding: 12px;
  font-size: 11px;
  color: rgba(255, 255, 255, 0.35);
  text-align: center;
}

.main-pane {
  flex: 1;
  display: flex;
  flex-direction: column;
  min-width: 0;
}

.messages {
  flex: 1;
  overflow-y: auto;
  padding: 16px 20px;
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.empty-state {
  margin: auto;
  text-align: center;
  color: rgba(255, 255, 255, 0.45);
  font-size: 13px;
}
.empty-state .hint {
  margin-top: 8px;
  font-size: 11.5px;
  color: rgba(255, 255, 255, 0.3);
}

.msg {
  max-width: 78%;
  padding: 8px 12px;
  border-radius: 10px;
  font-size: 13px;
  line-height: 1.55;
}
.msg-role {
  display: flex;
  align-items: baseline;
  gap: 6px;
  margin-bottom: 4px;
  font-size: 10.5px;
  color: rgba(255, 255, 255, 0.5);
  font-weight: 500;
}
.msg-time { color: rgba(255, 255, 255, 0.3); }
.msg-content {
  white-space: pre-wrap;
  word-break: break-word;
  color: rgba(255, 255, 255, 0.92);
}
.msg-user {
  align-self: flex-end;
  background: rgba(0, 212, 255, 0.14);
  border: 1px solid rgba(0, 212, 255, 0.28);
}
.msg-user .msg-role { justify-content: flex-end; }
.msg-assistant {
  align-self: flex-start;
  background: rgba(255, 255, 255, 0.04);
  border: 1px solid rgba(255, 255, 255, 0.08);
}
.msg-tool {
  align-self: flex-start;
  background: rgba(168, 85, 247, 0.08);
  border: 1px solid rgba(168, 85, 247, 0.25);
  font-family: ui-monospace, monospace;
  font-size: 11.5px;
}
.tool-header {
  cursor: pointer;
  user-select: none;
}
.tool-header:hover { color: rgba(255, 255, 255, 0.75); }
.tool-toggle {
  display: inline-block;
  width: 12px;
  color: rgba(168, 85, 247, 0.9);
  font-family: ui-monospace, monospace;
}
.tool-preview {
  color: rgba(255, 255, 255, 0.6);
  font-style: italic;
}
.tool-expanded {
  margin: 0;
  padding: 6px 8px;
  background: rgba(0, 0, 0, 0.3);
  border-radius: 4px;
  max-height: 400px;
  overflow: auto;
  white-space: pre-wrap;
  word-break: break-word;
  font-family: ui-monospace, monospace;
  font-size: 11px;
  line-height: 1.45;
  color: rgba(255, 255, 255, 0.85);
}
.msg-system { display: none; }   /* 系统消息不可见 */

.pending-write {
  min-width: min(420px, 72vw);
}
.pending-summary {
  margin: 4px 0 10px;
  padding: 10px 12px;
  color: rgba(255, 255, 255, 0.9);
  background: rgba(0, 0, 0, 0.24);
  border: 1px solid rgba(255, 255, 255, 0.08);
  border-radius: 8px;
  white-space: pre-wrap;
  word-break: break-word;
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  font-size: 12px;
  line-height: 1.55;
}
.pending-actions {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
}
.pending-btn {
  height: 30px;
  padding: 0 12px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  font-family: inherit;
  font-size: 12px;
  font-weight: 600;
  color: rgba(255, 255, 255, 0.82);
  background: rgba(255, 255, 255, 0.07);
  border: 1px solid rgba(255, 255, 255, 0.12);
  border-radius: 7px;
  cursor: pointer;
}
.pending-btn:hover:not(:disabled) {
  color: #fff;
  background: rgba(255, 255, 255, 0.12);
}
.pending-btn-primary {
  color: rgba(8, 20, 34, 0.96);
  background: rgba(0, 212, 255, 0.9);
  border-color: rgba(0, 212, 255, 0.18);
}
.pending-btn-primary:hover:not(:disabled) {
  color: rgba(8, 20, 34, 0.96);
  background: rgba(44, 225, 255, 1);
}
.pending-btn:disabled {
  opacity: 0.48;
  cursor: not-allowed;
}
.pending-note {
  margin: 8px 0 0;
  color: rgba(255, 255, 255, 0.55);
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif;
  font-size: 12px;
}
.pending-note.ok { color: rgba(52, 211, 153, 0.95); }
.pending-note.fail { color: rgba(248, 113, 113, 0.95); }

.typing {
  color: rgba(255, 255, 255, 0.55);
  font-style: italic;
}
.tool-call-hint {
  color: rgba(168, 85, 247, 0.9);
  font-size: 11.5px;
  font-style: italic;
}

.input-area {
  padding: 10px 14px 14px;
  background: rgba(0, 0, 0, 0.18);
  border-top: 1px solid rgba(255, 255, 255, 0.06);
  display: flex;
  gap: 8px;
  align-items: flex-end;
}
.input-box {
  flex: 1;
  padding: 8px 10px;
  font-family: inherit;
  font-size: 13px;
  color: rgba(255, 255, 255, 0.92);
  background: rgba(255, 255, 255, 0.06);
  border: 1px solid rgba(255, 255, 255, 0.1);
  border-radius: 8px;
  resize: none;
  outline: none;
  line-height: 1.45;
}
.input-box:focus {
  border-color: rgba(0, 212, 255, 0.5);
  background: rgba(0, 212, 255, 0.05);
}
.input-box:disabled { opacity: 0.6; cursor: not-allowed; }
.send-btn {
  padding: 8px 18px;
  height: 38px;
  font-size: 13px;
  font-weight: 500;
  color: #fff;
  background: rgba(0, 212, 255, 0.85);
  border: none;
  border-radius: 8px;
  cursor: pointer;
}
.send-btn:hover:not(:disabled) { background: rgba(0, 212, 255, 1); }
.send-btn:disabled {
  background: rgba(255, 255, 255, 0.08);
  color: rgba(255, 255, 255, 0.3);
  cursor: not-allowed;
}
</style>
