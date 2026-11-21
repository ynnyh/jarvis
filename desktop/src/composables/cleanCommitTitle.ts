const LEADING_EMOJI_RE =
  /^(?:(?:\p{Extended_Pictographic}|\p{Emoji_Presentation}|[\u{1F1E6}-\u{1F1FF}]|️|‍)+\s*)+/u

const CC_PREFIX_RE =
  /^(?:feat|fix|refactor|build|chore|docs|test|style|perf|ci|revert|wip)(?:\([^)]+\))?!?\s*:\s*/i

// 剥标题自带序号前缀用："." 后必须跟空白才视为序号（防误伤 "1.5 版本发布"）
const LEADING_INDEX_RE =
  /^(?:\s*(?:[0-9０-９]{1,3}\s*[、,，:：;；)）]\s*|[0-9０-９]{1,3}\.\s+|\([0-9]{1,3}\)\s*|（[0-9０-９]{1,3}）\s*|[①-⑳]\s*|[一二三四五六七八九十]{1,3}\s*[、.．]\s*))+/

/** 剥掉标题自带的序号前缀（"1、xxx" "2. xxx" "(3) xxx" "（4）xxx" "⑤xxx" "五、xxx"），
 * 避免拼装工时内容时出现「2、1、xxx」双编号。与后端 git_scan::strip_leading_index 行为一致。 */
export function stripLeadingIndex(s: string): string {
  return s.replace(LEADING_INDEX_RE, '').trim()
}

/** 清理 commit 标题：剥 emoji + conventional commit 前缀；不截断——
 * 工时/日报内容要求完整展示（UI 侧用限高+滚动/展开兜底）。 */
export function cleanCommitTitle(title: string): string {
  if (!title) return ''
  let s = title

  for (let i = 0; i < 3; i++) {
    const before = s
    s = s.replace(LEADING_EMOJI_RE, '')
    s = s.replace(CC_PREFIX_RE, '')
    if (s === before) break
  }

  return s.trim()
}
