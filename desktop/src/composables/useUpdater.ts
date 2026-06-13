import { ref, computed, markRaw } from 'vue'
import { check, type Update } from '@tauri-apps/plugin-updater'
import { relaunch } from '@tauri-apps/plugin-process'
import { getVersion } from '@tauri-apps/api/app'

/**
 * 启动后台轮询 Tauri updater。
 *
 * 流程：
 *   1. 启动后 30s 第一次 check（避开冷启动尖峰）
 *   2. 之后每 6 小时一次
 *   3. 有新版本 → 通过 onAvailable 回调把版本号交给上层（一般弹气泡提示）
 *   4. 用户点确认 → installAndRestart() 下载安装包 → 静默安装 → 重启
 *
 * 不主动下载安装：要给用户拒绝的余地，避免开了一半工被强退。
 *
 * 状态机 phase：UI 用这个来判断按钮可点 / 显示什么文案
 * - 'idle'        初始
 * - 'checking'    正在调 endpoint 查
 * - 'no-update'   查完了，没有新版本
 * - 'available'   有新版本，等用户点确认
 * - 'downloading' 正在下载，downloadProgress 持续更新
 * - 'installing'  下载完成、走 NSIS/PKG 安装中（这一步 Tauri 不给进度，文案告知）
 * - 'installed'   安装结束，即将 relaunch
 * - 'error'       任何环节出错，lastError 拿原因
 *
 * busy 状态（isBusy）= checking | downloading | installing —— UI 据此 disable
 * 「检查更新」按钮防止用户连点导致重复下载 / 状态错乱。
 */

interface UseUpdaterOptions {
  /** 检测到新版本时回调（用于弹气泡 / 改菜单徽标） */
  onAvailable?: (version: string, notes: string) => void
  /** 首次检查延迟（毫秒） */
  initialDelay?: number
  /** 轮询间隔（毫秒） */
  interval?: number
}

export type UpdaterPhase =
  | 'idle'
  | 'checking'
  | 'no-update'
  | 'available'
  | 'downloading'
  | 'installing'
  | 'installed'
  | 'error'

export interface DownloadProgress {
  downloaded: number  // 字节
  total: number       // 字节，0 表示未知
  percent: number     // 0~100，total 为 0 时持平 0
}

const SIX_HOURS = 6 * 60 * 60 * 1000

/** 给异步操作套超时，超时则 reject。endpoint(Gitee) 偶发网络 stall 时 check()
 * 可能长时间不返回，UI 会卡在"检查中"并因 isBusy 锁死所有按钮（含关闭）。 */
function withTimeout<T>(p: Promise<T>, ms: number, timeoutMsg: string): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const t = setTimeout(() => reject(new Error(timeoutMsg)), ms)
    p.then((v) => { clearTimeout(t); resolve(v) }, (e) => { clearTimeout(t); reject(e) })
  })
}

