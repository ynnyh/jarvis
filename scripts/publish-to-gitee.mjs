#!/usr/bin/env node
/**
 * 把 build matrix 各平台的产物推到 Gitee Releases + GitHub Releases，
 * 同时把 latest.json 写到 Gitee 仓库 main 分支根目录（供 tauri-plugin-updater 拉取）。
 *
 * 发布策略（双保险）：
 *   1. 先上传所有产物到 GitHub Release（同区域，快且稳）
 *   2. 再上传到 Gitee Release（跨太平洋，可能超时）
 *   3. latest.json 的下载 URL 优先用 Gitee；Gitee 失败时回退到 GitHub URL
 *
 * 输入：
 *   ARTIFACTS_DIR        必填，目录里每个子目录是一个 actions artifact（每个 artifact 来自一个 platform 的 build job）
 *                        子目录结构：{某 artifact 名}/{安装包} + {签名文件} + PLATFORM_ID
 *   GITEE_TOKEN          必填，Gitee 私人访问令牌
 *   GITEE_OWNER          可选，默认 ynnyh
 *   GITEE_REPO           可选，默认 jarvis
 *   GITHUB_TOKEN         可选，GitHub Token（用于 Release 备份）
 *   GITHUB_REPO          可选，GitHub 仓库（owner/repo）
 *   RELEASE_NOTES        可选
 *
 * 行为：
 *   - 扫所有子目录，按 PLATFORM_ID 区分平台
 *   - 安装包文件 = 任意非 .sig 非 PLATFORM_ID 文件；签名 = 同名 .sig
 *   - 全部上传到 GitHub Release + Gitee Release，写一个含所有平台的 latest.json
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
  connectTimeout: 30 * 1000,
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

const GITHUB_TOKEN = process.env.GITHUB_TOKEN
const GITHUB_REPO = process.env.GITHUB_REPO

// --- 0. --sync-from-github 模式：从 GitHub Release 同步附件到 Gitee ---
if (process.argv.includes('--sync-from-github')) {
  if (!GITHUB_TOKEN || !GITHUB_REPO) {
    console.error('❌ sync 模式需要 GITHUB_TOKEN + GITHUB_REPO 环境变量')
    process.exit(1)
  }

  const tauriConfPath = path.join(repoRoot, 'src-tauri/tauri.conf.json')
  const tauriConf = JSON.parse(readFileSync(tauriConfPath, 'utf8'))
  const syncTag = `v${tauriConf.version}`
  console.log(`🔄 从 GitHub Release 同步 ${syncTag} 到 Gitee...`)

  // 1. 读 GitHub Release assets
  const ghRes = await fetch(
    `https://api.github.com/repos/${GITHUB_REPO}/releases/tags/${syncTag}`,
    { headers: { Authorization: `token ${GITHUB_TOKEN}`, 'User-Agent': 'jarvis-ci' } },
  )
  if (!ghRes.ok) {
    console.error(`❌ GitHub Release ${syncTag} 不存在：${ghRes.status}`)
    process.exit(1)
  }
  const ghRelease = await ghRes.json()
  const assets = ghRelease.assets || []
  console.log(`  找到 ${assets.length} 个 GitHub 附件`)

  // 2. 创建/复用 Gitee Release
  const giteeRes = await fetch(`${API}/repos/${GITEE_OWNER}/${GITEE_REPO}/releases`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      access_token: TOKEN,
      tag_name: syncTag,
      name: syncTag,
      body: ghRelease.body || `Jarvis ${syncTag}`,
      prerelease: false,
      target_commitish: 'main',
    }),
  })
  let giteeRelease
  if (giteeRes.ok) {
    giteeRelease = await giteeRes.json()
    console.log(`✓ 创建 Gitee release ${syncTag} (id=${giteeRelease.id})`)
  } else {
    const t = await giteeRes.text()
    if (t.includes('已存在') || t.includes('exist') || t.includes('标签已经存在') || t.includes('tag_name has already been taken')) {
      const r2 = await fetch(`${API}/repos/${GITEE_OWNER}/${GITEE_REPO}/releases/tags/${syncTag}?access_token=${TOKEN}`)
      if (!r2.ok) throw new Error(`查询 Gitee release 失败：${r2.status}`)
      giteeRelease = await r2.json()
      console.log(`✓ 复用已有 Gitee release ${syncTag} (id=${giteeRelease.id})`)
    } else {
      throw new Error(`创建 Gitee release 失败：${giteeRes.status} ${t}`)
    }
  }

  // 3. 下载 GitHub 附件 → 上传到 Gitee
  for (const asset of assets) {
    console.log(`  ↓ 下载 ${asset.name} (${(asset.size / 1024 / 1024).toFixed(1)}MB)...`)
    const dlRes = await fetch(asset.browser_download_url)
    if (!dlRes.ok) {
      console.warn(`  ⚠ 下载 ${asset.name} 失败：${dlRes.status}，跳过`)
      continue
    }
    const data = Buffer.from(await dlRes.arrayBuffer())

    // 删除 Gitee 上已有的同名附件
    const listUrl = `${API}/repos/${GITEE_OWNER}/${GITEE_REPO}/releases/${giteeRelease.id}/attach_files?access_token=${TOKEN}`
    const listRes = await fetch(listUrl)
    if (listRes.ok) {
      const existing = (await listRes.json()).find(a => a.name === asset.name)
      if (existing) {
        await fetch(`${API}/repos/${GITEE_OWNER}/${GITEE_REPO}/releases/${giteeRelease.id}/attach_files/${existing.id}?access_token=${TOKEN}`, { method: 'DELETE' })
        console.log(`  🗑 删除旧 ${asset.name}`)
      }
    }

    const form = new FormData()
    form.append('access_token', TOKEN)
    form.append('file', new Blob([data]), asset.name)
    const upRes = await fetch(
      `${API}/repos/${GITEE_OWNER}/${GITEE_REPO}/releases/${giteeRelease.id}/attach_files`,
      { method: 'POST', body: form },
    )
    if (upRes.ok) {
      console.log(`  ✓ 上传 ${asset.name} 到 Gitee`)
    } else {
      console.warn(`  ⚠ 上传 ${asset.name} 失败：${upRes.status} ${await upRes.text()}`)
    }
  }

  console.log(`\n🎉 ${syncTag} 同步完成`)
  process.exit(0)
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
    const extras = all.filter(f => f !== sig && f !== updaterTarget && !f.startsWith('.'))
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

// --- 2b. 从 CHANGELOG.md 提取中文更新说明 ---
// notes 优先级：CHANGELOG.md 当前版本节 > RELEASE_NOTES env > 兜底
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
const releaseNotes = extractChangelogNotes(tag)
if (releaseNotes) {
  console.log(`✓ 已从 CHANGELOG.md 抽到 ${tag} 的中文更新说明（${releaseNotes.length} 字符）`)
} else {
  console.log(`⚠ CHANGELOG.md 没找到 "## ${tag}" 节，回退到 RELEASE_NOTES env`)
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
      body: releaseNotes || process.env.RELEASE_NOTES || `Jarvis ${tag}`,
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
  if (
    res.status === 422 ||
    text.includes('已存在') ||
    text.includes('exist') ||
    text.includes('标签已经存在发行版') ||
    text.includes('tag_name has already been taken') ||
    text.includes('验证错误')
  ) {
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

/** 删除 release 上已存在的同名附件（防重跑时 sig/exe 不一致） */
async function deleteExistingAsset(name) {
  const listUrl = `${API}/repos/${GITEE_OWNER}/${GITEE_REPO}/releases/${release.id}/attach_files?access_token=${TOKEN}`
  const r = await fetch(listUrl)
  if (!r.ok) return
  const assets = await r.json()
  const existing = (Array.isArray(assets) ? assets : []).find(a => a.name === name)
  if (!existing) return
  const del = await fetch(
    `${API}/repos/${GITEE_OWNER}/${GITEE_REPO}/releases/${release.id}/attach_files/${existing.id}?access_token=${TOKEN}`,
    { method: 'DELETE' },
  )
  if (del.ok) {
    console.log(`  🗑 已删除旧 ${name} (id=${existing.id})，将重新上传`)
  } else {
    console.warn(`  ⚠ 删除旧 ${name} 失败：${del.status} ${await del.text()}`)
  }
}

