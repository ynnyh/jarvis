#!/usr/bin/env node
/**
 * 把 build matrix 各平台的产物推到 Gitee Releases，
 * 同时把 latest.json 写到 Gitee 仓库 main 分支根目录（供 tauri-plugin-updater 拉取）。
 *
 * 输入：
 *   ARTIFACTS_DIR        必填，目录里每个子目录是一个 actions artifact（每个 artifact 来自一个 platform 的 build job）
 *                        子目录结构：{某 artifact 名}/{安装包} + {签名文件} + PLATFORM_ID
 *   GITEE_TOKEN          必填，Gitee 私人访问令牌
 *   GITEE_OWNER          可选，默认 ynnyh
 *   GITEE_REPO           可选，默认 jarvis
 *   RELEASE_NOTES        可选
 *
 * 行为：
 *   - 扫所有子目录，按 PLATFORM_ID 区分平台
 *   - 安装包文件 = 任意非 .sig 非 PLATFORM_ID 文件；签名 = 同名 .sig
 *   - 全部上传到 release，写一个含所有平台的 latest.json
 *
 * 兼容旧调用：未给 ARTIFACTS_DIR 时回退到只扫 src-tauri/target/release/bundle/nsis（保留本机一键发布能力）。
 */

import fs from 'node:fs/promises'
import { readFileSync, existsSync, readdirSync, statSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { Agent, setGlobalDispatcher } from 'undici'

// Gitee 上传大文件（macOS .app.tar.gz + .dmg 加起来 ~70MB）从 GH Actions
// 上行 + 服务端处理可能超过 5 分钟，需把超时拉长
setGlobalDispatcher(new Agent({
  headersTimeout: 30 * 60 * 1000,
  bodyTimeout: 30 * 60 * 1000,
  connectTimeout: 60 * 1000,
}))

const __filename = fileURLToPath(import.meta.url)
const __dirname = path.dirname(__filename)
const repoRoot = path.resolve(__dirname, '..')

const GITEE_OWNER = process.env.GITEE_OWNER || 'ynnyh'
const GITEE_REPO = process.env.GITEE_REPO || 'jarvis'
const TOKEN = process.env.GITEE_TOKEN
const API = 'https://gitee.com/api/v5'
const ARTIFACTS_DIR = process.env.ARTIFACTS_DIR

if (!TOKEN) {
  console.error('❌ 缺少 GITEE_TOKEN 环境变量')
  process.exit(1)
}

// --- 1. 读 tauri.conf.json 拿版本号 ---
const tauriConfPath = path.join(repoRoot, 'src-tauri/tauri.conf.json')
const tauriConf = JSON.parse(readFileSync(tauriConfPath, 'utf8'))
const version = tauriConf.version
const tag = `v${version}`
console.log(`发布版本：${tag}`)

// --- 2. 收集所有平台的产物 ---
/** @type {Array<{platformId: string, installerPath: string, installerName: string, sigPath: string}>} */
const platforms = []

if (ARTIFACTS_DIR && existsSync(ARTIFACTS_DIR)) {
  // CI 路径：每个子目录是一个 platform 的 artifact
  // 一个 platform 可能有多个文件：
  //   Windows: *-setup.exe + *-setup.exe.sig
  //   macOS:   *.dmg + *.app.tar.gz + *.app.tar.gz.sig
  // 约定：以 .sig 结尾的文件就是签名；签名去掉 .sig 后缀对应的就是 updater target。
  // 其余文件作为"额外发行物"上传（如 macOS 的 .dmg 给用户下载装）。
  const dirs = readdirSync(ARTIFACTS_DIR).filter(d => statSync(path.join(ARTIFACTS_DIR, d)).isDirectory())
  for (const d of dirs) {
    const sub = path.join(ARTIFACTS_DIR, d)
    const platformIdFile = path.join(sub, 'PLATFORM_ID')
    if (!existsSync(platformIdFile)) {
      console.warn(`  ⚠ ${d} 缺 PLATFORM_ID，跳过`)
      continue
    }
    const platformId = readFileSync(platformIdFile, 'utf8').trim()
    const all = readdirSync(sub).filter(f => f !== 'PLATFORM_ID')
    const sig = all.find(f => f.endsWith('.sig'))
    if (!sig) {
      console.warn(`  ⚠ ${d} 缺 .sig 文件，跳过`)
      continue
    }
    const updaterTarget = sig.replace(/\.sig$/, '')
    if (!all.includes(updaterTarget)) {
      console.warn(`  ⚠ ${d} 有 ${sig} 但找不到 ${updaterTarget}，跳过`)
      continue
    }
    const extras = all.filter(f => f !== sig && f !== updaterTarget)
    platforms.push({
      platformId,
      updaterPath: path.join(sub, updaterTarget),
      updaterName: updaterTarget,
      sigPath: path.join(sub, sig),
      sigName: sig,
      extras: extras.map(name => ({ path: path.join(sub, name), name })),
    })
  }
} else {
  // 本机回退：自动扫当前机器已经打出来的 Windows / macOS 包。
  // Windows 和 Mac 可以分开上传，同版本 latest.json 会自动合并已有平台。
  const winBundleDir = path.join(repoRoot, 'src-tauri/target/release/bundle/nsis')
  if (existsSync(winBundleDir)) {
    const files = readdirSync(winBundleDir)
    const installer = files.find(f => f.endsWith('-setup.exe'))
    const sig = files.find(f => f.endsWith('-setup.exe.sig'))
    if (installer && sig) {
      platforms.push({
        platformId: 'windows-x86_64',
        updaterPath: path.join(winBundleDir, installer),
        updaterName: installer,
        sigPath: path.join(winBundleDir, sig),
        sigName: sig,
        extras: [],
      })
    }
  }

  const macBundleRoots = [
    path.join(repoRoot, 'src-tauri/target/universal-apple-darwin/release/bundle'),
    path.join(repoRoot, 'src-tauri/target/release/bundle'),
  ]
  for (const macBundleRoot of macBundleRoots) {
    if (!existsSync(macBundleRoot)) continue
    const all = walkFiles(macBundleRoot)
    const sigPath = all.find(f => f.endsWith('.sig'))
    if (!sigPath) continue
    const updaterPath = sigPath.replace(/\.sig$/, '')
    if (!existsSync(updaterPath)) continue
    const dmgPath = all.find(f => f.endsWith('.dmg'))
    platforms.push({
      platformId: 'darwin-aarch64,darwin-x86_64',
      updaterPath,
      updaterName: path.basename(updaterPath),
      sigPath,
      sigName: path.basename(sigPath),
      extras: dmgPath ? [{ path: dmgPath, name: path.basename(dmgPath) }] : [],
    })
    break
  }
}

if (platforms.length === 0) {
  console.error('❌ 没扫到任何平台产物')
  process.exit(1)
}

console.log(`扫到 ${platforms.length} 个平台产物：${platforms.map(p => p.platformId).join(', ')}`)

function walkFiles(root) {
  const out = []
  for (const name of readdirSync(root)) {
    const full = path.join(root, name)
    const stat = statSync(full)
    if (stat.isDirectory()) out.push(...walkFiles(full))
    else out.push(full)
  }
  return out
}

// --- 3. 创建（或复用）Release ---
async function createOrFindRelease() {
  const res = await fetch(`${API}/repos/${GITEE_OWNER}/${GITEE_REPO}/releases`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      access_token: TOKEN,
      tag_name: tag,
      name: tag,
      body: process.env.RELEASE_NOTES || `Jarvis ${tag}`,
      prerelease: false,
      target_commitish: 'main',
    }),
  })

  if (res.ok) {
    const j = await res.json()
    console.log(`✓ 创建 release ${tag} (id=${j.id})`)
    return j
  }

  const text = await res.text()
  if (res.status === 422 || text.includes('已存在') || text.includes('exist')) {
    const r2 = await fetch(
      `${API}/repos/${GITEE_OWNER}/${GITEE_REPO}/releases/tags/${tag}?access_token=${TOKEN}`,
    )
    if (!r2.ok) throw new Error(`查询已有 release 失败：${r2.status} ${await r2.text()}`)
    const j = await r2.json()
    console.log(`✓ 复用已有 release ${tag} (id=${j.id})`)
    return j
  }
  throw new Error(`创建 release 失败：${res.status} ${text}`)
}

