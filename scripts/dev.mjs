import { spawn, execSync } from 'child_process'

const child = spawn('npx', ['tauri', 'dev'], {
  stdio: 'inherit',
  shell: true,
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
