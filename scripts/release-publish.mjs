#!/usr/bin/env node
/**
 * 本地发版编排器：等 Actions 构建完成 → 下载产物 → 本地发布双渠道 → 校验。
 *
 * 为什么发布要在本地跑（2026-10-28 改造）：GitHub runner（美国）往 GitHub
 * Releases / Gitee 传 20~100MB 大文件经常超时（v0.12.0 两次 45min 实锤，
 * UND_ERR_HEADERS_TIMEOUT），而本机到 Gitee（国内 OTA 主源）和 GitHub 都稳。
 * Actions 只负责跨平台构建，发布从此一条本地命令完成。
 *
 * 用法：
 *   GITEE_TOKEN=<gitee私人令牌> npm run release:publish              # 发当前版本（读 tauri.conf.json）
 *   GITEE_TOKEN=... npm run release:publish -- v0.12.1               # 指定 tag
 *   GITEE_TOKEN=... npm run release:publish -- --sync-only           # 只补发 Gitee（从 GitHub Release 同步附件 + latest.json 由全量模式负责）
 *
 * 前置：gh CLI 已登录（构建等待/产物下载）；GITEE_TOKEN（Gitee 发布）。
 * 可选：GITHUB_REPO（默认 ynnyh/jarvis）。
 *
 * 流程：
 *   1. 找到该 tag 的 Release run（构建 + quality 门），未完成则 gh run watch 等待
 *   2. gh run download 拉全部构建产物到临时目录
 *   3. ARTIFACTS_DIR 指向产物 → 复用 publish-to-gitee.mjs 全量发布
 *      （GitHub Release 保底 + Gitee Release + OTA latest.json 写入 Gitee 仓库 main）
 *   4. 校验 GitHub Release 资产清单，打印双渠道状态
 */

import { execFileSync, spawnSync } from 'node:child_process'
import { rmSync } from 'node:fs'
import { resolve } from 'node:path'
import { readFileSync } from 'node:fs'

const repoRoot = resolve(import.meta.dirname, '..')
const args = process.argv.slice(2)
const tagArg = args.find(a => !a.startsWith('--'))

// 版本号：参数优先，否则读 tauri.conf.json
const tag = tagArg ?? `v${JSON.parse(readFileSync(resolve(repoRoot, 'src-tauri/tauri.conf.json'), 'utf8')).version}`
if (!/^v\d+\.\d+\.\d+/.test(tag)) {
  console.error(`❌ tag 名不合法: ${tag}`)
  process.exit(1)
}

function gh(args2, opts = {}) {
  return execFileSync('gh', args2, { encoding: 'utf8', ...opts })
}

console.log(`\n🚀 Jarvis ${tag} 本地发布\n`)

// ===== 1. 定位该 tag 的 Release run =====
let runId
try {
  const runs = JSON.parse(gh(['run', 'list', '--workflow=Release', '--branch', tag, '--json', 'databaseId,status,conclusion', '--limit', '5']))
  if (runs.length === 0) {
    console.error(`❌ 没找到 ${tag} 的 Release run——确认 tag 已推送（git push origin ${tag}）`)
    process.exit(1)
  }
  runId = runs[0].databaseId
  console.log(`✓ Release run #${runId}（${runs[0].status}${runs[0].conclusion ? '/' + runs[0].conclusion : ''}）`)
} catch (e) {
  console.error('❌ gh run list 失败（gh CLI 已登录？）:', e.message)
  process.exit(1)
}

// ===== 2. 等构建完成（已结束则直接过）=====
console.log('⏳ 等待构建完成（macOS runner 高峰期可能排队很久，Ctrl+C 可中断后稍后再跑，产物不会丢）...')
try {
  gh(['run', 'watch', String(runId), '--exit-status', '--interval', '30'], { stdio: 'inherit' })
} catch {
  console.error('❌ Release run 失败——先看 Actions 日志修构建，产物不出全发布无从谈起')
  process.exit(1)
}
console.log('✓ 构建完成')

// ===== 3. 下载产物 =====
const artifactsDir = resolve(repoRoot, '.release-artifacts')
rmSync(artifactsDir, { recursive: true, force: true })
console.log('⏳ 下载构建产物...')
gh(['run', 'download', String(runId), '-D', artifactsDir])
console.log(`✓ 产物就绪：${artifactsDir}`)

// ===== 4. 本地发布（GitHub Release 保底 + Gitee + OTA latest.json）=====
if (!process.env.GITEE_TOKEN) {
  console.error('❌ 缺少 GITEE_TOKEN 环境变量（Gitee 私人令牌，发布 Gitee Release + 写 OTA latest.json 必需）')
  process.exit(1)
}
console.log('⏳ 本地发布（GitHub 保底 + Gitee + latest.json）...')
// gh auth token 提供给脚本做 GitHub Release 保底（脚本用 fetch 直传）
const ghToken = spawnSync('gh', ['auth', 'token'], { encoding: 'utf8' }).stdout.trim()
const res = spawnSync('node', [resolve(repoRoot, 'scripts/publish-to-gitee.mjs')], {
  stdio: 'inherit',
  env: {
    ...process.env,
    ARTIFACTS_DIR: artifactsDir,
    GITHUB_REPO: process.env.GITHUB_REPO ?? 'ynnyh/jarvis',
    ...(ghToken ? { GITHUB_TOKEN: ghToken } : {}),
  },
})
if (res.status !== 0) {
  console.error('❌ 发布脚本失败——Gitee 部分可能缺失，修复后重跑本命令（幂等，已传资产会被复用/覆盖）')
  process.exit(res.status ?? 1)
}

// ===== 5. 校验 GitHub Release 资产 =====
const assets = JSON.parse(gh(['release', 'view', tag, '--json', 'assets', '-q', '[.assets[].name]']))
console.log(`\n✅ ${tag} 发布完成`)
console.log(`  GitHub: https://github.com/ynnyh/jarvis/releases/tag/${tag}（${assets.length} 个资产）`)
if (assets.length === 0) console.warn('  ⚠ GitHub 资产为空——检查 publish-to-gitee.mjs 的 GitHub 上传日志')
console.log(`  Gitee:  https://gitee.com/ynnyh/jarvis/releases/tag/${tag}`)
console.log(`  OTA:    https://gitee.com/ynnyh/jarvis/raw/main/latest.json`)
