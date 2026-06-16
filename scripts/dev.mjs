import { spawn, execSync } from 'child_process'
import { resolve } from 'path'
import { fileURLToPath } from 'url'

// 直接用本地 node_modules 里的 @tauri-apps/cli 入口启动，
// 避免走 npx（npx 找不到本地 tauri 时会去 registry 拉同名占位包 tauri@0.15.0，
// 报 "could not determine executable to run"）。同时 shell:false 消除
// "Passing args to a child process with shell option true" 安全告警。
const cliEntry = resolve(
  fileURLToPath(new URL('.', import.meta.url)),
  '..',
  'node_modules',
  '@tauri-apps',
  'cli',
  'tauri.js'
)

const child = spawn(process.execPath, [cliEntry, 'dev'], {
  stdio: 'inherit',
})

let cleaned = false

function cleanup() {
  if (cleaned) return
  cleaned = true
  try {
    if (process.platform === 'win32') {
      // 杀掉整个进程树（包括 Rust 编译的 .exe）
      execSync(`taskkill /pid ${child.pid} /T /F`, { stdio: 'ignore' })
    } else {
      child.kill('SIGINT')
    }
  } catch {}
}

// 各种退出场景都清理
process.on('exit', cleanup)
process.on('SIGINT', () => { cleanup(); process.exit() })
process.on('SIGTERM', () => { cleanup(); process.exit() })

child.on('close', (code) => process.exit(code ?? 0))