async function uploadAsset(filePath, name, { optional = false } = {}) {
  const stat = await fs.stat(filePath)
  const fileSizeMB = stat.size / (1024 * 1024)
  const data = await fs.readFile(filePath)
  const isLargeAsset = fileSizeMB > 8
  const maxAttempts = isLargeAsset ? 8 : 5
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
          console.log(`  ↪ ${name} (${fileSizeMB.toFixed(1)}MB) 已存在，先删除再重新上传`)
          await deleteExistingAsset(name)
          // 重试上传（不计入 attempt 次数，因为这是删除后的重传）
          const retryForm = new FormData()
          retryForm.append('access_token', TOKEN)
          retryForm.append('file', new Blob([data]), name)
          const retry = await fetch(
            `${API}/repos/${GITEE_OWNER}/${GITEE_REPO}/releases/${release.id}/attach_files`,
            { method: 'POST', body: retryForm },
          )
          if (retry.ok) {
            const j = await retry.json()
            console.log(`  ✓ 重新上传 ${name} (${fileSizeMB.toFixed(1)}MB): ${j.browser_download_url}`)
            return j.browser_download_url
          }
          const rt = await retry.text()
          throw new Error(`重新上传 ${name} 失败：${retry.status} ${rt}`)
        }
        if (r.status >= 500 || r.status === 429) {
          throw new Error(`HTTP ${r.status}: ${t}`)
        }
        throw new Error(`上传 ${name} 失败：${r.status} ${t}`)
      }
      const j = await r.json()
      console.log(`  ✓ 上传 ${name} (${fileSizeMB.toFixed(1)}MB): ${j.browser_download_url}`)
      return j.browser_download_url
    } catch (e) {
      lastErr = e
      const errCode = e?.cause?.code || e?.code
      const isRetryable =
        errCode === 'UND_ERR_HEADERS_TIMEOUT' ||
        errCode === 'UND_ERR_BODY_TIMEOUT' ||
        errCode === 'UND_ERR_CONNECT_TIMEOUT' ||
        errCode === 'ETIMEDOUT' ||
        errCode === 'ECONNRESET' ||
        errCode === 'ECONNREFUSED' ||
        errCode === 'ENOTFOUND' ||
        /HTTP 5\d\d/.test(String(e?.message)) ||
        /HTTP 429/.test(String(e?.message))
      if (attempt < maxAttempts && isRetryable) {
        const baseBackoff = isLargeAsset ? 10000 : fileSizeMB > 30 ? 8000 : 3000
        const backoff = baseBackoff * attempt
        console.warn(`  ⚠ ${name} (${fileSizeMB.toFixed(1)}MB) 第 ${attempt}/${maxAttempts} 次失败（${errCode || e?.message}），${(backoff / 1000).toFixed(0)}s 后重试`)
        await new Promise(r => setTimeout(r, backoff))
        continue
      }
      if (optional) {
        console.warn(`  ⚠ ${name} 上传失败（可选文件，跳过）：${e?.message || e}`)
        return null
      }
      throw e
    }
  }
  if (optional) {
    console.warn(`  ⚠ ${name} ${maxAttempts} 次全部失败（可选文件，跳过）`)
    return null
  }
  throw lastErr
}

