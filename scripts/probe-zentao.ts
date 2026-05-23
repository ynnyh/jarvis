#!/usr/bin/env node
// 禅道连接探测脚本 —— 不进打包，纯诊断工具
//
// 用法：
//   npx tsx scripts/probe-zentao.ts <baseUrl> <account> <password>
//
// 用 Node fetch 模拟 Tauri credentials.rs::zentao_test_connection 发的请求，
// 输出完整 URL、状态码、响应头、body 前 500 字符。
//
// 同时用两种 User-Agent 分别探一次：
//   1) reqwest 默认 UA（之前会 500 的情况）
//   2) Mozilla 浏览器 UA（Node 端 ZenTaoProvider 已经在用的）
// 对比能直接看出禅道服务端是否在做 UA 过滤。
//
// 账号密码只在本进程内存里，不写文件、不发任何第三方。

import { normalizeZentaoBaseUrl } from '../desktop/src/composables/zentaoUrl.ts'

const [rawBase, account, password] = process.argv.slice(2)
if (!rawBase || !account || !password) {
  console.error('用法: npx tsx scripts/probe-zentao.ts <baseUrl> <account> <password>')
  process.exit(2)
}

const base = normalizeZentaoBaseUrl(rawBase)
console.log(`📥 输入 baseUrl: ${rawBase}`)
console.log(`🔧 清洗后:       ${base}`)
console.log('')

const url = `${base}/api.php/v1/tokens`
console.log(`🌐 请求 URL:     ${url}`)
console.log('')

const UAs: [string, string][] = [
  ['reqwest-default', 'reqwest/0.12'],
  ['mozilla',         'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36'],
]

for (const [tag, ua] of UAs) {
  console.log(`=== [${tag}]  User-Agent: ${ua}`)
  try {
    const t0 = Date.now()
    const resp = await fetch(url, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'Accept': 'application/json',
        'User-Agent': ua,
      },
      body: JSON.stringify({ account, password }),
    })
    const ms = Date.now() - t0
    const body = await resp.text()
    console.log(`    HTTP ${resp.status} (${ms} ms)`)
    console.log(`    响应头:`)
    for (const [k, v] of resp.headers) console.log(`        ${k}: ${v}`)
    const bodyHead = body.length > 500 ? body.slice(0, 500) + '...(truncated)' : body
    console.log(`    Body (${body.length} bytes):`)
    console.log(bodyHead.split('\n').map(l => '        ' + l).join('\n'))
    try {
      const j = JSON.parse(body)
      if (j.token) console.log(`    ✓ 拿到 token: ${j.token.slice(0, 12)}...`)
    } catch {}
  } catch (e: any) {
    console.log(`    ✗ ${e?.message ?? e}`)
  }
  console.log('')
}
