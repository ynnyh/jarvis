<script setup lang="ts">
import { ref, computed, onUnmounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { useConfigStore } from '../../stores/config'

// ============================================================================
// 后端契约（src-tauri/src/voice.rs，命令已实现）
// ============================================================================
//   voice_assets_status() -> {
//     ready, voiceDir, hasBinary, hasModel,
//     proxy,                 // 下载是否走代理（null=直连）
//     binaryName, modelName, // 两个文件名
//     binaryUrl, modelUrl,   // 原始直链（手动兜底用）
//     modelMirrorUrl,        // 模型的 hf-mirror 镜像链
//   }
//   voice_download_assets() -> { ready }   // 边下边 emit voice-download-progress
//   voice_open_dir() -> ()                 // 系统文件管理器打开 voiceDir（手动放文件用）
//
// 事件 voice-download-progress: { phase: 'binary'|'model', downloaded, total, percent, bytesPerSec }
//
// UX（照搬 deployEnabled 的「默认关闭 + 开启才生效」范式，叠加首启下载确认）：
//   开 → 查 voice_assets_status：
//     ready=true            → 直接开。
//     ready=false           → 弹「确认下载约 600MB」模态：
//        确认 → voice_download_assets，进度条；成功保持开+提示就绪，失败回退到关+错误。
//        取消 → 回退到关，不下载。
//   关 → 直接关（保留已下资产，不删）。
//
// 国内下载痛点：引擎在 GitHub、模型在 HF（hf-mirror 只 302 跳美国 Xet 存储，574MB 必断）。
// 三道保险：① 下载走用户代理（config.channels.telegram.proxy）；② 断点续传+自动重试；
// ③ 手动兜底区（列直链 + 打开目录 + 重新检测）。

const store = useConfigStore()

type Phase = 'binary' | 'model'
interface DownloadProgress {
  phase: Phase
  downloaded: number
  total: number
  percent: number
  bytesPerSec?: number
}

interface AssetsStatus {
  ready: boolean
  voiceDir: string
  hasBinary: boolean
  hasModel: boolean
  proxy: string | null
  binaryName: string
  modelName: string
  binaryUrl: string
  modelUrl: string
  modelMirrorUrl: string
}

// 是否正处于「确认下载」模态。
const showConfirm = ref(false)
// 是否正在下载。
const downloading = ref(false)
// 下载失败、等待用户「重试 / 取消」（与 message 配合显式呈现失败态 + 重试按钮）。
const downloadFailed = ref(false)
// 最近一次进度（null = 还没开始/已结束）。
const progress = ref<DownloadProgress | null>(null)
// 结果消息（成功/失败提示）。
const message = ref('')
const messageKind = ref<'ok' | 'fail' | 'info'>('info')
// 资产状态快照（含代理 / 目标目录 / 手动直链），用于代理提示 + 手动兜底区。
const assets = ref<AssetsStatus | null>(null)
// 手动下载区是否展开（下载失败时默认展开，平时可手动点开）。
const showManual = ref(false)
// 「重新检测」按钮忙碌态。
const rechecking = ref(false)

let unlistenProgress: UnlistenFn | null = null

// 组件挂载即拉一次资产状态：好在下载前就显示代理提示 + 让手动兜底区可用。
void refreshAssets()

onUnmounted(() => {
  unlistenProgress?.()
})

// 开关用本地代理：拦截「打开」动作先走资产门禁，门禁通过/已就绪才真正写进 store；
// 取消或失败时不写 store（保持关闭）。直接 v-model 到 store 会即时翻转并触发自动保存，
// 没法在「用户取消下载」时干净地回退。
const enabledModel = computed<boolean>({
  get: () => store.config.voiceInputEnabled,
  set: (next) => {
    if (next) {
      void requestEnable()
    } else {
      // 关闭：直接落 store（保留资产），并注销全局热键（不在关闭态占用）。
      store.config.voiceInputEnabled = false
      void syncHotkey()
      resetTransient()
    }
  },
})

function resetTransient() {
  showConfirm.value = false
  downloading.value = false
  downloadFailed.value = false
  progress.value = null
}

/** 通知后端按当前开关状态注册/注销全局热键。失败只记日志，不打扰用户。
 *  后端 voice_hotkey_sync 以磁盘 config.json 的 voiceInputEnabled 为准，而 store 的写盘是
 *  250ms 防抖的；这里先 await store.save() 立即落盘，避免后端读到翻转前的旧值（注册/注销反了）。 */
async function syncHotkey() {
  try {
    await store.save()
    await invoke('voice_hotkey_sync')
  } catch (e) {
    console.error('[voice] 同步全局热键失败:', e)
  }
}

/** 用户想开启：查资产，决定「直接开」还是「弹确认下载」。 */
async function requestEnable() {
  message.value = ''
  try {
    const status = await invoke<AssetsStatus>('voice_assets_status')
    assets.value = status
    if (status.ready) {
      store.config.voiceInputEnabled = true
      await syncHotkey()
      messageKind.value = 'ok'
      message.value = '语音输入已启用，可按 Ctrl/Cmd+Shift+空格 说话。'
    } else {
      // 资产没就绪 → 弹确认（此时 store 仍是 false，开关视觉保持关闭直到下载成功）。
      showConfirm.value = true
    }
  } catch (e) {
    messageKind.value = 'fail'
    message.value = `检查语音资产失败：${errText(e)}`
  }
}

/** 拉取资产状态快照（代理 / 目录 / 手动直链 / 就绪态）。挂载时 + 重新检测时调。
 *  失败只记日志：拿不到状态不阻断主流程，手动区会缺直链但仍可展开提示。 */
async function refreshAssets() {
  try {
    assets.value = await invoke<AssetsStatus>('voice_assets_status')
  } catch (e) {
    console.error('[voice] 查询语音资产状态失败:', e)
  }
}

/** 「重新检测」：手动放好文件后调用——重查资产，就绪则开开关 + 同步热键并提示，否则提示仍缺。 */
async function recheckAssets() {
  rechecking.value = true
  message.value = ''
  try {
    const status = await invoke<AssetsStatus>('voice_assets_status')
    assets.value = status
    if (status.ready) {
      store.config.voiceInputEnabled = true
      await syncHotkey()
      downloadFailed.value = false
      showManual.value = false
      messageKind.value = 'ok'
      message.value = '检测到语音引擎与模型已就绪，语音输入已启用。'
    } else {
      const miss: string[] = []
      if (!status.hasBinary) miss.push('语音引擎')
      if (!status.hasModel) miss.push('识别模型')
      messageKind.value = 'fail'
      message.value = `仍缺少：${miss.join('、')}。请把对应文件放进目录后再检测。`
    }
  } catch (e) {
    messageKind.value = 'fail'
    message.value = `重新检测失败：${errText(e)}`
  } finally {
    rechecking.value = false
  }
}

/** 「打开目录」：用系统文件管理器打开语音资产目录，方便用户手动放文件。 */
async function openVoiceDir() {
  try {
    await invoke('voice_open_dir')
  } catch (e) {
    messageKind.value = 'fail'
    message.value = `打开目录失败：${errText(e)}`
  }
}

/** 确认下载：用户在确认模态点「是」，关模态后启动下载。 */
async function confirmDownload() {
  showConfirm.value = false
  await runDownload()
}

/** 重试下载：失败态点「重试下载」，重新调 voice_download_assets。
 *  后端文件级幂等——已下好的部分（如二进制已就绪）会跳过，只补没下完的，故重试会接着下。 */
async function retryDownload() {
  await runDownload()
}

/** 实际下载流程：监听进度 → 调下载命令 → 成功开+提示 / 失败置失败态+错误（带重试按钮）。
 *  确认下载与重试共用这段。 */
async function runDownload() {
  downloading.value = true
  downloadFailed.value = false
  message.value = ''
  progress.value = { phase: 'binary', downloaded: 0, total: 0, percent: 0 }

  // 先挂监听再发起下载，避免漏掉早期进度事件。
  unlistenProgress?.()
  unlistenProgress = await listen<DownloadProgress>('voice-download-progress', (e) => {
    progress.value = e.payload
  })

  try {
    await invoke('voice_download_assets')
    store.config.voiceInputEnabled = true
    await syncHotkey()
    messageKind.value = 'ok'
    message.value = '语音引擎与模型已就绪，语音输入已启用，可按 Ctrl/Cmd+Shift+空格 说话。'
  } catch (e) {
    // 失败：保持关闭（store 没被置 true），进入显式失败态 → 模板渲染「重试下载」按钮，
    // 并自动展开手动兜底区（自动下不动就引导用户手动下）。
    store.config.voiceInputEnabled = false
    downloadFailed.value = true
    showManual.value = true
    messageKind.value = 'fail'
    message.value = `下载失败：${errText(e)}`
    // 刷新资产状态：部分已下好的（如二进制就绪）能在手动区如实反映。
    void refreshAssets()
  } finally {
    downloading.value = false
    progress.value = null
    unlistenProgress?.()
    unlistenProgress = null
  }
}

/** 取消下载：回退到关闭，不下载（确认模态点「否」时用）。 */
function cancelDownload() {
  showConfirm.value = false
  store.config.voiceInputEnabled = false
  messageKind.value = 'info'
  message.value = '已取消，语音输入保持关闭。'
}

/** 失败态点「取消」：放弃重试，回到关闭并清失败态。 */
function dismissFailure() {
  downloadFailed.value = false
  store.config.voiceInputEnabled = false
  messageKind.value = 'info'
  message.value = '已取消，语音输入保持关闭。'
}

function phaseLabel(p: Phase): string {
  return p === 'binary' ? '语音引擎' : '识别模型'
}

function fmtMB(bytes: number): string {
  return `${(bytes / 1_000_000).toFixed(1)}MB`
}

/** 速度文案：B/s → MB/s（保留一位小数）。<=0 或缺省时返回空串（不显示速度段）。 */
function fmtSpeed(bytesPerSec?: number): string {
  if (!bytesPerSec || bytesPerSec <= 0) return ''
  return `${(bytesPerSec / 1_000_000).toFixed(1)} MB/s`
}

/** 代理提示：有代理 → 「下载将通过代理 127.0.0.1:7897」；无 → 「直连下载（未配置代理）」。
 *  剥掉 http(s):// 前缀只显示 host:port，简洁。 */
const proxyHint = computed<string>(() => {
  const proxy = assets.value?.proxy
  if (proxy) {
    const short = proxy.replace(/^https?:\/\//, '').replace(/^socks5:\/\//, '')
    return `下载将通过代理 ${short}`
  }
  return '直连下载（未配置代理，国内可能失败；可在「机器人」设置里填代理后重试）'
})

function errText(e: unknown): string {
  if (e instanceof Error) return e.message
  return String(e)
}
</script>

<template>
  <section class="settings-section">
    <h3 class="settings-section-title">语音输入</h3>
    <label class="settings-toggle">
      <input type="checkbox" v-model="enabledModel" :disabled="downloading" />
      <span>启用语音输入</span>
    </label>
    <p class="settings-section-hint">
      本地语音转写，默认关闭。启用后可按热键 / 点小人说话，转写文字直接注入当前聚焦的输入框（本地处理，不上云）。
      首次启用需下载语音引擎与模型（约 600MB）。
    </p>

    <!-- 下载进度 -->
    <div v-if="downloading && progress" class="voice-progress">
      <div class="voice-progress-head">
        <span>正在下载{{ phaseLabel(progress.phase) }}…</span>
        <span v-if="progress.total > 0">{{ progress.percent }}%</span>
      </div>
      <div class="voice-progress-bar">
        <div
          class="voice-progress-fill"
          :style="{ width: progress.total > 0 ? progress.percent + '%' : '100%' }"
          :class="{ 'voice-progress-indeterminate': progress.total === 0 }"
        ></div>
      </div>
      <div class="voice-progress-sub">
        {{ fmtMB(progress.downloaded) }}<template v-if="progress.total > 0"> / {{ fmtMB(progress.total) }}</template
        ><template v-if="fmtSpeed(progress.bytesPerSec)"> · {{ fmtSpeed(progress.bytesPerSec) }}</template>
      </div>
      <p class="voice-proxy-hint">{{ proxyHint }}</p>
    </div>

    <!-- 结果消息 -->
    <p
      v-if="message && !downloading"
      class="settings-msg"
      :class="`settings-msg-${messageKind === 'info' ? 'testing' : messageKind}`"
    >
      {{ message }}
    </p>

    <!-- 下载失败：显式失败态 + 重试 / 取消（重试接着下没下完的部分） -->
    <div v-if="downloadFailed && !downloading" class="voice-retry">
      <button class="settings-btn voice-retry-primary" @click="retryDownload">重试下载</button>
      <button class="settings-btn" @click="dismissFailure">取消</button>
    </div>

    <!-- 手动下载兜底区：下载失败默认展开，平时可点开。自动下不动就手动下这两个文件丢进目录。 -->
    <div v-if="!downloading" class="voice-manual">
      <button class="voice-manual-toggle" type="button" @click="showManual = !showManual">
        {{ showManual ? '▾' : '▸' }} 手动下载（自动下载失败时用）
      </button>
      <div v-if="showManual" class="voice-manual-body">
        <p class="voice-manual-intro">
          自动下不动时，按下面两个地址手动下载（建议用浏览器或下载工具，支持断点续传），下好后丢进目标目录即可：
        </p>

        <ol class="voice-manual-list">
          <li>
            <div class="voice-manual-label">① 语音引擎（zip，需解压）</div>
            <a v-if="assets?.binaryUrl" class="voice-manual-link" :href="assets.binaryUrl" target="_blank" rel="noreferrer">{{ assets.binaryUrl }}</a>
            <div class="voice-manual-note">
              下载后<b>解压</b>，把里面的 <code>{{ assets?.binaryName || 'whisper-cli.exe' }}</code> 和所有 <code>.dll</code> 平铺放进目标目录。
            </div>
          </li>
          <li>
            <div class="voice-manual-label">② 识别模型（约 574MB，直接放）</div>
            <a v-if="assets?.modelUrl" class="voice-manual-link" :href="assets.modelUrl" target="_blank" rel="noreferrer">{{ assets.modelUrl }}</a>
            <a v-if="assets?.modelMirrorUrl" class="voice-manual-link voice-manual-link-alt" :href="assets.modelMirrorUrl" target="_blank" rel="noreferrer">国内镜像：{{ assets.modelMirrorUrl }}</a>
            <div class="voice-manual-note">
              下载到的 <code>{{ assets?.modelName || 'ggml-large-v3-turbo-q5_0.bin' }}</code> <b>直接</b>放进目标目录（不要解压、不要改名）。
            </div>
          </li>
        </ol>

        <div class="voice-manual-dir">
          <span class="voice-manual-dir-label">目标目录：</span>
          <code v-if="assets?.voiceDir" class="voice-manual-dir-path">{{ assets.voiceDir }}</code>
        </div>

        <div class="voice-manual-actions">
          <button class="settings-btn" @click="openVoiceDir">打开目录</button>
          <button class="settings-btn voice-manual-primary" :disabled="rechecking" @click="recheckAssets">
            {{ rechecking ? '检测中…' : '重新检测' }}
          </button>
        </div>
      </div>
    </div>

    <!-- 确认下载模态 -->
    <div v-if="showConfirm" class="voice-modal-mask" @click.self="cancelDownload">
      <div class="voice-modal">
        <h4 class="voice-modal-title">启用语音输入</h4>
        <p class="voice-modal-body">
          启用语音输入需下载语音引擎与模型（约 600MB，含 large-v3-turbo 中英文模型，仅首次）。是否下载？
        </p>
        <p class="voice-modal-proxy">{{ proxyHint }}</p>
        <div class="voice-modal-actions">
          <button class="settings-btn" @click="cancelDownload">否</button>
          <button class="settings-btn voice-modal-primary" @click="confirmDownload">是，下载</button>
        </div>
      </div>
    </div>
  </section>
</template>

<style scoped>
.voice-progress {
  display: flex;
  flex-direction: column;
  gap: 4px;
  margin-top: 6px;
}
.voice-progress-head {
  display: flex;
  justify-content: space-between;
  font-size: 11px;
  color: var(--text-dim);
}
.voice-progress-bar {
  height: 6px;
  border-radius: 3px;
  background: var(--surface-2);
  overflow: hidden;
}
.voice-progress-fill {
  height: 100%;
  background: var(--accent);
  border-radius: 3px;
  transition: width 0.2s ease;
}
.voice-progress-indeterminate {
  width: 35% !important;
  animation: voice-slide 1.1s ease-in-out infinite;
}
@keyframes voice-slide {
  0% { margin-left: -35%; }
  100% { margin-left: 100%; }
}
.voice-progress-sub {
  font-size: 10.5px;
  color: var(--text-faint);
}
.voice-proxy-hint {
  margin: 2px 0 0;
  font-size: 10.5px;
  color: var(--text-faint);
}

/* 手动下载兜底区 */
.voice-manual {
  margin-top: 10px;
}
.voice-manual-toggle {
  padding: 0;
  font-size: 11.5px;
  color: var(--text-dim);
  background: none;
  border: none;
  cursor: pointer;
}
.voice-manual-toggle:hover {
  color: var(--text);
}
.voice-manual-body {
  display: flex;
  flex-direction: column;
  gap: 10px;
  margin-top: 8px;
  padding: 12px;
  background: var(--surface-2);
  border: var(--divider);
  border-radius: 8px;
}
.voice-manual-intro {
  margin: 0;
  font-size: 11.5px;
  line-height: 1.6;
  color: var(--text-dim);
}
.voice-manual-list {
  display: flex;
  flex-direction: column;
  gap: 10px;
  margin: 0;
  padding: 0;
  list-style: none;
}
.voice-manual-label {
  font-size: 11.5px;
  font-weight: 600;
  color: var(--text);
}
.voice-manual-link {
  display: block;
  margin-top: 3px;
  font-size: 11px;
  color: var(--accent);
  word-break: break-all;
}
.voice-manual-link-alt {
  color: var(--text-dim);
}
.voice-manual-note {
  margin-top: 3px;
  font-size: 11px;
  line-height: 1.6;
  color: var(--text-faint);
}
.voice-manual-note code,
.voice-manual-dir-path {
  padding: 1px 4px;
  font-family: var(--mono, monospace);
  font-size: 10.5px;
  background: var(--surface);
  border-radius: 4px;
}
.voice-manual-dir {
  font-size: 11px;
  color: var(--text-dim);
}
.voice-manual-dir-label {
  margin-right: 4px;
}
.voice-manual-dir-path {
  word-break: break-all;
}
.voice-manual-actions {
  display: flex;
  gap: 8px;
}
.voice-manual-primary {
  color: var(--on-accent);
  background: var(--accent);
  border-color: var(--accent);
}
.voice-manual-primary:hover:not(:disabled) {
  background: color-mix(in srgb, var(--accent) 88%, #000);
  border-color: color-mix(in srgb, var(--accent) 88%, #000);
}

/* 下载失败重试行 */
.voice-retry {
  display: flex;
  gap: 8px;
  margin-top: 8px;
}
.voice-retry-primary {
  color: var(--on-accent);
  background: var(--accent);
  border-color: var(--accent);
}
.voice-retry-primary:hover:not(:disabled) {
  background: color-mix(in srgb, var(--accent) 88%, #000);
  border-color: color-mix(in srgb, var(--accent) 88%, #000);
}

/* 确认模态 */
.voice-modal-mask {
  position: fixed;
  inset: 0;
  z-index: 50;
  display: flex;
  align-items: center;
  justify-content: center;
  background: color-mix(in srgb, #000 45%, transparent);
}
.voice-modal {
  width: min(320px, 88vw);
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 16px;
  background: var(--popup-bg, var(--surface));
  border: var(--divider);
  border-radius: 10px;
  box-shadow: 0 8px 28px rgba(0, 0, 0, 0.35);
}
.voice-modal-title {
  margin: 0;
  font-size: 13px;
  font-weight: 600;
  color: var(--text);
}
.voice-modal-body {
  margin: 0;
  font-size: 12px;
  line-height: 1.6;
  color: var(--text-dim);
}
.voice-modal-proxy {
  margin: 0;
  font-size: 11px;
  line-height: 1.5;
  color: var(--text-faint);
}
.voice-modal-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}
.voice-modal-primary {
  color: var(--on-accent);
  background: var(--accent);
  border-color: var(--accent);
}
.voice-modal-primary:hover:not(:disabled) {
  background: color-mix(in srgb, var(--accent) 88%, #000);
  border-color: color-mix(in srgb, var(--accent) 88%, #000);
}
</style>