const release = await createOrFindRelease()

// --- 4. 上传附件 ---
async function uploadAsset(filePath, name) {
  const data = await fs.readFile(filePath)
  const maxAttempts = 3
  let lastErr
  for (let attempt = 1; attempt <= maxAttempts; attempt++) {
    try {
      const form = new FormData()
      form.append('access_token', TOKEN)
      form.append('file', new Blob([data]), name)
      const r = await fetch(
        `${API}/repos/${GITEE_OWNER}/${GITEE_REPO}/releases/${release.id}/attach_files`,
        { method: 'POST', body: form },
      )
      if (!r.ok) {
        const t = await r.text()
        if (t.includes('已存在') || t.includes('exist')) {
          console.log(`  ↪ ${name} 已存在，跳过上传`)
          return `https://gitee.com/${GITEE_OWNER}/${GITEE_REPO}/releases/download/${tag}/${name}`
        }
        // 5xx / 429 值得重试，4xx 直接报
        if (r.status >= 500 || r.status === 429) {
          throw new Error(`HTTP ${r.status}: ${t}`)
        }
        throw new Error(`上传 ${name} 失败：${r.status} ${t}`)
      }
      const j = await r.json()
      console.log(`  ↪ 上传 ${name}: ${j.browser_download_url}`)
      return j.browser_download_url
    } catch (e) {
      lastErr = e
      const isTimeout =
        e?.cause?.code === 'UND_ERR_HEADERS_TIMEOUT' ||
        e?.cause?.code === 'UND_ERR_BODY_TIMEOUT' ||
        e?.cause?.code === 'UND_ERR_CONNECT_TIMEOUT' ||
        e?.cause?.code === 'ECONNRESET' ||
        e?.code === 'ECONNRESET' ||
        /HTTP 5\d\d/.test(String(e?.message)) ||
        /HTTP 429/.test(String(e?.message))
      if (attempt < maxAttempts && isTimeout) {
        const backoff = 5000 * attempt
        console.warn(`  ⚠ ${name} 第 ${attempt}/${maxAttempts} 次失败（${e?.cause?.code || e?.code || e?.message}），${backoff}ms 后重试`)
        await new Promise(r => setTimeout(r, backoff))
        continue
      }
      throw e
    }
  }
  throw lastErr
}