// --- 4a. 先上传所有产物到 GitHub Release 保底 ---
// GitHub Actions (US) → GitHub API (US) 同区域，速度快且稳定。
// 先确保产物安全落盘，再尝试 Gitee（跨太平洋，可能超时）。
const ghUrlByName = {} // filename → GitHub download URL，用于 Gitee 失败时回退

async function createOrFindGitHubRelease() {
  const ghCheckRes = await fetch(
    `https://api.github.com/repos/${GITHUB_REPO}/releases/tags/${tag}`,
    { headers: { Authorization: `token ${GITHUB_TOKEN}`, 'User-Agent': 'jarvis-ci' } },
  )
  if (ghCheckRes.ok) {
    const j = await ghCheckRes.json()
    console.log(`✓ 复用已有 GitHub release ${tag} (id=${j.id})`)
    return j
  }
  const ghCreateRes = await fetch(
    `https://api.github.com/repos/${GITHUB_REPO}/releases`,
    {
      method: 'POST',
      headers: {
        Authorization: `token ${GITHUB_TOKEN}`,
        'User-Agent': 'jarvis-ci',
        'Content-Type': 'application/json',
      },
      body: JSON.stringify({
        tag_name: tag,
        name: tag,
        body: releaseNotes || process.env.RELEASE_NOTES || `Jarvis ${tag}`,
        prerelease: false,
        target_commitish: 'main',
      }),
    },
  )
  if (!ghCreateRes.ok) throw new Error(`创建 GitHub release 失败：${ghCreateRes.status} ${await ghCreateRes.text()}`)
  const j = await ghCreateRes.json()
  console.log(`✓ 创建 GitHub release ${tag} (id=${j.id})`)
  return j
}

