#!/usr/bin/env node
// 禅道连接测试 helper —— Tauri 后端通过 spawn(node, [此脚本, base, account, password])
// 调用，从 stdout 读最后一行 `__JARVIS_RESULT__{json}` 拿结果。
//
// 为什么不让 Rust 自己发 HTTP？
//   reqwest 在某些禅道前置（IIS 反代 + WAF）下被拦成 403，即便 UA / Accept /
//   no_proxy / 擦掉代理 env 都无效。实测表明 reqwest 的请求会被 WAF 误判，
//   而 Node 18+ 全局 fetch（undici）从同一台机器同一组凭证发出去能拿到
//   token —— daemon 这条路径长期验证可用。所以让 Tauri spawn node.exe 跑
//   这段，100% 与 daemon 共用网络栈。
//
// 输入：argv[2..] = base, account, password
// 输出：stdout 最后一行 = `__JARVIS_RESULT__` + JSON.stringify({ ok, message })
// 退出码：永远 0（错误也以 JSON 形式回传），让 Rust 端只关心 stdout。

function normalize(input) {
  const trimmed = String(input ?? '').trim()
  if (!trimmed) return ''
  const withScheme = /^https?:\/\//i.test(trimmed) ? trimmed : `http://${trimmed}`
  let u
  try { u = new URL(withScheme) } catch { return trimmed }
  u.search = ''
  u.hash = ''
  const segs = u.pathname.split('/').filter(s => s.length > 0)
  const kept = []
  for (const s of segs) {
    if (/\.(html?|php|json|jsp|aspx?)$/i.test(s)) break
    kept.push(s)
  }
  u.pathname = kept.length ? '/' + kept.join('/') : '/'
  return u.toString().replace(/\/+$/, '')
}

// 已知四种失败现场（按出现频率）：
//   500 + HTML  → 禅道后台没启 API，或版本太老没 v1 REST
//   404         → URL 路径错（多半 baseUrl 漏了 /zentao 子路径）
//   200 + HTML  → baseUrl 命中登录页或别的 HTML（如 user-login-xxx.html）
//   401/403     → 账号密码错，或被前置 WAF/反代拦截
//   2xx 无 token → 账号密码错或 API 返回了别的 shape
function diagnose(url, status, body) {
  const trimmed = (body || '').trim()
  const snippet = trimmed.slice(0, 200)
  const looksHtml = trimmed.startsWith('<')
    || trimmed.toLowerCase().includes('<!doctype html')
    || trimmed.toLowerCase().includes('<html')

  if (status === 500 && looksHtml) {
    return `禅道服务器内部错误（HTTP 500）。最常见原因：\n`
      + `1) 后台 → 二次开发 → API 未启用 → 联系禅道管理员开启\n`
      + `2) 禅道版本低于 12.3.3，没有 v1 REST 接口\n`
      + `实际请求：${url}`
  }
  if (status === 404) {
    return `找不到接口（HTTP 404）。多半是 baseUrl 漏了子路径（常见为 /zentao）。\n实际请求：${url}`
  }
  if ((status === 200 || status === 201) && looksHtml) {
    return `禅道返回了 HTML 页面而不是 JSON，说明 baseUrl 命中了登录页或别的网页。\n检查 baseUrl 是否多了页面路径（如 /user-login-xxx.html）。\n实际请求：${url}`
  }
  if (status === 401 || status === 403) {
    // 403 + HTML 多半是经过外网反代/WAF。提示用户改用内网地址
    if (looksHtml) {
      return `请求被前置代理或 WAF 拦截（HTTP ${status}）。\n如果 baseUrl 是公网域名，尝试改用禅道内网地址（如 http://192.168.x.x:port/zentao）。\n实际请求：${url}`
    }
    return `账号或密码错误（HTTP ${status}）。\n响应：${snippet}`
  }
  if (status >= 200 && status < 300) {
    return `账号或密码错误，或禅道未返回 token。\n响应：${snippet}`
  }
  return `禅道返回 HTTP ${status}：${snippet}`
}

function emit(obj) {
  process.stdout.write('\n__JARVIS_RESULT__' + JSON.stringify(obj) + '\n')
}

const [rawBase, account, password] = process.argv.slice(2)
const base = normalize(rawBase)

if (!base) { emit({ ok: false, message: '禅道地址不能为空' }); process.exit(0) }
if (!account || !account.trim()) { emit({ ok: false, message: '账号不能为空' }); process.exit(0) }

const url = `${base}/api.php/v1/tokens`

try {
  const resp = await fetch(url, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36',
    },
    body: JSON.stringify({ account, password: password ?? '' }),
  })
  const body = await resp.text()

  if (!resp.ok) {
    emit({ ok: false, message: diagnose(url, resp.status, body) })
    process.exit(0)
  }

  let token = ''
  try { token = JSON.parse(body).token || '' } catch {}
  if (!token) {
    emit({ ok: false, message: diagnose(url, resp.status, body) })
    process.exit(0)
  }

  emit({
    ok: true,
    message: `连接成功，已获取 Token（${token.slice(0, 10)}...）\nbaseUrl 已规范化为：${base}`,
  })
} catch (e) {
  emit({
    ok: false,
    message: `无法连接禅道：${e?.message ?? e}\n请检查地址是否正确、是否在公司网络。\n实际请求：${url}`,
  })
}
