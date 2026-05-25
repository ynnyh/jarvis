#!/usr/bin/env node
// 下载便携版 node 二进制到 src-tauri/bundled/{node.exe | node}
//
// 用途：让最终安装包自带 node 运行时，终端用户不需要装 Node。
// 跨平台：
//   - Windows x64 → 拉 node.exe（单文件）
//   - macOS arm64 → 拉 .tar.gz，抽出 bin/node 放进 bundled/node
//   - macOS x64   → 同上但 url 走 darwin-x64
//   - Linux x64   → 同上但 url 走 linux-x64（未充分测试，预留）
//
// 跳过条件：目标文件已存在且大小合理（>10MB）。
// 失败时给出清晰错误，让构建者手动放二进制也行。

import { fileURLToPath } from 'url'
import { dirname, resolve } from 'path'
import { mkdirSync, existsSync, statSync, createWriteStream, unlinkSync, renameSync, writeFileSync, chmodSync, readdirSync, rmSync } from 'fs'
import { pipeline } from 'stream/promises'
import { execSync } from 'child_process'
import os from 'os'
import https from 'https'

const here = dirname(fileURLToPath(import.meta.url))
const root = resolve(here, '..')
const outdir = resolve(root, 'src-tauri/bundled')

const NODE_VERSION = process.env.NODE_BUNDLE_VERSION || 'v22.11.0'

const platform = os.platform()
const arch = os.arch()

function targetSpec() {
  if (platform === 'win32' && arch === 'x64') {
    return { url: `https://nodejs.org/dist/${NODE_VERSION}/win-x64/node.exe`, kind: 'exe', outName: 'node.exe' }
  }
  if (platform === 'darwin' && (arch === 'arm64' || arch === 'aarch64')) {
    return { url: `https://nodejs.org/dist/${NODE_VERSION}/node-${NODE_VERSION}-darwin-arm64.tar.gz`, kind: 'tar', outName: 'node', inner: `node-${NODE_VERSION}-darwin-arm64/bin/node` }
  }
  if (platform === 'darwin' && arch === 'x64') {
    return { url: `https://nodejs.org/dist/${NODE_VERSION}/node-${NODE_VERSION}-darwin-x64.tar.gz`, kind: 'tar', outName: 'node', inner: `node-${NODE_VERSION}-darwin-x64/bin/node` }
  }
  if (platform === 'linux' && arch === 'x64') {
    return { url: `https://nodejs.org/dist/${NODE_VERSION}/node-${NODE_VERSION}-linux-x64.tar.gz`, kind: 'tar', outName: 'node', inner: `node-${NODE_VERSION}-linux-x64/bin/node` }
  }
  throw new Error(`unsupported platform: ${platform}-${arch}`)
}

const spec = targetSpec()
const outfile = resolve(outdir, spec.outName)

mkdirSync(outdir, { recursive: true })

if (existsSync(outfile) && statSync(outfile).size > 10 * 1024 * 1024) {
  console.log(`[fetch-node] ${spec.outName} already present (${(statSync(outfile).size / 1024 / 1024).toFixed(1)} MB), skipping`)
  writeVersionMarker()
  process.exit(0)
}

console.log(`[fetch-node] platform=${platform}-${arch}, downloading ${spec.url}`)

function download(currentUrl, dest, redirects = 0) {
  return new Promise((resolveDl, reject) => {
    if (redirects > 5) return reject(new Error('too many redirects'))
    https.get(currentUrl, (res) => {
      if (res.statusCode && res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
        res.resume()
        return download(res.headers.location, dest, redirects + 1).then(resolveDl, reject)
      }
      if (res.statusCode !== 200) {
        return reject(new Error(`download failed: HTTP ${res.statusCode}`))
      }
      const out = createWriteStream(dest)
      pipeline(res, out).then(resolveDl, reject)
    }).on('error', reject)
  })
}

try {
  if (spec.kind === 'exe') {
    const tmp = outfile + '.part'
    await download(spec.url, tmp)
    const size = statSync(tmp).size
    if (size < 10 * 1024 * 1024) {
      unlinkSync(tmp)
      throw new Error(`downloaded file too small: ${size} bytes`)
    }
    renameSync(tmp, outfile)
  } else if (spec.kind === 'tar') {
    // 拉 tar.gz 到临时位置，用系统 tar 解出 bin/node，扔进 outdir，删临时
    const tmpTar = resolve(outdir, '_node.tar.gz')
    await download(spec.url, tmpTar)
    if (statSync(tmpTar).size < 5 * 1024 * 1024) {
      unlinkSync(tmpTar)
      throw new Error('tarball too small')
    }
    const stageDir = resolve(outdir, '_node_stage')
    if (existsSync(stageDir)) rmSync(stageDir, { recursive: true, force: true })
    mkdirSync(stageDir, { recursive: true })
    // 只抽 bin/node 一个文件
    execSync(`tar -xzf "${tmpTar}" -C "${stageDir}" "${spec.inner}"`, { stdio: 'inherit' })
    const extracted = resolve(stageDir, spec.inner)
    if (!existsSync(extracted)) throw new Error(`expected ${spec.inner} in tarball not found`)
    renameSync(extracted, outfile)
    chmodSync(outfile, 0o755)
    rmSync(tmpTar, { force: true })
    rmSync(stageDir, { recursive: true, force: true })
  }
  const size = statSync(outfile).size
  console.log(`[fetch-node] ✓ ${outfile} (${(size / 1024 / 1024).toFixed(1)} MB)`)
  writeVersionMarker()
} catch (err) {
  console.error(`[fetch-node] ✗ ${err.message}`)
  console.error(`[fetch-node] manual fallback:`)
  console.error(`             download node ${NODE_VERSION} for ${platform}-${arch} from`)
  console.error(`             https://nodejs.org/dist/${NODE_VERSION}/`)
  console.error(`             and place the binary at ${outfile} (chmod +x on macOS/Linux)`)
  process.exit(1)
}

function writeVersionMarker() {
  writeFileSync(resolve(outdir, 'NODE_VERSION'), NODE_VERSION + '\n')
}
