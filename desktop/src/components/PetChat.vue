<script setup lang="ts">
// 贴身迷你对话面板（方案 B1）：挂在 avatar 透明窗内的 DOM 面板，与小人同窗
// ——拖小人时面板天然跟随，不需要任何窗口管理。能力与 ChatApp 等价但 UI 极简：
// 无侧栏 / 无 markdown / 无待确认写入卡片，固定用最近更新的会话；
// 想切会话、看富文本、确认写入 → 菜单开聊天大窗（两者互斥，见 useWindowOrchestration）。
//
// 热区纪律：只有根元素挂 pointer-target（穿透判定依赖它精确）；
// 内部子元素不再重复标记（closest 会沿祖先链命中根元素）。
import { computed, nextTick, onMounted, onUnmounted, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { useConfigStore } from '../stores/config'
import {
  chatSendStream,
  generateConversationId,
  stripLocalFields,
  type ChatMessage,
  type ChatStreamEvent,
  type Conversation,
  type ConversationMeta,
} from '../api/chat'

const emit = defineEmits<{ close: [] }>()
const configStore = useConfigStore()

/** 性能保护：只渲染最近 200 条，更老的消息仍在会话里、只是不画 */
const RENDER_LIMIT = 200

const conversation = ref<Conversation | null>(null)
const inputText = ref('')
const isSending = ref(false)
/** 流式输出累积文本（纯视觉，不落盘），done 后清空 */
const streamingContent = ref('')
/** 工具状态行：「🔧 {name}」，只报工具名不渲染 preview 全文 */
const toolStatus = ref('')
/** 面板内错误行（不弹系统通知） */
const errorText = ref('')
const messagesEl = ref<HTMLElement | null>(null)
const inputEl = ref<HTMLTextAreaElement | null>(null)

const visibleMessages = computed<ChatMessage[]>(() =>
  conversation.value ? conversation.value.messages.slice(-RENDER_LIMIT) : [],
)

function scrollToBottom() {
  if (messagesEl.value) messagesEl.value.scrollTop = messagesEl.value.scrollHeight
}

/** 打开时定位到最近更新的会话；一个都没有才新建（标题「对话」）。
 *  Conversation 结构是后端强类型（conversations.rs），附加 source 字段会在
 *  save 时被 serde 丢弃，故不做标记——靠互斥逻辑即可区分场景。 */
async function loadLatestConversation() {
  try {
    const metas = await invoke<ConversationMeta[]>('conversations_list')
    if (metas.length > 0) {
      // conversations_list 已按 updatedAt 倒序，首个即最近
      conversation.value = await invoke<Conversation>('conversations_load', { id: metas[0].id })
    } else {
      const now = Date.now()
      conversation.value = {
        id: generateConversationId(),
        title: '对话',
        createdAt: now,
        updatedAt: now,
        messages: [],
      }
    }
  } catch (e) {
    console.error('迷你面板加载会话失败:', e)
    errorText.value = '会话加载失败，请稍后重试'
  }
}

async function send() {
  const text = inputText.value.trim()
  if (!text || isSending.value || !conversation.value) return
  const conv = conversation.value
  const now = Date.now()

  conv.messages.push({ role: 'user', content: text, createdAt: now })
  conv.updatedAt = now
  inputText.value = ''
  isSending.value = true
  errorText.value = ''
  toolStatus.value = ''
  await nextTick()
  scrollToBottom()

  try {
    await invoke('conversations_save', { conversation: conv })

    // chat:stream 是全窗口广播：与大窗的互斥（开本面板前先 chat_close）保证
    // 前台只有本面板一个消费者在渲染消息流，不会双计。反订阅句柄挂在
    // activeUnlisten 上：发送完成 or 面板半路被关（onUnmounted）都会即刻摘除，
    // 不留僵尸监听——否则大窗开回来发送时，广播会把两轮 agent 的 delta 串台。
    activeUnlisten = await listen<ChatStreamEvent>('chat:stream', (event) => {
      const p = event.payload
      if (p.type === 'delta' && typeof p.text === 'string') {
        streamingContent.value += p.text
        scrollToBottom()
      } else if (p.type === 'tool' && typeof p.name === 'string') {
        toolStatus.value = `🔧 ${p.name}`
        scrollToBottom()
      } else if (p.type === 'assistant' && p.hasToolCalls) {
        toolStatus.value = '🔍 准备查询数据…'
      }
    })

    try {
      const r = await chatSendStream({
        messages: conv.messages.map(stripLocalFields),
        assistantName: configStore.config.assistantName,
        userTitle: configStore.config.userTitle,
        conversationId: conv.id,
      })

      // 面板半路被关：已发生的请求等后端自然跑完即可，但不回写 UI/会话——
      // 此时大窗可能已加载了同一会话（互斥只保证「打开时」收起，管不着
      // 在途请求），迟到的 save 会用它那份旧视图把大窗里的新消息冲掉。
      // 用户消息在发送前已先行落盘，不会丢；这轮回复放弃保存。
      if (disposed) return
      // done 收口：清视觉层，用后端返回的真实消息（含工具调用）落进会话
      streamingContent.value = ''
      toolStatus.value = ''
      const baseTs = Date.now()
      for (let i = 0; i < r.newMessages.length; i++) {
        conv.messages.push({ ...r.newMessages[i], createdAt: baseTs + i })
      }
    } finally {
      detachStream()
    }

    conv.updatedAt = Date.now()
    await invoke('conversations_save', { conversation: conv })
    await nextTick()
    scrollToBottom()
  } catch (e) {
    streamingContent.value = ''
    toolStatus.value = ''
    console.error('迷你面板发送失败:', e)
    errorText.value = `发送失败：${e instanceof Error ? e.message : String(e)}`
  } finally {
    isSending.value = false
    inputEl.value?.focus()
  }
}

function onInputKeydown(e: KeyboardEvent) {
  // Enter 发送 / Shift+Enter 换行（Esc 由 window 级监听统一关闭面板）。
  // isComposing：中文输入法组词途中的 Enter 是「确认候选词」，不该当发送。
  if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) {
    e.preventDefault()
    send()
  }
}