// 并发上传：每个文件独立 connection，Gitee 单连接慢但多连接可压一半时间
// 失败重试逻辑在 uploadAsset 内部，Promise.all 失败 fail-fast
const platformEntries = {}
const uploadJobs = []

const macDevInstaller = path.join(repoRoot, 'scripts/install-macos-dev.sh')
if (existsSync(macDevInstaller)) {
  uploadJobs.push(uploadAsset(macDevInstaller, 'install-macos-dev.sh'))
}

for (const p of platforms) {
  const fileCount = 1 + 1 + p.extras.length
  console.log(`→ ${p.platformId}: 排队 ${fileCount} 个上传`)
  const platformIds = p.platformId
    .split(',')
    .map(id => id.trim())
    .filter(Boolean)
  for (const platformId of platformIds) {
    platformEntries[platformId] = {
      signature: readFileSync(p.sigPath, 'utf8').trim(),
      url: null, // 待 updater 上传完填
    }
  }
  // updater target（latest.json 的 url 指向它）
  uploadJobs.push(
    uploadAsset(p.updaterPath, p.updaterName).then(url => {
      for (const platformId of platformIds) {
        platformEntries[platformId].url = url
      }
    }),
  )
  // sig
  uploadJobs.push(uploadAsset(p.sigPath, p.sigName))
  // 额外发行物（macOS .dmg 等）
  for (const ex of p.extras) {
    uploadJobs.push(uploadAsset(ex.path, ex.name))
  }
}

