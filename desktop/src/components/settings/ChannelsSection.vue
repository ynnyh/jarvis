<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useConfigStore } from '../../stores/config'

const store = useConfigStore()
const status = ref<{ running: boolean; message: string }>({ running: false, message: '未启动' })
const busy = ref(false)
const serviceMessage = ref('')
const telegramProbeState = ref<'idle' | 'checking' | 'ok' | 'fail'>('idle')
const telegramProbeMessage = ref('')
const telegramBotName = ref('')
const qqProbeState = ref<'idle' | 'checking' | 'ok' | 'fail'>('idle')
const qqProbeMessage = ref('')
const qqGateway = ref('')
const hasTelegramToken = () => !!store.config.channels.telegram.botToken?.trim()
const hasQqSecret = () => !!store.config.channels.qqbot.appSecret?.trim()
const telegramRecentChats = ref<Array<{
  chatId: string
  chatType?: string
  title?: string
  fromId?: string
  fromName?: string
  text?: string
}>>([])

function parseList(text: string): string[] {
  return text
    .split(/[\n,，\s]+/)
    .map(s => s.trim())
    .filter(Boolean)
}

function listText(list: string[]): string {
  return (list ?? []).join('\n')
}

async function refreshStatus() {
  try {
    status.value = await invoke('channel_status')
  } catch (e: any) {
    status.value = { running: false, message: String(e?.message ?? e) }
  }
}

async function startService() {
  busy.value = true
  serviceMessage.value = ''
  store.config.channels.autoStart = true
  await store.save()
  try {
    status.value = await invoke('channels_start')
    serviceMessage.value = status.value.message
  } catch (e: any) {
    serviceMessage.value = String(e?.message ?? e)
  } finally {
    busy.value = false
  }
}

async function restartService() {
  busy.value = true
  serviceMessage.value = ''
  store.config.channels.autoStart = true
  await store.save()
  try {
    await invoke('channels_stop')
    // channels_stop 只发停止信号即返回，不等后台 gateway task 真正退出。
    // 留一段缓冲让旧 task 退出，否则新旧 task 短暂并存会让 Telegram getUpdates 撞 409 冲突。
    await new Promise(resolve => window.setTimeout(resolve, 500))
    status.value = await invoke('channels_start')
    serviceMessage.value = status.value.message
  } catch (e: any) {
    serviceMessage.value = String(e?.message ?? e)
  } finally {
    busy.value = false
  }
}

async function startTelegramOnly() {
  store.config.channels.telegram.enabled = true
  if (status.value.running) {
    await restartService()
  } else {
    await startService()
  }
}

async function startQqOnly() {
  store.config.channels.qqbot.enabled = true
  if (status.value.running) {
    await restartService()
  } else {
    await startService()
  }
}

async function stopService() {
  busy.value = true
  serviceMessage.value = ''
  store.config.channels.autoStart = false
  await store.save()
  try {
    status.value = await invoke('channels_stop')
    serviceMessage.value = status.value.message
  } catch (e: any) {
    serviceMessage.value = String(e?.message ?? e)
  } finally {
    busy.value = false
  }
}

async function checkTelegram() {
  telegramProbeState.value = 'checking'
  telegramProbeMessage.value = ''
  telegramBotName.value = ''
  telegramRecentChats.value = []
  try {
    const result = await invoke<{
      ok: boolean
      botUsername?: string
      botName?: string
      recentChats: Array<{
        chatId: string
        chatType?: string
        title?: string
        fromId?: string
        fromName?: string
        text?: string
      }>
      message: string
    }>('telegram_probe', {
      botToken: store.config.channels.telegram.botToken,
      apiBaseUrl: store.config.channels.telegram.apiBaseUrl,
      proxy: store.config.channels.telegram.proxy,
    })
    telegramProbeState.value = result.ok ? 'ok' : 'fail'
    telegramProbeMessage.value = result.message
    telegramBotName.value = result.botUsername ? `@${result.botUsername}` : (result.botName ?? '')
    telegramRecentChats.value = result.recentChats ?? []
    if (result.ok) {
      store.config.channels.telegram.enabled = true
      if (!status.value.running) {
        telegramProbeMessage.value += '\n下一步：点击本块里的“启动 Telegram”，然后在 Telegram 里给机器人发消息。'
      } else {
        telegramProbeMessage.value += '\n渠道服务正在运行。如果刚刚改过 token、代理或白名单，请点击“重启 Telegram”。'
      }
    }
  } catch (e: any) {
    telegramProbeState.value = 'fail'
    telegramProbeMessage.value = String(e?.message ?? e)
  }
}

