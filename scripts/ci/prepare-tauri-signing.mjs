import { mkdirSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { spawnSync } from 'node:child_process'

function fail(message) {
  console.error(`::error::${message}`)
  process.exit(1)
}

function decodeBase64(value) {
  try {
    return Buffer.from(value, 'base64').toString('utf8').trim()
  } catch {
    fail('TAURI_SIGNING_PRIVATE_KEY_B64 is not valid base64')
  }
}

const keyFromBase64 = process.env.TAURI_SIGNING_PRIVATE_KEY_B64?.trim()
const keyFromPlain = process.env.TAURI_SIGNING_PRIVATE_KEY?.trim()
const password = process.env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD ?? ''

console.log(`[debug] keyFromBase64 length: ${keyFromBase64?.length ?? '(not set)'}`)
console.log(`[debug] keyFromPlain length: ${keyFromPlain?.length ?? '(not set)'}`)
console.log(`[debug] password length: ${password.length}`)
console.log(`[debug] keyFromPlain first 60 chars: ${keyFromPlain?.slice(0, 60) ?? '(not set)'}`)
console.log(`[debug] keyFromPlain last 20 chars: ${keyFromPlain?.slice(-20) ?? '(not set)'}`)

const privateKey = keyFromBase64 ? decodeBase64(keyFromBase64) : keyFromPlain
const keySource = keyFromBase64 ? 'TAURI_SIGNING_PRIVATE_KEY_B64' : 'TAURI_SIGNING_PRIVATE_KEY'

if (!privateKey) {
  fail('Missing updater signing key. Set TAURI_SIGNING_PRIVATE_KEY_B64 or TAURI_SIGNING_PRIVATE_KEY in CI environment variables.')
}

process.env.TAURI_SIGNING_PRIVATE_KEY = privateKey
delete process.env.TAURI_SIGNING_PRIVATE_KEY_B64

const ciDir = join(process.cwd(), '.tauri-ci')
const keyPath = join(ciDir, 'updater-private.key')
const probeFile = join(ciDir, 'probe.txt')

mkdirSync(ciDir, { recursive: true })
writeFileSync(keyPath, privateKey, { encoding: 'utf8', mode: 0o600 })
writeFileSync(probeFile, 'tauri updater signing probe\n', 'utf8')
delete process.env.TAURI_SIGNING_PRIVATE_KEY
process.env.TAURI_SIGNING_PRIVATE_KEY_PATH = keyPath

const result = process.platform === 'win32'
  ? spawnSync('npx', ['tauri', 'signer', 'sign', probeFile], {
      env: process.env,
      encoding: 'utf8',
      shell: true,
    })
  : spawnSync('npx', ['tauri', 'signer', 'sign', probeFile], {
      env: process.env,
      encoding: 'utf8',
      shell: false,
    })

if (result.status !== 0) {
  const output = `${result.error?.message || ''}\n${result.stdout || ''}${result.stderr || ''}`.trim()
  fail(`Updater signing key check failed. Verify that ${keySource} and TAURI_SIGNING_PRIVATE_KEY_PASSWORD match the public key in src-tauri/tauri.conf.json. ${output}`)
}

console.log(`Updater signing key check passed (${keySource}, ${privateKey.length} chars, password ${password ? 'set' : 'empty'}).`)
console.log(`TAURI_SIGNING_PRIVATE_KEY_PATH=${keyPath}`)