// Esc 关面板（会话保留）。挂 window 捕获段：焦点在输入框、消息区、按钮上都能关，
// 不依赖某个元素持焦。面板随组件卸载一并摘除监听。
// isComposing：输入法组词途中 Esc 是「取消本次组词」，不该顺手把面板关了。
function onWindowKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape' && !e.isComposing) {
    e.stopPropagation()
    emit('close')
  }
}

// 面板被关（Esc/×/互斥收起）后在途请求的善后标记，见 send() 内说明
let disposed = false
/** 当前发送挂着的 chat:stream 反订阅（send 中赋值，完成或卸载时摘除） */
let activeUnlisten: (() => void) | null = null

function detachStream() {
  activeUnlisten?.()
  activeUnlisten = null
}

onMounted(async () => {
  window.addEventListener('keydown', onWindowKeydown, true)
  await loadLatestConversation()
  await nextTick()
  scrollToBottom()
  inputEl.value?.focus()
})

onUnmounted(() => {
  disposed = true
  detachStream()
  window.removeEventListener('keydown', onWindowKeydown, true)
})
</script>

<template>
  <div class="pet-chat pointer-target">
    <header class="pet-chat-header">
      <span class="pet-chat-title">💬 {{ configStore.config.assistantName }}</span>
      <button class="pet-chat-close" title="收起（Esc）" @click="emit('close')">×</button>
    </header>

    <div ref="messagesEl" class="pet-chat-messages">
      <div v-if="!conversation || visibleMessages.length === 0" class="pet-chat-empty">
        跟 {{ configStore.config.assistantName }} 聊点什么？
      </div>
      <template v-for="(msg, i) in visibleMessages" :key="`${msg.createdAt}-${i}`">
        <!-- tool 消息只画一行状态（不渲染 preview 全文，与流式 tool 事件同款） -->
        <div v-if="msg.role === 'tool'" class="pet-chat-line">
          🔧 {{ msg.name || '工具' }}
        </div>
        <div
          v-else-if="msg.role === 'assistant' && msg.tool_calls?.length && !msg.content"
          class="pet-chat-line"
        >
          → 调用 {{ msg.tool_calls.map(t => t.function.name).join('、') }}
        </div>
        <div v-else-if="msg.content" class="pet-chat-bubble" :class="`is-${msg.role}`">
          {{ msg.content }}
        </div>
      </template>
      <div v-if="toolStatus" class="pet-chat-line">{{ toolStatus }}</div>
      <div v-if="streamingContent" class="pet-chat-bubble is-assistant">{{ streamingContent }}</div>
      <div v-else-if="isSending" class="pet-chat-bubble is-assistant pet-chat-typing">思考中…</div>
      <div v-if="errorText" class="pet-chat-error">⚠️ {{ errorText }}</div>
    </div>

    <div class="pet-chat-input-row">
      <textarea
        ref="inputEl"
        v-model="inputText"
        class="pet-chat-input"
        rows="2"
        :disabled="isSending"
        placeholder="Enter 发送 · Shift+Enter 换行"
        @keydown="onInputKeydown"
      />
      <button
        class="pet-chat-send"
        :disabled="!inputText.trim() || isSending"
        @click="send"
      >
        {{ isSending ? '…' : '发送' }}
      </button>
    </div>
  </div>