function addTelegramChatId(chatId: string) {
  if (!store.config.channels.telegram.allowChatIds.includes(chatId)) {
    store.config.channels.telegram.allowChatIds.push(chatId)
  }
}

async function checkQqBot() {
  qqProbeState.value = 'checking'
  qqProbeMessage.value = ''
  qqGateway.value = ''
  try {
    const result = await invoke<{
      ok: boolean
      tokenOk: boolean
      gatewayOk: boolean
      gatewayUrl?: string
      message: string
    }>('qqbot_probe', {
      appId: store.config.channels.qqbot.appId,
      appSecret: store.config.channels.qqbot.appSecret,
      sandbox: store.config.channels.qqbot.sandbox,
    })
    qqProbeState.value = result.ok ? 'ok' : 'fail'
    qqProbeMessage.value = result.message
    qqGateway.value = result.gatewayUrl ?? ''
    if (result.ok) {
      store.config.channels.qqbot.enabled = true
      if (!status.value.running) {
        qqProbeMessage.value += '\n我已自动勾选 QQ。现在点击本块里的“启动 QQ”，再去 QQ 里给机器人发测试消息。'
      } else {
        qqProbeMessage.value += '\n渠道服务正在运行。如果刚刚改过 AppID、Secret、沙箱或白名单，请点击“重启 QQ”。'
      }
    }
  } catch (e: any) {
    qqProbeState.value = 'fail'
    qqProbeMessage.value = String(e?.message ?? e)
  }
}

onMounted(refreshStatus)
</script>

