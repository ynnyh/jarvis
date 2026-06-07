<script setup lang="ts">
import { ref, computed, onUnmounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { useConfigStore } from '../../stores/config'

// ============================================================================
// 后端契约（src-tauri/src/voice.rs，命令已实现）
// ============================================================================
//   voice_assets_status() -> { ready, voiceDir, hasBinary, hasModel }
//   voice_download_assets() -> { ready }   // 边下边 emit voice-download-progress
//
// 事件 voice-download-progress: { phase: 'binary'|'model', downloaded, total, percent }
//
// UX（照搬 deployEnabled 的「默认关闭 + 开启才生效」范式，叠加首启下载确认）：
//   开 → 查 voice_assets_status：
//     ready=true            → 直接开。
//     ready=false           → 弹「确认下载约 600MB」模态：
//        确认 → voice_download_assets，进度条；成功保持开+提示就绪，失败回退到关+错误。
//        取消 → 回退到关，不下载。
//   关 → 直接关（保留已下资产，不删）。

const store = useConfigStore()

type Phase = 'binary' | 'model'
interface DownloadProgress {
  phase: Phase
  downloaded: number
  total: number
  percent: number
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

let unlistenProgress: UnlistenFn | null = null

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
    const status = await invoke<{ ready: boolean }>('voice_assets_status')
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
    // 失败：保持关闭（store 没被置 true），进入显式失败态 → 模板渲染「重试下载」按钮。
    // 半截文件后端写的是 .part 临时文件、成功才落盘，已下好的整文件重试会跳过。
    store.config.voiceInputEnabled = false
    downloadFailed.value = true
    messageKind.value = 'fail'
    message.value = `下载失败：${errText(e)}`
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
        {{ fmtMB(progress.downloaded) }}<template v-if="progress.total > 0"> / {{ fmtMB(progress.total) }}</template>
      </div>
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

    <!-- 确认下载模态 -->
    <div v-if="showConfirm" class="voice-modal-mask" @click.self="cancelDownload">
      <div class="voice-modal">
        <h4 class="voice-modal-title">启用语音输入</h4>
        <p class="voice-modal-body">
          启用语音输入需下载语音引擎与模型（约 600MB，含 large-v3-turbo 中英文模型，仅首次）。是否下载？
        </p>
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
