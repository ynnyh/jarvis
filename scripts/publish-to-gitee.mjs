#!/usr/bin/env node
/**
 * 把刚 build 出来的 NSIS 安装包 + updater 产物推到 Gitee Releases，
 * 同时把 latest.json 写到 Gitee 仓库 main 分支根目录（供 tauri-plugin-updater 拉取）。
 *
 * 用法：
 *   GITEE_TOKEN=xxx GITEE_OWNER=ynnyh GITEE_REPO=jarvis \
 *     node scripts/publish-to-gitee.mjs
 *
 * 环境变量：
 *   GITEE_TOKEN     必填，Gitee 私人访问令牌（projects 权限）
 *   GITEE_OWNER     可选，默认 ynnyh
 *   GITEE_REPO      可选，默认 jarvis
 *   RELEASE_NOTES   可选，写进 release body + latest.json.notes，默认 "Jarvis vX.Y.Z"
 *
 * 工作流（Actions）里把这三个 env 注入即可。
 */

import fs from 'node:fs/promises'
import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const __filename = fileURLToPath(import.meta.url)
const __dirname = path.dirname(__filename)
const repoRoot = path.resolve(__dirname, '..')

const GITEE_OWNER = process.env.GITEE_OWNER || 'ynnyh'
const GITEE_REPO = process.env.GITEE_REPO || 'jarvis'
const TOKEN = process.env.GITEE_TOKEN
const API = 'https://gitee.com/api/v5'

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

// --- 2. 定位 NSIS 产物 ---
const bundleDir = path.join(repoRoot, 'src-tauri/target/release/bundle/nsis')
let files
try {
  files = await fs.readdir(bundleDir)
} catch {
  console.error(`❌ 找不到 ${bundleDir}，确认已跑 tauri build`)
  process.exit(1)
}

const setupExe = files.find(f => f.endsWith('-setup.exe'))
const setupExeSig = files.find(f => f.endsWith('-setup.exe.sig'))

if (!setupExe || !setupExeSig) {
  console.error('❌ 未找到 NSIS 安装包 / 签名文件')
  console.error('   确认 tauri.conf.json 里 bundle.createUpdaterArtifacts = true')
  console.error('   并且签名环境变量 TAURI_SIGNING_PRIVATE_KEY[_PASSWORD] 已设置')
  console.error(`   bundleDir 实际内容：${files.join(', ')}`)
  process.exit(1)
}

const signature = readFileSync(path.join(bundleDir, setupExeSig), 'utf8').trim()

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
  // 已存在 → 查回来复用
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
  const form = new FormData()
  form.append('access_token', TOKEN)
  form.append('file', new Blob([data]), name)
  const r = await fetch(
    `${API}/repos/${GITEE_OWNER}/${GITEE_REPO}/releases/${release.id}/attach_files`,
    { method: 'POST', body: form },
  )
  if (!r.ok) {
    const t = await r.text()
    // 同名已存在 → 视为成功，构造 URL（Gitee release 附件 URL 是稳定的）
    if (t.includes('已存在') || t.includes('exist')) {
      console.log(`  ↪ ${name} 已存在，跳过上传`)
      // 用约定 URL（与 Gitee 实际返回一致）
      return `https://gitee.com/${GITEE_OWNER}/${GITEE_REPO}/releases/download/${tag}/${name}`
    }
    throw new Error(`上传 ${name} 失败：${r.status} ${t}`)
  }
  const j = await r.json()
  console.log(`  ↪ 上传 ${name}: ${j.browser_download_url}`)
  return j.browser_download_url
}

const setupExeUrl = await uploadAsset(path.join(bundleDir, setupExe), setupExe)
await uploadAsset(path.join(bundleDir, setupExeSig), setupExeSig)

// --- 5. 写 latest.json 到仓库 main 分支根目录 ---
const latest = {
  version,
  notes: process.env.RELEASE_NOTES || `Jarvis ${tag}`,
  pub_date: new Date().toISOString(),
  platforms: {
    'windows-x86_64': {
      signature,
      url: setupExeUrl,
    },
  },
}
const latestStr = JSON.stringify(latest, null, 2)
const latestB64 = Buffer.from(latestStr, 'utf8').toString('base64')

async function getFileSha(filePath) {
  const r = await fetch(
    `${API}/repos/${GITEE_OWNER}/${GITEE_REPO}/contents/${filePath}?access_token=${TOKEN}&ref=main`,
  )
  if (r.status === 404) return null
  if (!r.ok) throw new Error(`查询 ${filePath} 失败：${r.status} ${await r.text()}`)
  const j = await r.json()
  return j.sha
}

const sha = await getFileSha('latest.json')
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
console.log(`✓ latest.json 已发布：`)
console.log(`   https://gitee.com/${GITEE_OWNER}/${GITEE_REPO}/raw/main/latest.json`)

console.log(`\n🎉 ${tag} 全部发布完成`)