<template>
  <section class="settings-section">
    <h3 class="settings-section-title">聊天渠道</h3>
    <p class="settings-section-hint">接入 Telegram 和 QQ 官方机器人。写入禅道会先发起确认，不会直接改数据。</p>

    <div class="settings-actions">
      <span class="channel-status" :class="{ running: status.running }">{{ status.message }}</span>
      <span class="channel-status" :class="{ running: store.config.channels.autoStart }">
        {{ store.config.channels.autoStart ? '自动启动已开启' : '自动启动已关闭' }}
      </span>
      <button class="settings-btn" :disabled="busy || !status.running" @click="stopService">停止全部</button>
    </div>
    <p v-if="serviceMessage" class="settings-msg" :class="status.running ? 'settings-msg-ok' : 'settings-msg-testing'">
      {{ serviceMessage }}
    </p>
    <p class="channel-next">
      检查通过只是验证凭据；真正收发消息需要启动对应渠道。启动后会记住常驻状态，下次打开应用自动启动；点“停止全部”才会关闭自动启动。
    </p>

    <div class="channel-block">
      <label class="settings-toggle">
        <input type="checkbox" v-model="store.config.channels.telegram.enabled" />
        <span>Telegram</span>
      </label>
      <div class="channel-help">
        <div class="help-title">怎么接入</div>
        <ol>
          <li>确认 token 有效：填好 botToken 后点“检查 Telegram”。如果连不上 <code>api.telegram.org</code>，先填本地代理或 Bot API 反代。</li>
          <li>去 Telegram 给机器人发一句：<code>今天有哪些任务？</code></li>
          <li>回来再点“检查 Telegram”，下面会显示 chat id。</li>
          <li>先留空白名单跑通；跑通后点“加入白名单”收紧访问。</li>
          <li>最后点本块里的“启动 Telegram”，之后就能在 Telegram 里对话。</li>
        </ol>
      </div>
      <label class="settings-field">
        <span class="settings-field-label">botToken</span>
        <input class="settings-input" type="password" placeholder="123456:ABC...；已保存时显示 ********" v-model="store.config.channels.telegram.botToken" />
      </label>
      <label class="settings-field">
        <span class="settings-field-label">API 地址</span>
        <input class="settings-input" type="url" placeholder="https://api.telegram.org" v-model="store.config.channels.telegram.apiBaseUrl" />
      </label>
      <label class="settings-field">
        <span class="settings-field-label">代理</span>
        <input class="settings-input" type="text" placeholder="http://127.0.0.1:7890 或 socks5://127.0.0.1:7890，留空读环境代理/直连" v-model="store.config.channels.telegram.proxy" />
      </label>
      <p class="channel-note">OpenClaw/Hermes 能连通常是因为它们吃到了系统/环境代理，或使用了 Bot API 反代。Jarvis 这里的请求由 Rust 后端发出，需要在这里显式填代理，或把 API 地址换成你的反代根地址。</p>
      <div class="settings-actions">
        <button class="settings-btn settings-btn-primary" :disabled="telegramProbeState === 'checking' || !hasTelegramToken()" @click="checkTelegram">
          {{ telegramProbeState === 'checking' ? '检查中...' : '检查 Telegram' }}
        </button>
        <button class="settings-btn" :disabled="busy || !hasTelegramToken()" @click="startTelegramOnly">
          {{ status.running ? '重启 Telegram' : '启动 Telegram' }}
        </button>
        <span v-if="telegramBotName" class="channel-status running">{{ telegramBotName }}</span>
      </div>
      <p v-if="telegramProbeMessage" class="settings-msg" :class="`settings-msg-${telegramProbeState}`">
        {{ telegramProbeMessage }}
      </p>
      <div v-if="telegramRecentChats.length" class="recent-list">
        <div v-for="chat in telegramRecentChats" :key="chat.chatId" class="recent-item">
          <div class="recent-main">
            <strong>{{ chat.title || chat.fromName || chat.chatId }}</strong>
            <span>{{ chat.chatType || 'chat' }} · {{ chat.chatId }}</span>
            <small v-if="chat.text">{{ chat.text }}</small>
          </div>
          <button class="settings-btn" @click="addTelegramChatId(chat.chatId)">加入白名单</button>
        </div>
      </div>
      <label class="settings-field settings-field-top">
        <span class="settings-field-label">白名单</span>
        <textarea
          class="settings-input settings-textarea"
          :value="listText(store.config.channels.telegram.allowChatIds)"
          placeholder="chat id，一行一个；留空允许所有"
          @input="store.config.channels.telegram.allowChatIds = parseList(($event.target as HTMLTextAreaElement).value)"
        />
      </label>
    </div>

    <div class="channel-block">
      <label class="settings-toggle">
        <input type="checkbox" v-model="store.config.channels.qqbot.enabled" />
        <span>QQ 官方机器人</span>
      </label>
      <div class="channel-help">
        <div class="help-title">怎么接入</div>
        <ol>
          <li>到 QQ 开放平台创建官方机器人，拿到 AppID 和 AppSecret。</li>
          <li>把 AppID、AppSecret 填到下面；如果机器人还在沙箱里，打开“使用沙箱环境”。</li>
          <li>点击“检查 QQ”，确认能拿到 access token 和网关地址。</li>
          <li>检查通过后点本块里的“启动 QQ”。私聊可直接发消息，群聊一般需要 @ 机器人。</li>
          <li>测试消息可以发：<code>今天有哪些任务？</code> 或 <code>任务 12345 写 1 小时工时，处理接口联调</code>。</li>
        </ol>
        <p>白名单填 QQ 官方事件里的 <code>user_openid</code> / <code>group_openid</code>。第一次建议先留空，确认能收发后再补。</p>
      </div>
      <label class="settings-field">
        <span class="settings-field-label">AppID</span>
        <input class="settings-input" type="text" v-model="store.config.channels.qqbot.appId" />
      </label>
      <label class="settings-field">
        <span class="settings-field-label">AppSecret</span>
        <input class="settings-input" type="password" placeholder="已保存时显示 ********" v-model="store.config.channels.qqbot.appSecret" />
      </label>
      <label class="settings-toggle">
        <input type="checkbox" v-model="store.config.channels.qqbot.sandbox" />
        <span>使用沙箱环境</span>
      </label>
      <div class="settings-actions">
        <button
          class="settings-btn settings-btn-primary"
          :disabled="qqProbeState === 'checking' || !store.config.channels.qqbot.appId || !hasQqSecret()"
          @click="checkQqBot"
        >
          {{ qqProbeState === 'checking' ? '检查中...' : '检查 QQ' }}
        </button>
        <button class="settings-btn" :disabled="busy || !store.config.channels.qqbot.appId || !hasQqSecret()" @click="startQqOnly">
          {{ status.running ? '重启 QQ' : '启动 QQ' }}
        </button>
        <span v-if="qqGateway" class="channel-status running">Gateway OK</span>
      </div>
      <p v-if="qqProbeMessage" class="settings-msg" :class="`settings-msg-${qqProbeState}`">
        {{ qqProbeMessage }}
      </p>
      <label class="settings-field settings-field-top">
        <span class="settings-field-label">用户白名单</span>
        <textarea
          class="settings-input settings-textarea"
          :value="listText(store.config.channels.qqbot.allowUserIds)"
          placeholder="user_openid，一行一个；留空允许所有"
          @input="store.config.channels.qqbot.allowUserIds = parseList(($event.target as HTMLTextAreaElement).value)"
        />
      </label>
      <label class="settings-field settings-field-top">
        <span class="settings-field-label">群白名单</span>
        <textarea
          class="settings-input settings-textarea"
          :value="listText(store.config.channels.qqbot.allowGroupIds)"
          placeholder="group_openid，一行一个；留空允许所有"
          @input="store.config.channels.qqbot.allowGroupIds = parseList(($event.target as HTMLTextAreaElement).value)"
        />
      </label>
    </div>
  </section>
