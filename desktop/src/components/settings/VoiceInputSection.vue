<script setup lang="ts">
import { ref, computed, onUnmounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { useConfigStore } from '../../stores/config'

// ============================================================================
// 后端契约（src-tauri/src/voice.rs，命令已实现）
// ============================================================================
//   voice_assets_status() -> {
//     ready, voiceDir, hasBinary, hasModel, hasTokens,
//     proxy,                              // 下载是否走代理（null=直连）
//     binaryName, modelName, tokensName,  // 三个资产的落地文件名
//     binaryUrl, modelUrl, tokensUrl,     // 原始直链（手动兜底用）
//     modelMirrorUrl, tokensMirrorUrl,    // 模型/词表的 hf-mirror 镜像链
//   }
//   voice_download_assets() -> { ready }   // 边下边 emit voice-download-progress
//   voice_open_dir() -> ()                 // 系统文件管理器打开 voiceDir（手动放文件用）
//
// 事件 voice-download-progress: { phase: 'binary'|'model'|'tokens', downloaded, total, percent, bytesPerSec }
//
// UX（照搬 deployEnabled 的「默认关闭 + 开启才生效」范式，叠加首启下载确认）：
//   开 → 查 voice_assets_status：
//     ready=true            → 直接开。
//     ready=false           → 弹「确认下载约 250MB」模态：
//        确认 → voice_download_assets，进度条；成功保持开+提示就绪，失败回退到关+错误。
//        取消 → 回退到关，不下载。
//   关 → 直接关（保留已下资产，不删）。
//
// 国内下载痛点：引擎在 GitHub、模型/词表在 HF（hf-mirror 大文件 302 跳美国 Xet 存储，易断）。
// 三道保险：① 下载走用户代理（config.channels.telegram.proxy）；② 断点续传+自动重试；
// ③ 手动兜底区（列直链 + 打开目录 + 重新检测）。

const store = useConfigStore()

const SECRET_PLACEHOLDER = '********'

// 当前引擎：本地 SenseVoice / 云端火山。直接读写 store.config.voiceEngine。
const engine = computed<'local' | 'cloud'>({
  get: () => (store.config.voiceEngine === 'cloud' ? 'cloud' : 'local'),
  set: (next) => { void switchEngine(next) },
})

// 切换引擎：先落 store（触发自动保存），关掉旧引擎残留态，再按当前开关重注册热键。
// 不在这里强制校验就绪——校验发生在用户「启用」时（云端查凭证 / 本地查资产）。
async function switchEngine(next: 'local' | 'cloud') {
  if (store.config.voiceEngine === next) return
  store.config.voiceEngine = next
  resetTransient()
  message.value = ''
  cloudMsg.value = ''
  // 已启用状态下换引擎：重注册热键（云端缺凭证 / 本地缺资产会自动注销，不报错打扰）。
  if (store.config.voiceInputEnabled) await syncHotkey()
}

type Phase = 'binary' | 'model' | 'tokens'
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
  hasTokens: boolean
  proxy: string | null
  binaryName: string
  modelName: string
  tokensName: string
  binaryUrl: string
  modelUrl: string
  modelMirrorUrl: string
  tokensUrl: string
  tokensMirrorUrl: string
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

// ====== 云端（火山引擎）凭证 ======
// App ID 明文双向绑定 store；Access Token 走 keychain，后端保存后回填占位符 ********。
const volcAppId = computed<string>({
  get: () => store.config.voiceCloud.volcAppId,
  set: (v) => { store.config.voiceCloud.volcAppId = v },
})
const volcAccessToken = computed<string>({
  get: () => store.config.voiceCloud.volcAccessToken,
  set: (v) => { store.config.voiceCloud.volcAccessToken = v },
})
// 云端启用/校验的结果消息（与本地下载消息分开，单独显示在云端区）。
const cloudMsg = ref('')
const cloudMsgKind = ref<'ok' | 'fail' | 'info'>('info')
// 云端「启用/校验凭证」按钮忙碌态。
const cloudChecking = ref(false)

// ====== 自定义快捷键录制 ======
// 是否处于录制态（点输入框进入，监听 keydown 捕获组合键）。
const recording = ref(false)
// 录制中实时显示的组合键（accelerator 字符串），未捕获到合法组合时为空。
const draftHotkey = ref('')
// 改键提交忙碌态（写盘 + 重注册期间禁用按钮）。
const savingHotkey = ref(false)
// 改键结果消息（与下载消息区分开，单独一行显示在录制框下）。
const hotkeyMsg = ref('')
const hotkeyMsgKind = ref<'ok' | 'fail' | 'info'>('info')

let unlistenProgress: UnlistenFn | null = null

// 当前生效热键（展示用）：取 store 里的 voiceHotkey，空则给默认值兜底。
const currentHotkey = computed<string>(
  () => store.config.voiceHotkey || 'CommandOrControl+Shift+Space',
)

// 把 accelerator 字符串拆段，给 <kbd> 逐键渲染（如 "CommandOrControl+Shift+Space" → 3 段）。
function hotkeyParts(accel: string): string[] {
  return accel.split('+').filter(Boolean)
}

// 单段 accelerator 的人类可读名：CommandOrControl 按平台显示 ⌘/Ctrl，其余原样。
function prettyKey(part: string): string {
  if (part === 'CommandOrControl') return isMac.value ? 'Cmd' : 'Ctrl'
  return part
}

// 平台判断（影响 Ctrl/Cmd 文案与映射）：navigator.platform 含 'mac' 视为 macOS。
const isMac = computed<boolean>(() =>
  /mac/i.test(typeof navigator !== 'undefined' ? navigator.platform : ''),
)

// 浏览器 KeyboardEvent.key → Tauri accelerator「主键」段的映射。
// 命中映射 → 返回对应 accelerator 段；返回 null → 这个键不支持作主键（提示重录）。
function mainKeyToken(e: KeyboardEvent): string | null {
  const key = e.key
  // 单个可见字符：字母统一大写（Tauri 认大写字母）；数字/符号里挑常见的。
  if (key.length === 1) {
    const code = key.toUpperCase()
    if (/^[A-Z0-9]$/.test(code)) return code
    // 空格的 key 是 ' '，单独处理（下面 ' ' 命中）。
    if (key === ' ') return 'Space'
    return null // 其它符号（,.;/ 等）随键盘布局多变，先不收，提示重录更稳。
  }
  // 具名键：映射到 Tauri 认的 token。
  const named: Record<string, string> = {
    ' ': 'Space',
    Spacebar: 'Space',
    ArrowUp: 'Up',
    ArrowDown: 'Down',
    ArrowLeft: 'Left',
    ArrowRight: 'Right',
    Enter: 'Enter',
    Tab: 'Tab',
    Backspace: 'Backspace',
    Delete: 'Delete',
    Insert: 'Insert',
    Home: 'Home',
    End: 'End',
    PageUp: 'PageUp',
    PageDown: 'PageDown',
    Escape: 'Escape',
  }
  if (named[key]) return named[key]
  // 功能键 F1~F24。
  if (/^F([1-9]|1[0-9]|2[0-4])$/.test(key)) return key
  return null
}

/** KeyboardEvent → Tauri accelerator 字符串（如 "CommandOrControl+Shift+Space"）。
 *  - 修饰键：Ctrl→CommandOrControl、Meta(⌘/Win)→CommandOrControl、Shift→Shift、Alt→Alt。
 *    Ctrl 与 Meta 都归一到 CommandOrControl，跨平台一致（macOS=⌘、Win/Linux=Ctrl）。
 *  - 主键：见 mainKeyToken；拿不到合法主键 → 返回 null（调用方提示重录）。
 *  顺序固定 CommandOrControl→Alt→Shift→主键，和 Tauri 习惯一致、避免抖动。 */
function eventToAccelerator(e: KeyboardEvent): string | null {
  const main = mainKeyToken(e)
  if (!main) return null
  const mods: string[] = []
  // Ctrl 或 Meta 任一按下都算 CommandOrControl（只加一次）。
  if (e.ctrlKey || e.metaKey) mods.push('CommandOrControl')
  if (e.altKey) mods.push('Alt')
  if (e.shiftKey) mods.push('Shift')
  // 校验：至少一个修饰键（避免纯单键误触）。
  if (mods.length === 0) return null
  return [...mods, main].join('+')
}

/** 进入录制态：清草稿/消息，挂全局 keydown 监听捕获下一组合键。 */
function startRecording() {
  if (savingHotkey.value) return
  recording.value = true
  draftHotkey.value = ''
  hotkeyMsg.value = ''
  window.addEventListener('keydown', onRecordKeydown, true)
}

/** 退出录制态：摘监听、清草稿。 */
function stopRecording() {
  recording.value = false
  draftHotkey.value = ''
  window.removeEventListener('keydown', onRecordKeydown, true)
}

/** 录制中的 keydown 处理：阻断默认/冒泡，纯修饰键不收（等主键），合法组合写进草稿。 */
function onRecordKeydown(e: KeyboardEvent) {
  e.preventDefault()
  e.stopPropagation()
  // Esc：取消录制，保留旧值。
  if (e.key === 'Escape') {
    stopRecording()
    hotkeyMsgKind.value = 'info'
    hotkeyMsg.value = '已取消录制，保留原快捷键。'
    return
  }
  // 只按了修饰键、还没按主键：先不收，提示继续按主键。
  if (['Control', 'Shift', 'Alt', 'Meta'].includes(e.key)) {
    draftHotkey.value = ''
    return
  }
  const accel = eventToAccelerator(e)
  if (!accel) {
    // 没有修饰键，或主键不支持 → 提示重录（不退出录制态，让用户接着试）。
    draftHotkey.value = ''
    hotkeyMsgKind.value = 'fail'
    hotkeyMsg.value = '需至少一个修饰键（Ctrl/Cmd/Shift/Alt）+ 一个主键，且主键需为字母/数字/空格/方向键/功能键等常见键，请重录。'
    return
  }
  // 捕获到合法组合：写草稿、退出监听，落键交给 applyHotkey。
  draftHotkey.value = accel
  hotkeyMsg.value = ''
  window.removeEventListener('keydown', onRecordKeydown, true)
  recording.value = false
  void applyHotkey(accel)
}

/** 落键：写 config.voiceHotkey → 落盘 → 通知后端重注册。
 *  成功提示新键位；失败（冲突/无效）回显错误并把 config 恢复旧值（后端会用旧值重注册）。 */
async function applyHotkey(accel: string) {
  const prev = store.config.voiceHotkey
  if (accel === prev) {
    hotkeyMsgKind.value = 'info'
    hotkeyMsg.value = '快捷键未变化。'
    return
  }
  savingHotkey.value = true
  hotkeyMsg.value = ''
  try {
    store.config.voiceHotkey = accel
    await store.save()
    // 后端按新 config 重注册（先撤旧键再注册新键）。注册失败会抛错。
    const res = await invoke<{ registered: boolean; hotkey: string }>('voice_hotkey_sync')
    hotkeyMsgKind.value = 'ok'
    hotkeyMsg.value = store.config.voiceInputEnabled
      ? `快捷键已更新为 ${res.hotkey || accel}。`
      : `快捷键已保存为 ${res.hotkey || accel}（启用语音输入后生效）。`
  } catch (e) {
    // 注册失败（被占用/无效）：回退旧值并让后端按旧值重注册，避免悬空。
    store.config.voiceHotkey = prev
    try {
      await store.save()
      await invoke('voice_hotkey_sync')
    } catch (e2) {
      console.error('[voice] 回退旧快捷键失败:', e2)
    }
    hotkeyMsgKind.value = 'fail'
    hotkeyMsg.value = `设置快捷键失败（可能被其它程序占用或无效）：${errText(e)}，已保留原快捷键。`
  } finally {
    savingHotkey.value = false
    draftHotkey.value = ''
  }
}

// 组件挂载即拉一次资产状态：好在下载前就显示代理提示 + 让手动兜底区可用。
void refreshAssets()

onUnmounted(() => {
  unlistenProgress?.()
  // 若卸载时仍在录制，摘掉全局 keydown 监听，避免泄漏。
  window.removeEventListener('keydown', onRecordKeydown, true)
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

/** 用户想开启：按引擎分流。
 *  云端 → 校验火山凭证（voice_cloud_status），齐了直接开、缺了提示去填，不走下载。
 *  本地 → 查 sherpa 资产，ready 直接开、否则弹「确认下载」模态。 */
async function requestEnable() {
  message.value = ''
  cloudMsg.value = ''
  if (engine.value === 'cloud') {
    await enableCloud()
    return
  }
  try {
    const status = await invoke<AssetsStatus>('voice_assets_status')
    assets.value = status
    if (status.ready) {
      store.config.voiceInputEnabled = true
      await syncHotkey()
      messageKind.value = 'ok'
      message.value = `语音输入已启用，可按 ${currentHotkey.value} 说话。`
    } else {
      // 资产没就绪 → 弹确认（此时 store 仍是 false，开关视觉保持关闭直到下载成功）。
      showConfirm.value = true
    }
  } catch (e) {
    messageKind.value = 'fail'
    message.value = `检查语音资产失败：${errText(e)}`
  }
}

/** 云端启用：先把当前 App ID / Token 落盘（token 进 keychain），再查 voice_cloud_status 校验。
 *  齐了 → 开开关 + 同步热键；缺了 → 保持关闭并提示去控制台开通。云端不需要下载模型。 */
async function enableCloud() {
  cloudChecking.value = true
  cloudMsg.value = ''
  try {
    // 先落盘：把刚填的 token 抽进 keychain（后端 strip_secrets_for_save），config 里留占位符。
    await store.save()
    const res = await invoke<{ ready: boolean; message?: string }>('voice_cloud_status')
    if (res.ready) {
      store.config.voiceInputEnabled = true
      await syncHotkey()
      cloudMsgKind.value = 'ok'
      cloudMsg.value = `云端语音已启用，可按 ${currentHotkey.value} 说话。`
    } else {
      store.config.voiceInputEnabled = false
      cloudMsgKind.value = 'fail'
      cloudMsg.value = res.message || '请填入火山引擎 App ID 和 Access Token 后再启用。'
    }
  } catch (e) {
    store.config.voiceInputEnabled = false
    cloudMsgKind.value = 'fail'
    cloudMsg.value = `启用云端语音失败：${errText(e)}`
  } finally {
    cloudChecking.value = false
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
      if (!status.hasTokens) miss.push('识别词表')
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
    message.value = `语音引擎与模型已就绪，语音输入已启用，可按 ${currentHotkey.value} 说话。`
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
  if (p === 'binary') return '语音引擎'
  if (p === 'tokens') return '识别词表'
  return '识别模型'
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
      <input type="checkbox" v-model="enabledModel" :disabled="downloading || cloudChecking" />
      <span>启用语音输入</span>
    </label>
    <p class="settings-section-hint">
      按热键 / 点小人说话，转写文字直接注入当前聚焦的输入框。默认关闭。支持中英混说，自动断句标点。
    </p>

    <!-- 语音引擎选择：本地 SenseVoice（离线/隐私）/ 云端火山（快且准，需联网+凭证） -->
    <div class="voice-engine">
      <span class="voice-engine-label">语音引擎</span>
      <div class="voice-engine-options">
        <label class="voice-engine-opt" :class="{ 'voice-engine-opt-active': engine === 'local' }">
          <input type="radio" value="local" v-model="engine" />
          <span class="voice-engine-opt-title">本地（SenseVoice）</span>
          <span class="voice-engine-opt-desc">离线、隐私，不上云；首次需下载模型（约 250MB）</span>
        </label>
        <label class="voice-engine-opt" :class="{ 'voice-engine-opt-active': engine === 'cloud' }">
          <input type="radio" value="cloud" v-model="engine" />
          <span class="voice-engine-opt-title">云端（豆包·火山）</span>
          <span class="voice-engine-opt-desc">快且准，中文强；需联网 + 火山引擎凭证（有免费额度）</span>
        </label>
      </div>
    </div>

    <!-- 自定义快捷键 -->
    <div class="voice-hotkey">
      <div class="voice-hotkey-row">
        <span class="voice-hotkey-label">触发快捷键</span>
        <button
          type="button"
          class="voice-hotkey-box"
          :class="{ 'voice-hotkey-recording': recording }"
          :disabled="savingHotkey"
          @click="recording ? stopRecording() : startRecording()"
        >
          <template v-if="recording">
            <span v-if="draftHotkey" class="voice-hotkey-keys">
              <kbd v-for="(part, i) in hotkeyParts(draftHotkey)" :key="i">{{ prettyKey(part) }}</kbd>
            </span>
            <span v-else class="voice-hotkey-hint-text">请按下组合键…（Esc 取消）</span>
          </template>
          <template v-else>
            <span class="voice-hotkey-keys">
              <kbd v-for="(part, i) in hotkeyParts(currentHotkey)" :key="i">{{ prettyKey(part) }}</kbd>
            </span>
          </template>
        </button>
        <button
          type="button"
          class="settings-btn voice-hotkey-action"
          :disabled="savingHotkey"
          @click="recording ? stopRecording() : startRecording()"
        >
          {{ recording ? '取消' : (savingHotkey ? '应用中…' : '录制') }}
        </button>
      </div>
      <p class="voice-hotkey-tip">
        点「录制」后按下组合键（需至少一个修饰键 Ctrl/Cmd/Shift/Alt + 一个主键），松开即生效。
      </p>
      <p
        v-if="hotkeyMsg"
        class="settings-msg"
        :class="`settings-msg-${hotkeyMsgKind === 'info' ? 'testing' : hotkeyMsgKind}`"
      >
        {{ hotkeyMsg }}
      </p>
    </div>

    <!-- ====== 云端（火山引擎）凭证区：仅选「云端」时显示 ====== -->
    <div v-if="engine === 'cloud'" class="voice-cloud">
      <div class="voice-cloud-field">
        <label class="voice-cloud-label">App ID</label>
        <input
          v-model.trim="volcAppId"
          class="settings-input"
          type="text"
          placeholder="火山语音控制台的 App ID（一串数字）"
          autocomplete="off"
        />
      </div>
      <div class="voice-cloud-field">
        <label class="voice-cloud-label">Access Token</label>
        <input
          v-model.trim="volcAccessToken"
          class="settings-input"
          type="password"
          placeholder="控制台的 Access Token（形如 volc_…）"
          autocomplete="off"
        />
      </div>
      <p class="voice-cloud-hint">
        去
        <a
          class="voice-cloud-link"
          href="https://console.volcengine.com/speech/service"
          target="_blank"
          rel="noreferrer"
          >火山引擎新版语音控制台</a
        >
        开通「流式语音识别大模型」，拿到 App ID + Access Token 填这里（有免费额度）。务必用新版控制台，旧版鉴权方式不同。
      </p>
      <div class="voice-cloud-actions">
        <button
          class="settings-btn voice-cloud-primary"
          :disabled="cloudChecking"
          @click="enableCloud"
        >
          {{ cloudChecking ? '校验中…' : (store.config.voiceInputEnabled ? '重新校验凭证' : '校验并启用') }}
        </button>
      </div>
      <p
        v-if="cloudMsg"
        class="settings-msg"
        :class="`settings-msg-${cloudMsgKind === 'info' ? 'testing' : cloudMsgKind}`"
      >
        {{ cloudMsg }}
      </p>
    </div>

    <!-- ====== 本地（SenseVoice）下载/状态/手动兜底区：仅选「本地」时显示 ====== -->
    <template v-if="engine === 'local'">
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
          自动下不动时，按下面三个地址手动下载（建议用浏览器或下载工具，支持断点续传），下好后丢进目标目录即可：
        </p>

        <ol class="voice-manual-list">
          <li>
            <div class="voice-manual-label">① 语音引擎（.tar.bz2，需解压）</div>
            <a v-if="assets?.binaryUrl" class="voice-manual-link" :href="assets.binaryUrl" target="_blank" rel="noreferrer">{{ assets.binaryUrl }}</a>
            <div class="voice-manual-note">
              下载后<b>解压</b>，把包内 <code>bin/</code> 下的 <code>{{ assets?.binaryName || 'sherpa-onnx-offline.exe' }}</code> 和所有 <code>.dll</code>（onnxruntime 等）平铺放进目标目录。
            </div>
          </li>
          <li>
            <div class="voice-manual-label">② 识别模型（约 228MB，直接放）</div>
            <a v-if="assets?.modelMirrorUrl" class="voice-manual-link" :href="assets.modelMirrorUrl" target="_blank" rel="noreferrer">国内镜像：{{ assets.modelMirrorUrl }}</a>
            <a v-if="assets?.modelUrl" class="voice-manual-link voice-manual-link-alt" :href="assets.modelUrl" target="_blank" rel="noreferrer">{{ assets.modelUrl }}</a>
            <div class="voice-manual-note">
              下载到的 <code>{{ assets?.modelName || 'model.int8.onnx' }}</code> <b>直接</b>放进目标目录（不要解压、不要改名）。
            </div>
          </li>
          <li>
            <div class="voice-manual-label">③ 识别词表（约 0.3MB，直接放）</div>
            <a v-if="assets?.tokensMirrorUrl" class="voice-manual-link" :href="assets.tokensMirrorUrl" target="_blank" rel="noreferrer">国内镜像：{{ assets.tokensMirrorUrl }}</a>
            <a v-if="assets?.tokensUrl" class="voice-manual-link voice-manual-link-alt" :href="assets.tokensUrl" target="_blank" rel="noreferrer">{{ assets.tokensUrl }}</a>
            <div class="voice-manual-note">
              下载到的 <code>{{ assets?.tokensName || 'tokens.txt' }}</code> <b>直接</b>放进目标目录（不要改名）。
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
          启用语音输入需下载语音引擎与模型（约 250MB，SenseVoice 中英日韩粤多语模型，仅首次）。是否下载？
        </p>
        <p class="voice-modal-proxy">{{ proxyHint }}</p>
        <div class="voice-modal-actions">
          <button class="settings-btn" @click="cancelDownload">否</button>
          <button class="settings-btn voice-modal-primary" @click="confirmDownload">是，下载</button>
        </div>
      </div>
    </div>
    </template>
  </section>
</template>

<style scoped>
/* 语音引擎选择 */
.voice-engine {
  margin-top: 12px;
}
.voice-engine-label {
  display: block;
  margin-bottom: 6px;
  font-size: 12px;
  color: var(--text-dim);
}
.voice-engine-options {
  display: flex;
  gap: 8px;
}
.voice-engine-opt {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 3px;
  padding: 8px 10px;
  background: var(--surface-2);
  border: var(--divider);
  border-radius: 8px;
  cursor: pointer;
}
.voice-engine-opt:hover {
  border-color: var(--accent);
}
.voice-engine-opt-active {
  border-color: var(--accent);
  box-shadow: 0 0 0 1px var(--accent) inset;
}
.voice-engine-opt input {
  margin-right: 4px;
}
.voice-engine-opt-title {
  font-size: 12px;
  font-weight: 600;
  color: var(--text);
}
.voice-engine-opt-desc {
  font-size: 10.5px;
  line-height: 1.5;
  color: var(--text-faint);
}

/* 云端凭证区 */
.voice-cloud {
  display: flex;
  flex-direction: column;
  gap: 8px;
  margin-top: 12px;
  padding: 12px;
  background: var(--surface-2);
  border: var(--divider);
  border-radius: 8px;
}
.voice-cloud-field {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.voice-cloud-label {
  font-size: 11.5px;
  color: var(--text-dim);
}
.voice-cloud-hint {
  margin: 0;
  font-size: 11px;
  line-height: 1.6;
  color: var(--text-faint);
}
.voice-cloud-link {
  color: var(--accent);
}
.voice-cloud-actions {
  display: flex;
  gap: 8px;
}
.voice-cloud-primary {
  color: var(--on-accent);
  background: var(--accent);
  border-color: var(--accent);
}
.voice-cloud-primary:hover:not(:disabled) {
  background: color-mix(in srgb, var(--accent) 88%, #000);
  border-color: color-mix(in srgb, var(--accent) 88%, #000);
}

/* 自定义快捷键 */
.voice-hotkey {
  margin-top: 10px;
}
.voice-hotkey-row {
  display: flex;
  align-items: center;
  gap: 8px;
}
.voice-hotkey-label {
  font-size: 12px;
  color: var(--text-dim);
}
.voice-hotkey-box {
  flex: 1;
  min-height: 30px;
  display: flex;
  align-items: center;
  gap: 5px;
  padding: 4px 8px;
  font-size: 11.5px;
  text-align: left;
  color: var(--text);
  background: var(--surface-2);
  border: var(--divider);
  border-radius: 6px;
  cursor: pointer;
}
.voice-hotkey-box:hover:not(:disabled) {
  border-color: var(--accent);
}
.voice-hotkey-box:disabled {
  opacity: 0.6;
  cursor: default;
}
.voice-hotkey-recording {
  border-color: var(--accent);
  box-shadow: 0 0 0 2px color-mix(in srgb, var(--accent) 30%, transparent);
}
.voice-hotkey-keys {
  display: inline-flex;
  align-items: center;
  gap: 5px;
}
.voice-hotkey-keys kbd {
  padding: 1px 6px;
  font-family: var(--mono, monospace);
  font-size: 10.5px;
  color: var(--text);
  background: var(--surface);
  border: var(--divider);
  border-radius: 4px;
}
.voice-hotkey-hint-text {
  font-size: 11px;
  color: var(--text-faint);
}
.voice-hotkey-action {
  flex: none;
}
.voice-hotkey-tip {
  margin: 6px 0 0;
  font-size: 10.5px;
  line-height: 1.5;
  color: var(--text-faint);
}

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
