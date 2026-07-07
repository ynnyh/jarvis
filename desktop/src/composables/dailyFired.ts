/**
 * 当日去重工具：统一的「今天 + category + suffix」localStorage 键，
 * 供各提醒 composable 复用，消除 useWorkdayNudges / useTimeGreetings /
 * useEffortClosingCheck / useEveningReminder 里各自复制的 todayKey/hasFired/markFired 样板。
 *
 * 语义：key 形如 `jarvis.{category}.{YYYY-MM-DD}.{suffix}`，按自然日自动失效
 * （次日拼出的 key 不同，旧 flag 自然读不到，无需主动清理）。
 */

/** 当前自然日，格式 YYYY-MM-DD（与各处既有 todayStr 实现一致）。 */
export function todayStr(): string {
  const d = new Date()
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(
    d.getDate(),
  ).padStart(2, '0')}`
}

function dailyKey(category: string, suffix: string): string {
  return `jarvis.${category}.${todayStr()}.${suffix}`
}

/** 当日是否已触发（写入固定 '1' 标记的场景）。 */
export function hasFired(category: string, suffix: string): boolean {
  try {
    return localStorage.getItem(dailyKey(category, suffix)) === '1'
  } catch {
    return false
  }
}

/** 标记当日已触发（写入固定 '1'）。 */
export function markFired(category: string, suffix: string): void {
  try {
    localStorage.setItem(dailyKey(category, suffix), '1')
  } catch {
    // localStorage 满了忽略
  }
}

/** 读取当日原始标记值（不要求等于 '1'，用于存任意 payload 的场景）。 */
export function getFlag(category: string, suffix: string): string | null {
  try {
    return localStorage.getItem(dailyKey(category, suffix))
  } catch {
    return null
  }
}

/** 写入当日原始标记值。 */
export function setFlag(category: string, suffix: string, value: string): void {
  try {
    localStorage.setItem(dailyKey(category, suffix), value)
  } catch {
    // 忽略 storage quota
  }
}