</template>

<style scoped>
/* 定位基准由 App.vue 的 --petchat-* 变量按 data-anchor 翻转（同 --panel-* 体系），
   面板朝屏幕中心方向展开、贴小人对侧。 */
.pet-chat {
  position: fixed;
  inset: var(--petchat-top, auto) var(--petchat-right, auto) var(--petchat-bottom, auto) var(--petchat-left, auto);
  width: 320px;
  height: 440px;
  display: flex;
  flex-direction: column;
  z-index: 50;
  /* 透明主窗上的悬浮面板主背景必须用 --popup-bg（全部主题不透明），
     --panel-bg/--menu-bg 在极简主题近乎全透明，会透出桌面 */
  background: var(--popup-bg);
  border: var(--panel-border);
  border-radius: var(--radius-lg);
  box-shadow: var(--panel-shadow);
  overflow: hidden;
  color: var(--text);
}

.pet-chat-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 6px 8px 6px 12px;
  background: var(--panel-header-bg);
  border-bottom: var(--panel-header-border);
}
.pet-chat-title {
  font-size: 12px;
  font-weight: 600;
}
.pet-chat-close {
  width: 20px;
  height: 20px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  font-size: 15px;
  line-height: 1;
  color: var(--text-dim);
  background: transparent;
  border: none;
  border-radius: var(--radius-control);
  cursor: pointer;
}
.pet-chat-close:hover {
  color: var(--text);
  background: var(--surface-item-hover);
}

.pet-chat-messages {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: 10px;
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.pet-chat-empty {
  margin: auto;
  padding: 16px;
  text-align: center;
  font-size: 11.5px;
  color: var(--text-muted);
}
.pet-chat-line {
  align-self: flex-start;
  font-size: 10.5px;
  font-style: italic;
  color: var(--purple-text);
}
.pet-chat-bubble {
  max-width: 86%;
  padding: 6px 10px;
  border-radius: var(--radius-md);
  font-size: 12px;
  line-height: 1.55;
  white-space: pre-wrap;
  word-break: break-word;
}
.pet-chat-bubble.is-user {
  align-self: flex-end;
  background: color-mix(in srgb, var(--accent) 14%, transparent);
  border: 1px solid color-mix(in srgb, var(--accent) 28%, transparent);
}
.pet-chat-bubble.is-assistant {
  align-self: flex-start;
  background: color-mix(in srgb, var(--text) 4%, transparent);
  border: 1px solid var(--border);
}
.pet-chat-typing {
  color: var(--text-ghost);
  font-style: italic;
}
.pet-chat-error {
  align-self: stretch;
  padding: 5px 8px;
  font-size: 11px;
  line-height: 1.5;
  color: var(--red-text);
  background: var(--red-bg);
  border-radius: var(--radius-sm);
  word-break: break-word;
}

.pet-chat-input-row {
  display: flex;
  align-items: flex-end;
  gap: 6px;
  padding: 8px 10px 10px;
}
.pet-chat-input {
  flex: 1;
  min-width: 0;
  padding: 6px 8px;
  font-family: inherit;
  font-size: 12px;
  line-height: 1.45;
  color: var(--text);
  background: var(--input-bg);
  border: 1px solid var(--input-border);
  border-radius: var(--radius-md);
  resize: none;
  outline: none;
}
.pet-chat-input:focus {
  border-color: color-mix(in srgb, var(--accent) 50%, transparent);
}
.pet-chat-input:disabled {
  opacity: 0.6;
  cursor: not-allowed;
}
.pet-chat-send {
  padding: 6px 12px;
  font-size: 12px;
  color: var(--on-accent);
  background: color-mix(in srgb, var(--accent) 85%, transparent);
  border: none;
  border-radius: var(--radius-md);
  cursor: pointer;
}
.pet-chat-send:hover:not(:disabled) {
  background: var(--accent);
}
.pet-chat-send:disabled {
  background: var(--surface);
  color: var(--text-muted);
  cursor: not-allowed;
}
</style>
