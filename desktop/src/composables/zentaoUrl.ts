// 把用户输入的禅道地址清洗成可拼接 API 路径的根 URL。
//
// 用户最容易踩两类坑：
//   1. 直接从浏览器地址栏复制了"登录页/任意页面"的 URL，里面带 user-login-xxx.html
//      或者 my-tasks.html 这种页面路由 → 我们拼 /api.php/v1/tokens 时 URL 是错的
//   2. URL 后面带 ?refer=xxx#section 这种 query / fragment
//
// 规则：
//   - 去前后空白
//   - 没写 http:// / https:// 时默认补 http://
//   - 把 path 按 '/' 切段，**从左到右走，遇到第一个看起来是"入口文件"的段就截断**：
//       *.html / *.htm / *.php / *.json / *.jsp / *.asp / *.aspx
//     这能正确处理:
//       /zentao/user-login-XXX.html  → /zentao
//       /zentao/index.html           → /zentao
//       /zentao/api.php/v1/tokens    → /zentao
//       /zentao                      → /zentao（保持）
//   - 去 query / fragment
//   - 去尾斜杠（统一形态，调用方拼 /api.php... 不会重复斜杠）
//
// 输入解析失败时返回原值 — 让后端测试连接报错，不会"静默丢数据"

export function normalizeZentaoBaseUrl(input: string): string {
  const trimmed = input.trim()
  if (!trimmed) return ''

  const withScheme = /^https?:\/\//i.test(trimmed) ? trimmed : `http://${trimmed}`

  let u: URL
  try {
    u = new URL(withScheme)
  } catch {
    return trimmed
  }

  // 丢 query + fragment
  u.search = ''
  u.hash = ''

  const segs = u.pathname.split('/').filter((s) => s.length > 0)
  const kept: string[] = []
  for (const seg of segs) {
    if (/\.(html?|php|json|jsp|aspx?)$/i.test(seg)) break
    kept.push(seg)
  }
  u.pathname = kept.length ? '/' + kept.join('/') : '/'

  return u.toString().replace(/\/+$/, '')
}
