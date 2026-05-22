/**
 * 清理 commit 标题，让它适合直接出现在日报里。
 *
 * 规则：
 *   1. 去掉行首的 emoji / gitmoji（含 variation selector）
 *   2. 去掉 conventional commits 前缀：`type:` / `type(scope):` / `type!:`
 *      （type ∈ feat|fix|refactor|build|chore|docs|test|style|perf|ci|revert|wip）
 *   3. 去掉首尾空白
 *   4. 超过 maxLen（默认 60）截断并加省略号
 *
 * 注意：此函数同时在 desktop/src/composables/cleanCommitTitle.ts 有一份镜像
 * （前端 vite 编译不走 tsc rootDir，无法直接 import）。修改时务必同步。
 */

// 同步：desktop/src/composables/cleanCommitTitle.ts
const LEADING_EMOJI_RE =
  /^(?:(?:\p{Extended_Pictographic}|\p{Emoji_Presentation}|[\u{1F1E6}-\u{1F1FF}]|️|‍)+\s*)+/u

const CC_PREFIX_RE =
  /^(?:feat|fix|refactor|build|chore|docs|test|style|perf|ci|revert|wip)(?:\([^)]+\))?!?\s*:\s*/i

export function cleanCommitTitle(title: string, maxLen = 60): string {
  if (!title) return ''
  let s = title

  // 反复剥离前缀+emoji（顺序不定，比如 "🎨 feat: xxx" 和 "feat: 🎨 xxx"）
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
