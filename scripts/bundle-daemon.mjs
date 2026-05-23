#!/usr/bin/env node
// 把 dist/daemon/server.js + 所有 runtime deps 打成单文件 CJS，
// 这样发行版只需要带一个 node.exe 就能跑，不需要拷 node_modules。
//
// 输出：src-tauri/bundled/daemon.cjs
// 平台无关；node.exe 由 fetch-node.mjs 单独下载到同目录。

import { build } from 'esbuild'
import { fileURLToPath } from 'url'
import { dirname, resolve } from 'path'
import { mkdirSync, existsSync, statSync } from 'fs'

const here = dirname(fileURLToPath(import.meta.url))
const root = resolve(here, '..')
const entry = resolve(root, 'dist/daemon/server.js')
const outdir = resolve(root, 'src-tauri/bundled')
const outfile = resolve(outdir, 'daemon.mjs')

if (!existsSync(entry)) {
  console.error(`[bundle-daemon] entry not found: ${entry}`)
  console.error(`[bundle-daemon] run "npm run build" first`)
  process.exit(1)
}

mkdirSync(outdir, { recursive: true })

await build({
  entryPoints: [entry],
  bundle: true,
  platform: 'node',
  target: 'node20',
  format: 'esm',
  outfile,
  minify: false,
  sourcemap: false,
  external: [],
  // 给打包后的 ESM 注入 require()，覆盖少数 CJS 依赖在 ESM 上下文里要求的 require
  banner: {
    js: "import { createRequire as __jarvisCreateRequire } from 'module'; const require = __jarvisCreateRequire(import.meta.url);",
  },
  legalComments: 'none',
  logLevel: 'info',
})

const size = statSync(outfile).size
console.log(`[bundle-daemon] ✓ ${outfile} (${(size / 1024).toFixed(1)} KB)`)