async function uploadToGitHub(filePath, name, ghRelease) {
  const data = await fs.readFile(filePath)
  const fileSizeMB = data.length / (1024 * 1024)
  const uploadUrl = ghRelease.upload_url.replace(/\{.*\}/, '')
  const r = await fetch(`${uploadUrl}?name=${name}`, {
    method: 'POST',
    headers: {
      Authorization: `token ${GITHUB_TOKEN}`,
      'User-Agent': 'jarvis-ci',
      'Content-Type': 'application/octet-stream',
      'Content-Length': String(data.length),
    },
    body: data,
  })
  if (!r.ok) throw new Error(`GitHub 上传 ${name} 失败：${r.status} ${await r.text()}`)
  const j = await r.json()
  console.log(`  ✓ GitHub ${name} (${fileSizeMB.toFixed(1)}MB)`)
  ghUrlByName[name] = j.browser_download_url
  return j.browser_download_url
}

let githubSucceeded = false
if (GITHUB_TOKEN && GITHUB_REPO) {
  console.log(`\n--- Step 1/2: 上传所有产物到 GitHub Release ---`)
  try {
    const ghRelease = await createOrFindGitHubRelease()
    const ghUploadStart = Date.now()
    for (const p of platforms) {
      await uploadToGitHub(p.sigPath, p.sigName, ghRelease)
      await uploadToGitHub(p.updaterPath, p.updaterName, ghRelease)
      for (const ex of p.extras) {
        await uploadToGitHub(ex.path, ex.name, ghRelease)
      }
    }
    const macDevInstaller = path.join(repoRoot, 'scripts/install-macos-dev.sh')
    if (existsSync(macDevInstaller)) {
      await uploadToGitHub(macDevInstaller, 'install-macos-dev.sh', ghRelease)
    }
    console.log(`✓ GitHub 全部上传完成（${((Date.now() - ghUploadStart) / 1000).toFixed(1)}s）`)
    githubSucceeded = true
  } catch (e) {
    console.warn(`⚠ GitHub Release 上传失败：${e?.message || e}`)
  }
} else {
  console.log(`\n⚠ 未配置 GITHUB_TOKEN / GITHUB_REPO，跳过 GitHub Release 备份`)
}

// --- 4b. 上传到 Gitee Release ---
// latest.json 的下载 URL 优先指向 Gitee（国内用户可达）。
// 如果 Gitee 上传失败但 GitHub 成功，用 GitHub URL 回退。
const platformEntries = {}
const uploadQueue = []