console.log(`\n并发上传 ${uploadJobs.length} 个文件...`)
const t0 = Date.now()
await Promise.all(uploadJobs)
console.log(`✓ 全部上传完成（${((Date.now() - t0) / 1000).toFixed(1)}s）`)

// --- 5. 写 latest.json ---
// notes 优先级：CHANGELOG.md 当前版本节 > RELEASE_NOTES env > 兜底占位。
// CHANGELOG 节锚点是 "## ${tag}"（如 "## v0.6.3"），body 直到下一个 "## " 或文末。
function extractChangelogNotes(targetTag) {
  const changelogPath = path.join(repoRoot, 'CHANGELOG.md')
  if (!existsSync(changelogPath)) return null
  const text = readFileSync(changelogPath, 'utf8')
  const header = `## ${targetTag}`
  const lines = text.split('\n')
  let i = lines.findIndex(l => l.trim() === header || l.trim().startsWith(header + ' '))
  if (i < 0) return null
  i++
  const body = []
  while (i < lines.length) {
    if (lines[i].startsWith('## ')) break
    body.push(lines[i])
    i++
  }
  while (body.length && !body[0].trim()) body.shift()
  while (body.length && !body[body.length - 1].trim()) body.pop()
  return body.length ? body.join('\n') : null
}
const changelogNotes = extractChangelogNotes(tag)
if (changelogNotes) {
  console.log(`✓ 已从 CHANGELOG.md 抽到 ${tag} 的更新说明（${changelogNotes.length} 字符）`)
} else {
  console.log(`⚠ CHANGELOG.md 没找到 "## ${tag}" 节，回退到 RELEASE_NOTES env`)
}

const latest = {
  version,
  notes: changelogNotes || process.env.RELEASE_NOTES || `Jarvis ${tag}`,
  pub_date: new Date().toISOString(),
  platforms: platformEntries,
}

async function getExistingFile(filePath) {
  const r = await fetch(
    `${API}/repos/${GITEE_OWNER}/${GITEE_REPO}/contents/${filePath}?access_token=${TOKEN}&ref=main`,
  )
  if (r.status === 404) return null
  if (!r.ok) throw new Error(`查询 ${filePath} 失败：${r.status} ${await r.text()}`)
  const j = await r.json()
  let text = ''
  if (j.content) {
    text = Buffer.from(String(j.content).replace(/\s/g, ''), 'base64').toString('utf8')
  }
  return { sha: j.sha, text }
}

const existingLatest = await getExistingFile('latest.json')
if (existingLatest?.text) {
  try {
    const parsed = JSON.parse(existingLatest.text)
    if (parsed?.version === version && parsed?.platforms && typeof parsed.platforms === 'object') {
      latest.platforms = {
        ...parsed.platforms,
        ...platformEntries,
      }
      console.log(`✓ 合并已有 latest.json 平台：${Object.keys(parsed.platforms).join(', ')}`)
    }
  } catch (e) {
    console.warn(`⚠ 解析已有 latest.json 失败，将覆盖写入：${e?.message || e}`)
  }
}

const latestStr = JSON.stringify(latest, null, 2)
const latestB64 = Buffer.from(latestStr, 'utf8').toString('base64')

const sha = existingLatest?.sha ?? null
const method = sha ? 'PUT' : 'POST'
const r = await fetch(
  `${API}/repos/${GITEE_OWNER}/${GITEE_REPO}/contents/latest.json`,
  {
    method,
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      access_token: TOKEN,
      content: latestB64,
      message: `release: ${tag}`,
      branch: 'main',
      ...(sha ? { sha } : {}),
    }),
  },
)
if (!r.ok) {
  console.error(`❌ 更新 latest.json 失败：${r.status} ${await r.text()}`)
  console.error('   如果是 404：先在 Gitee 网页给仓库初始化 main 分支（建个 README 即可）')
  process.exit(1)
}
console.log(`\n✓ latest.json 已发布（${platforms.length} 个平台）：`)
console.log(`   https://gitee.com/${GITEE_OWNER}/${GITEE_REPO}/raw/main/latest.json`)

console.log(`\n🎉 ${tag} 全部发布完成`)