</template>

<style scoped>
.channel-block {
  display: flex;
  flex-direction: column;
  gap: 5px;
  padding: 8px;
  background: var(--surface);
  border: var(--divider);
  border-radius: 6px;
}
.channel-status {
  align-self: center;
  font-size: 11px;
  color: var(--text-dim);
}
.channel-status.running {
  color: var(--green-text);
}
.channel-next {
  margin: 0;
  padding: 5px 8px;
  font-size: 10.5px;
  line-height: 1.45;
  color: var(--text-ghost);
  background: var(--blue-bg);
  border: 1px solid var(--blue-border);
  border-radius: 5px;
}
.channel-help {
  padding: 6px 8px;
  font-size: 10.5px;
  line-height: 1.55;
  color: var(--text-ghost);
  background: var(--surface-2);
  border: var(--divider-soft);
  border-radius: 5px;
}
.help-title {
  margin-bottom: 2px;
  color: var(--accent-text);
  font-weight: 600;
}
.channel-help ol {
  margin: 0;
  padding-left: 18px;
}
.channel-help p {
  margin: 4px 0 0;
  color: var(--text-dim);
}
.channel-note {
  margin: -1px 0 2px;
  font-size: 10px;
  line-height: 1.45;
  color: var(--text-dim);
}
.channel-help code {
  font-family: ui-monospace, monospace;
  color: var(--text-ghost);
}
.settings-field-top {
  align-items: flex-start;
}
.settings-textarea {
  min-height: 54px;
  resize: vertical;
  line-height: 1.35;
}
.recent-list {
  display: flex;
  flex-direction: column;
  gap: 5px;
}
.recent-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 8px;
  background: var(--surface);
  border: var(--divider);
  border-radius: 5px;
}
.recent-main {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
  font-size: 11px;
}
.recent-main strong {
  color: var(--text-ghost);
}
.recent-main span,
.recent-main small {
  color: var(--text-dim);
  word-break: break-all;
}
</style>