for (const p of platforms) {
  const fileCount = 1 + 1 + p.extras.length
  console.log(`→ ${p.platformId}: ${fileCount} 个文件待上传`)
  const platformIds = p.platformId
    .split(',')
    .map(id => id.trim())
    .filter(Boolean)
  for (const platformId of platformIds) {
    platformEntries[platformId] = {
      signature: readFileSync(p.sigPath, 'utf8').trim(),
      url: null,
    }
  }
  uploadQueue.push({
    filePath: p.sigPath,
    name: p.sigName,
    priority: 1,
    optional: false,
    onComplete: () => {},
  })
  uploadQueue.push({
    filePath: p.updaterPath,
    name: p.updaterName,
    priority: 2,
    optional: false,
    onComplete: (url) => {
      for (const platformId of platformIds) {
        platformEntries[platformId].url = url
      }
    },
  })
  for (const ex of p.extras) {
    uploadQueue.push({
      filePath: ex.path,
      name: ex.name,
      priority: 3,
      optional: true,
      onComplete: () => {},
    })
  }
}

const macDevInstaller = path.join(repoRoot, 'scripts/install-macos-dev.sh')
if (existsSync(macDevInstaller)) {
  uploadQueue.push({
    filePath: macDevInstaller,
    name: 'install-macos-dev.sh',
    priority: 2,
    optional: true,
    onComplete: () => {},
  })
}

uploadQueue.sort((a, b) => a.priority - b.priority)

console.log(`\n--- Step 2/2: 上传到 Gitee Release ---`)
console.log(`上传 ${uploadQueue.length} 个文件（同优先级并发=2）...`)
const t0 = Date.now()

const groups = new Map()
for (const job of uploadQueue) {
  if (!groups.has(job.priority)) groups.set(job.priority, [])
  groups.get(job.priority).push(job)
}
const sortedGroups = [...groups.entries()].sort((a, b) => a[0] - b[0])

let giteeSucceeded = true
try {
  for (const [, jobs] of sortedGroups) {
    let idx = 0
    const CONCURRENCY = 2
    await Promise.all(
      Array.from({ length: Math.min(CONCURRENCY, jobs.length) }, async () => {
        while (idx < jobs.length) {
          const job = jobs[idx++]
          const url = await uploadAsset(job.filePath, job.name, { optional: job.optional })
          job.onComplete(url)
        }
      }),
    )
  }
  console.log(`✓ Gitee 全部上传完成（${((Date.now() - t0) / 1000).toFixed(1)}s）`)
} catch (e) {
  giteeSucceeded = false
  if (githubSucceeded) {
    console.error(`\n⚠ Gitee 上传失败（产物已在 GitHub Release 备份，可手动补传）：${e?.message || e}`)
    // 用 GitHub URL 回退 null 的 platformEntries
    for (const p of platforms) {
      const ghUrl = ghUrlByName[p.updaterName]
      if (ghUrl) {
        const platformIds = p.platformId.split(',').map(id => id.trim()).filter(Boolean)
        for (const pid of platformIds) {
          if (!platformEntries[pid].url) {
            platformEntries[pid].url = ghUrl
            console.log(`  ↪ ${pid} 回退到 GitHub URL`)
          }
        }
      }
    }
  } else {
    console.error(`\n❌ Gitee 上传失败且 GitHub 也未成功：${e?.message || e}`)
    process.exit(1)
  }
}

// --- 5. 写 latest.json ---
const latest = {
  version,
  notes: releaseNotes || process.env.RELEASE_NOTES || `Jarvis ${tag}`,
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
if (githubSucceeded) console.log(`   ✓ GitHub Release: https://github.com/${GITHUB_REPO}/releases/tag/${tag}`)
if (giteeSucceeded) console.log(`   ✓ Gitee Release:  https://gitee.com/${GITEE_OWNER}/${GITEE_REPO}/releases/tag/${tag}`)
else console.log(`   ⚠ Gitee Release 上传失败，可从 GitHub 手动下载后补传`)
