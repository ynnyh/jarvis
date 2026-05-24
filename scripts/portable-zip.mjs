#!/usr/bin/env node
// 把 src-tauri/target/release/ 里的产物打成便携 zip，可直接发给同事解压运行
//
// 用途：当 .msi/.nsis 构建因网络/工具链问题失败时的兜底分发方式。
// 输出：dist/Jarvis-portable-<date>.zip
//
// zip 内容：
//   Jarvis/
//     jarvis.exe
//     bundled/
//       node.exe
//       daemon.mjs
//
// 解压后双击 jarvis.exe 即可运行（Windows 10+ 自带 WebView2 运行时）。

import { fileURLToPath } from 'url'
import { dirname, resolve } from 'path'
import { existsSync, statSync, mkdirSync, readdirSync, readFileSync, writeFileSync, createWriteStream } from 'fs'
import { execSync } from 'child_process'
import os from 'os'

if (os.platform() !== 'win32') {
  console.log(`[portable-zip] 跳过：当前平台 ${os.platform()} 不是 Windows。`)
  console.log(`[portable-zip] Mac/Linux 用户请用 tauri build 生成的 .dmg/.deb`)
  process.exit(0)
}

const here = dirname(fileURLToPath(import.meta.url))
const root = resolve(here, '..')
const releaseDir = resolve(root, 'src-tauri/target/release')
const distDir = resolve(root, 'dist')
const exe = resolve(releaseDir, 'jarvis.exe')
const bundledDir = resolve(releaseDir, 'bundled')

for (const p of [exe, resolve(bundledDir, 'node.exe'), resolve(bundledDir, 'daemon.mjs'), resolve(bundledDir, 'zentao-test.mjs')]) {
  if (!existsSync(p)) {
    console.error(`[portable-zip] missing required artifact: ${p}`)
    console.error(`[portable-zip] run "npm run desktop:build" (or at least "npm run desktop:prebuild && cargo build --release" in src-tauri/) first`)
    process.exit(1)
  }
}

mkdirSync(distDir, { recursive: true })

const date = new Date().toISOString().slice(0, 10)
const zipName = `Jarvis-portable-${date}.zip`
const zipPath = resolve(distDir, zipName)

// 用 PowerShell 的 Compress-Archive — Windows 自带，避免引入额外依赖
//
// 结构上让 zip 解压后产生 Jarvis/ 顶层目录：我们先在临时目录布好，再 zip 整个目录。
const stageDir = resolve(distDir, '_portable_stage')
try { execSync(`powershell -Command "Remove-Item -Recurse -Force '${stageDir.replace(/\\/g, '\\\\')}'"`, { stdio: 'ignore' }) } catch {}

const stageRoot = resolve(stageDir, 'Jarvis')
const stageBundled = resolve(stageRoot, 'bundled')
mkdirSync(stageBundled, { recursive: true })

execSync(`powershell -Command "Copy-Item -Path '${exe}' -Destination '${stageRoot}'"`, { stdio: 'inherit' })
execSync(`powershell -Command "Copy-Item -Path '${resolve(bundledDir, 'node.exe')}' -Destination '${stageBundled}'"`, { stdio: 'inherit' })
execSync(`powershell -Command "Copy-Item -Path '${resolve(bundledDir, 'daemon.mjs')}' -Destination '${stageBundled}'"`, { stdio: 'inherit' })
execSync(`powershell -Command "Copy-Item -Path '${resolve(bundledDir, 'zentao-test.mjs')}' -Destination '${stageBundled}'"`, { stdio: 'inherit' })

// 简单 README
const readme = `Jarvis · 便携版
================

使用：
  1. 解压本目录
  2. 双击 jarvis.exe
  3. 首次启动按欢迎引导配置禅道账号和代码文件夹

要求：Windows 10/11 自带 WebView2（多数机器已有，否则系统会提示安装）

数据存放位置：%USERPROFILE%\\.jarvis\\
  - config.json    所有偏好设置
  - daemon.json    后端进程信息（运行时生成）
  - memory/        历史日报、上下文记忆

卸载：删除整个 Jarvis 目录 + %USERPROFILE%\\.jarvis\\
`
writeFileSync(resolve(stageRoot, 'README.txt'), readme)

if (existsSync(zipPath)) {
  try { execSync(`powershell -Command "Remove-Item -Force '${zipPath}'"`, { stdio: 'ignore' }) } catch {}
}

console.log(`[portable-zip] compressing → ${zipName}`)
execSync(
  `powershell -Command "Compress-Archive -Path '${stageRoot}' -DestinationPath '${zipPath}' -CompressionLevel Optimal"`,
  { stdio: 'inherit' },
)

const size = statSync(zipPath).size
console.log(`[portable-zip] ✓ ${zipPath} (${(size / 1024 / 1024).toFixed(1)} MB)`)

// 清理 stage
try { execSync(`powershell -Command "Remove-Item -Recurse -Force '${stageDir}'"`, { stdio: 'ignore' }) } catch {}
