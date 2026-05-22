// 同步：src/services/clean-commit-title.ts
// 前端 vite 编译不走后端 tsc 的 rootDir，所以保留一份独立拷贝。
// 修改时务必同步两边。

const LEADING_EMOJI_RE =
  /^(?:(?:\p{Extended_Pictographic}|\p{Emoji_Presentation}|[\u{1F1E6}-\u{1F1FF}]|️|‍)+\s*)+/u

const CC_PREFIX_RE =
  /^(?:feat|fix|refactor|build|chore|docs|test|style|perf|ci|revert|wip)(?:\([^)]+\))?!?\s*:\s*/i

export function cleanCommitTitle(title: string, maxLen = 60): string {
  if (!title) return ''
  let s = title

  for (let i = 0; i < 3; i++) {
    const before = s
    s = s.replace(LEADING_EMOJI_RE, '')
    s = s.replace(CC_PREFIX_RE, '')
    if (s === before) break
  }

  s = s.trim()
  if (s.length > maxLen) {
    s = s.slice(0, maxLen - 1).trimEnd() + '…'
  }
  return s
}
