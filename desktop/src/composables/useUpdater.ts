import { ref } from 'vue'
import { check, type Update } from '@tauri-apps/plugin-updater'
import { relaunch } from '@tauri-apps/plugin-process'

/**
 * 启动后台轮询 Tauri updater。
 *
 * 流程：
 *   1. 启动后 30s 第一次 check（避开冷启动尖峰）
 *   2. 之后每 6 小时一次
 *   3. 有新版本 → 通过 onAvailable 回调把版本号交给上层（一般弹个气泡提示）
 *   4. 用户点确认 → installAndRestart() 下载安装包 → 静默安装 → 重启
 *
 * 不主动下载安装：要给用户拒绝的余地，避免开了一半工被强退。
 */

interface UseUpdaterOptions {
  /** 检测到新版本时回调（用于弹气泡 / 改菜单徽标） */
  onAvailable?: (version: string, notes: string) => void
  /** 首次检查延迟（毫秒） */
  initialDelay?: number
  /** 轮询间隔（毫秒） */
  interval?: number
}

const SIX_HOURS = 6 * 60 * 60 * 1000

export function useUpdater(options: UseUpdaterOptions = {}) {
  const { onAvailable, initialDelay = 30_000, interval = SIX_HOURS } = options
  const available = ref<Update | null>(null)
  const checking = ref(false)
  const lastError = ref<string | null>(null)

  /** 立刻查一次。返回是否有新版本。 */
  async function checkNow(): Promise<boolean> {
    if (checking.value) return false
    checking.value = true
    lastError.value = null
    try {
      const update = await check()
      if (update) {
        available.value = update
        onAvailable?.(update.version, update.body ?? '')
        return true
      }
      available.value = null
      return false
    } catch (e: any) {
      // endpoint 不通 / 公钥不匹配 / 网络抖动 都会到这里。失败不打扰用户，只记日志。
      lastError.value = e?.message ?? String(e)
      console.warn('[updater] check 失败：', e)
      return false
    } finally {
      checking.value = false
    }
  }

  /** 下载并安装当前发现的版本，安装完自动重启 app */
  async function installAndRestart() {
    if (!available.value) return
    try {
      await available.value.downloadAndInstall()
      await relaunch()
    } catch (e: any) {
      lastError.value = e?.message ?? String(e)
      console.error('[updater] install 失败：', e)
    }
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

  return { available, checking, lastError, checkNow, installAndRestart, start, stop }
}
