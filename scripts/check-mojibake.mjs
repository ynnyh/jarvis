import fs from 'node:fs'
import path from 'node:path'

const root = process.cwd()
const modeArg = process.argv.find(arg => arg.startsWith('--mode=')) ?? '--mode=strict'
const mode = modeArg.slice('--mode='.length)

const textExt = new Set([
  '.ts', '.tsx', '.js', '.mjs', '.cjs', '.vue', '.json', '.rs', '.md', '.html', '.css', '.yml', '.yaml', '.toml',
])

const commonSuspiciousPatterns = [
  { label: 'replacement-char', regex: /�/u },
  { label: 'mojibake-punct', regex: /鈥斺|锛岄|銆備/u },
  { label: 'mojibake-ui', regex: /闅愯棌|鏄剧ず|鍒囨崲|杩為€|璋冪敤澶辫触|鏈厤缃/u },
  { label: 'mojibake-domain', regex: /妯″瀷|鍗忚|鍐欏叆|绂呴亾|甯嗚蒋|鍙洖涓€/u },
]

const modeConfig = {
  strict: {
    includeRoots: [
      'desktop/src',
      'desktop/chat.html',
      'desktop/index.html',
      'desktop/index.mock.html',
      'desktop/manualHours.html',
      'desktop/settings.html',
      'desktop/writeHours.html',
      'src-tauri/src',
      'src-tauri/tauri.conf.json',
      'src-tauri/Cargo.toml',
      'scripts/ci',
      'scripts/publish-to-gitee.mjs',
      'README.md',
      'CHANGELOG.md',
      'package.json',
      'vite.config.ts',
      '.github/workflows',
      '.circleci/config.yml',
    ],
    skipFiles: [
      'scripts/check-mojibake.mjs',
    ],
  },
  full: {
    includeRoots: [
      'desktop',
      'src-tauri',
      'scripts',
      'tools',
      'README.md',
      'CHANGELOG.md',
      'package.json',
      'vite.config.ts',
      '.github/workflows',
      '.circleci',
    ],
    skipFiles: [
      'scripts/check-mojibake.mjs',
      'dev-api-page.html',
      'my-tasks-full.html',
      'zentao-my-tasks.html',
      'zentao-tasks-page.html',
    ],
    skipDirs: [
      'desktop/src/assets',
    ],
  },
}

if (!(mode in modeConfig)) {
  console.error(`Unknown mode: ${mode}. Expected one of: ${Object.keys(modeConfig).join(', ')}`)
  process.exit(2)
}

const config = modeConfig[mode]
const skipDirs = new Set([
  '.git',
  'node_modules',
  'dist',
  'src-tauri/target',
  ...(config.skipDirs ?? []),
])
const skipFiles = new Set(config.skipFiles)

function shouldSkip(rel) {
  return skipFiles.has(rel) || [...skipDirs].some(dir => rel === dir || rel.startsWith(`${dir}/`))
}

function isIncluded(rel) {
  return config.includeRoots.some(item => rel === item || rel.startsWith(`${item}/`))
}

function walk(dir, out) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name)
    const rel = path.relative(root, full).replace(/\\/g, '/')
    if (shouldSkip(rel)) continue
    if (entry.isDirectory()) {
      walk(full, out)
      continue
    }
    if (!isIncluded(rel)) continue
    if (!textExt.has(path.extname(entry.name).toLowerCase())) continue
    out.push(full)
  }
}

const files = []
walk(root, files)

const findings = []
for (const file of files) {
  const rel = path.relative(root, file).replace(/\\/g, '/')
  const text = fs.readFileSync(file, 'utf8')
  const lines = text.split(/\r?\n/)
  lines.forEach((line, index) => {
    for (const pattern of commonSuspiciousPatterns) {
      if (pattern.regex.test(line)) {
        findings.push({
          file: rel,
          line: index + 1,
          label: pattern.label,
          text: line.trim(),
        })
        break
      }
    }
  })
}

if (findings.length > 0) {
  console.error(`Detected suspicious mojibake text in ${mode} mode:`)
  for (const item of findings) {
    console.error(`${item.file}:${item.line} [${item.label}] ${item.text}`)
  }
  process.exit(1)
}

console.log(`Encoding audit passed in ${mode} mode (${files.length} files checked).`)
