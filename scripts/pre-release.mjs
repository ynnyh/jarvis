#!/usr/bin/env node
/**
 * 发版前预检脚本。
 * 用法：node scripts/pre-release.mjs [版本号]
 *   - 不传版本号则自动从 Cargo.toml 读取
 *   - 传了版本号则校验是否与文件一致
 *
 * 检查项：
 *   1. Cargo.toml 与 tauri.conf.json 版本一致
 *   2. 版本号格式合法 (semver)
 *   3. 远端不存在同名 tag
 *   4. cargo check 通过
 *   5. npm run check:text 通过
 *   6. 未提交变更检查（提醒）
 */

import { execSync } from 'node:child_process'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'

const root = resolve(import.meta.dirname, '..')

function run(cmd, opts = {}) {
  try {
    return execSync(cmd, { cwd: root, encoding: 'utf8', stdio: 'pipe', ...opts })
  } catch (e) {
    if (opts.allowFail) return null
    throw e
  }
}

function log(tag, msg) {
  const icons = { ok: '✓', fail: '✗', warn: '⚠', info: '→' }
  console.log(` ${icons[tag] || '·'} ${msg}`)
}

let passed = 0
let failed = 0
let warned = 0

function check(name, fn) {
  try {
    const result = fn()
    if (result === 'warn') {
      warned++
      log('warn', name)
    } else {
      passed++
      log('ok', name)
    }
  } catch (e) {
    failed++
    log('fail', `${name}\n    ${e.message.split('\n')[0]}`)
  }
}

// ── 1. 读取版本号 ──
const cargoToml = readFileSync(resolve(root, 'src-tauri/Cargo.toml'), 'utf8')
const cargoVer = cargoToml.match(/^version\s*=\s*"([^"]+)"/m)?.[1]
const tauriConf = JSON.parse(readFileSync(resolve(root, 'src-tauri/tauri.conf.json'), 'utf8'))
const tauriVer = tauriConf.version

const argVer = process.argv[2]

check('Cargo.toml 与 tauri.conf.json 版本一致', () => {
  if (cargoVer !== tauriVer) {
    throw new Error(`Cargo.toml=${cargoVer}, tauri.conf.json=${tauriVer}`)
  }
})

const targetVer = argVer || cargoVer

check('版本号格式合法', () => {
  if (!/^\d+\.\d+\.\d+(-[\w.]+)?$/.test(targetVer)) {
    throw new Error(`${targetVer} 不是合法 semver`)
  }
})

if (argVer && argVer !== cargoVer) {
  check(`指定版本 ${argVer} 与文件版本 ${cargoVer} 一致`, () => {
    throw new Error(`不一致：参数=${argVer}, 文件=${cargoVer}`)
  })
}

// ── 2. 远端 tag 检查 ──
check(`远端不存在 tag v${targetVer}`, () => {
  const tags = run('git ls-remote --tags origin', { allowFail: true }) || ''
  if (tags.includes(`refs/tags/v${targetVer}`)) {
    throw new Error(`v${targetVer} 已存在于远端`)
  }
})

// ── 3. 编译检查 ──
check('cargo check 通过', () => {
  run('cargo check --manifest-path src-tauri/Cargo.toml', { timeout: 120_000 })
})

// ── 4. 文本编码检查 ──
check('npm run check:text 通过', () => {
  run('npm run check:text', { timeout: 30_000 })
})

// ── 5. 未提交变更提醒 ──
check('工作区无未提交变更', () => {
  const status = run('git status --porcelain')
  if (status.trim()) {
    return 'warn'
  }
})

// ── 6. signing key 本地可验证（可选） ──
check('updater signing key 可用（本地有密钥时）', () => {
  if (!process.env.TAURI_SIGNING_PRIVATE_KEY && !process.env.TAURI_SIGNING_PRIVATE_KEY_B64) {
    return 'warn'
  }
  // 有密钥就跑 prepare 脚本验证
  run('node scripts/ci/prepare-tauri-signing.mjs', { timeout: 30_000 })
})

// ── 结果 ──
console.log('')
if (failed > 0) {
  console.log(`✗ 预检失败：${failed} 项不通过，${passed} 项通过，${warned} 项警告`)
  console.log('  请修复后再打 tag 发版。')
  process.exit(1)
} else {
  console.log(`✓ 预检通过：${passed} 项通过，${warned} 项警告`)
  console.log(`\n  可以打 tag 发版：`)
  console.log(`    git tag v${targetVer}`)
  console.log(`    git push origin v${targetVer}`)
}