export function useUpdater(options: UseUpdaterOptions = {}) {
  const { onAvailable, initialDelay = 30_000, interval = SIX_HOURS } = options
  const available = ref<Update | null>(null)
  const phase = ref<UpdaterPhase>('idle')
  const lastError = ref<string | null>(null)
  const currentVersion = ref<string>('')
  const downloadProgress = ref<DownloadProgress>({ downloaded: 0, total: 0, percent: 0 })

  // 启动时拉一下当前版本号——只需要一次，crash 了也无所谓
  getVersion().then(v => { currentVersion.value = v }).catch(() => {})

  const newVersion = computed(() => available.value?.version ?? '')
  const releaseNotes = computed(() => available.value?.body ?? '')

  /**
   * 「忙」状态：UI 据此禁用所有"开始/检查"入口，避免下载中途用户再点一次又
   * 触发一个 check / downloadAndInstall。Tauri updater 插件本身没做并发互斥，
   * 重复调用会同时下载 / 安装，状态机会乱。
   */
  const isBusy = computed(
    () =>
      phase.value === 'checking' ||
      phase.value === 'downloading' ||
      phase.value === 'installing',
  )

  /** 立刻查一次。返回是否有新版本。busy 时直接拒绝。 */
  async function checkNow(): Promise<boolean> {
    if (isBusy.value) return false
    phase.value = 'checking'
    lastError.value = null
    try {
      // check() 可能因 endpoint(Gitee) 网络 stall 长时间不返回 → phase 卡 checking、
      // isBusy 锁死 UI，还会让后续自动轮询全部被 isBusy 拒绝。加 30s 超时解锁。
      const update = await withTimeout(check(), 30_000, '检查更新超时，请稍后重试')
      if (update) {
        // markRaw：Tauri 的 Update 类内部用 #xxx 私有字段，ref/reactive 会拿
        // Proxy 包对象，而 JS 规范要求 # 字段访问的 this 必须是该类原始实例，
        // Proxy 不算 → 后续调 downloadAndInstall 时报 "Cannot read private
        // member from an object whose class did not declare it"。markRaw
        // 让 ref 的 .value 直接持有原始 Update 实例，绕开代理。
        available.value = markRaw(update)
        phase.value = 'available'
        onAvailable?.(update.version, update.body ?? '')
        return true
      }
      available.value = null
      phase.value = 'no-update'
      return false
    } catch (e: any) {
      lastError.value = e?.message ?? String(e)
      phase.value = 'error'
      console.warn('[updater] check 失败：', e)
      return false
    }
  }

  /** 下载并安装当前发现的版本，安装完自动重启 app。busy 时拒绝重入。 */
  async function installAndRestart() {
    if (isBusy.value) return
    if (!available.value) return
    downloadProgress.value = { downloaded: 0, total: 0, percent: 0 }
    phase.value = 'downloading'
    try {
      // Tauri updater 的 downloadAndInstall 接受进度回调。'Started' 给总大小，
      // 'Progress' 每个 chunk 给增量（不是累计），'Finished' 标记下载结束。
      // 下载完成后插件触发安装（Windows NSIS / macOS PKG），这阶段没进度回调，
      // 切到 'installing' 文案让用户知道还在做事。
      await available.value.downloadAndInstall((event) => {
        switch (event.event) {
          case 'Started': {
            const total = event.data.contentLength ?? 0
            downloadProgress.value = { downloaded: 0, total, percent: 0 }
            break
          }
          case 'Progress': {
            const prev = downloadProgress.value
            const downloaded = prev.downloaded + (event.data.chunkLength ?? 0)
            const percent = prev.total > 0
              ? Math.min(100, Math.round((downloaded / prev.total) * 100))
              : 0
            downloadProgress.value = { downloaded, total: prev.total, percent }
            break
          }
          case 'Finished': {
            // 进度拉满让用户视觉上看到 100%，然后切 installing
            const prev = downloadProgress.value
            downloadProgress.value = {
              downloaded: prev.total || prev.downloaded,
              total: prev.total,
              percent: 100,
            }
            phase.value = 'installing'
            break
          }
        }
      })
      phase.value = 'installed'
      await relaunch()
    } catch (e: any) {
      const msg = e?.message ?? String(e)
      lastError.value = msg
      phase.value = 'error'
      console.error('[updater] install 失败：', e)
      // 安装失败后清掉 available，避免 UI 上"下载更新"按钮无限出现。
      // 下次 checkNow() 会重新检测。
      available.value = null
    }
  }

  /** 回到 idle。installing 阶段（替换 .app / NSIS 安装）不可中断会损坏安装，禁止
   * 重置；其余阶段（含 downloading）允许，让用户在网络/plugin 卡住时能关窗逃离锁死。
   * 注意：Tauri updater 插件不支持取消进行中的下载，reset 只解锁 UI 状态机，
   * 后台若最终完成仍会 relaunch（用户主动更新，完成重启属预期）。 */
  function reset() {
    if (phase.value === 'installing') return
    phase.value = 'idle'
    lastError.value = null
    downloadProgress.value = { downloaded: 0, total: 0, percent: 0 }
  }

  let timer: ReturnType<typeof setTimeout> | null = null
  let intervalTimer: ReturnType<typeof setInterval> | null = null

  function start() {
    timer = setTimeout(() => {
      checkNow()
      intervalTimer = setInterval(checkNow, interval)
    }, initialDelay)
  }

  function stop() {
    if (timer) clearTimeout(timer)
    if (intervalTimer) clearInterval(intervalTimer)
    timer = null
    intervalTimer = null
  }

  return {
    available,
    phase,
    isBusy,
    currentVersion,
    newVersion,
    releaseNotes,
    downloadProgress,
    lastError,
    checkNow,
    installAndRestart,
    reset,
    start,
    stop,
  }
}
