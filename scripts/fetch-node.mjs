#!/usr/bin/env node
// 下载便携版 node.exe 到 src-tauri/bundled/node.exe
//
// 用途：让最终 .msi 自带 node 运行时，终端用户不需要装 Node。
// 默认拉 Node 20 LTS Windows x64，可通过 NODE_BUNDLE_VERSION 覆盖。
//
// 跳过条件：目标文件已存在且大小合理（>10MB）。
// 失败时给出清晰错误，让构建者手动放一个 node.exe 也行。

import { fileURLToPath } from 'url'
import { dirname, resolve } from 'path'
import { mkdirSync, existsSync, statSync, createWriteStream, unlinkSync, renameSync, writeFileSync } from 'fs'
import { pipeline } from 'stream/promises'
import { Readable } from 'stream'
import os from 'os'
import https from 'https'

const here = dirname(fileURLToPath(import.meta.url))
const root = resolve(here, '..')
const outdir = resolve(root, 'src-tauri/bundled')
const outfile = resolve(outdir, 'node.exe')

const NODE_VERSION = process.env.NODE_BUNDLE_VERSION || 'v20.18.1'
// 走官方 dist；如内网无法访问，构建者可手动放置 node.exe 跳过本脚本
const url = `https://nodejs.org/dist/${NODE_VERSION}/win-x64/node.exe`

mkdirSync(outdir, { recursive: true })

if (existsSync(outfile) && statSync(outfile).size > 10 * 1024 * 1024) {
  console.log(`[fetch-node] node.exe already present (${(statSync(outfile).size / 1024 / 1024).toFixed(1)} MB), skipping`)
  writeVersionMarker()
  process.exit(0)
}

if (os.platform() !== 'win32') {
  console.warn(`[fetch-node] non-Windows host detected (${os.platform()}); will attempt download anyway`)
}

console.log(`[fetch-node] downloading ${url}`)

const tmp = outfile + '.part'

function download(currentUrl, redirects = 0) {
  return new Promise((resolveDl, reject) => {
    if (redirects > 5) return reject(new Error('too many redirects'))
    https.get(currentUrl, (res) => {
      if (res.statusCode && res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
        res.resume()
        return download(res.headers.location, redirects + 1).then(resolveDl, reject)
      }
      if (res.statusCode !== 200) {
        return reject(new Error(`download failed: HTTP ${res.statusCode}`))
      }
      const out = createWriteStream(tmp)
      pipeline(res, out).then(resolveDl, reject)
    }).on('error', reject)
  })
}

try {
  await download(url)
  const size = statSync(tmp).size
  if (size < 10 * 1024 * 1024) {
    unlinkSync(tmp)
    throw new Error(`downloaded file too small: ${size} bytes`)
  }
  renameSync(tmp, outfile)
  console.log(`[fetch-node] ✓ ${outfile} (${(size / 1024 / 1024).toFixed(1)} MB)`)
  writeVersionMarker()
} catch (err) {
  console.error(`[fetch-node] ✗ ${err.message}`)
  console.error(`[fetch-node] manual fallback: download node.exe ${NODE_VERSION} win-x64 from`)
  console.error(`             https://nodejs.org/dist/${NODE_VERSION}/win-x64/node.exe`)
  console.error(`             and place it at ${outfile}`)
  process.exit(1)
}

function writeVersionMarker() {
  writeFileSync(resolve(outdir, 'NODE_VERSION'), NODE_VERSION + '\n')
}
