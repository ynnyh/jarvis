import { homedir } from 'os'
import { join } from 'path'
import { mkdirSync, readFileSync, writeFileSync, unlinkSync, existsSync } from 'fs'

export interface DaemonInfo {
  pid: number
  port: number
  token: string
  startedAt: string
  version: string
}

const JARVIS_DIR = join(homedir(), '.jarvis')
const INFO_FILE = join(JARVIS_DIR, 'daemon.json')

export function getDaemonInfoPath(): string {
  return INFO_FILE
}

export function readDaemonInfo(): DaemonInfo | null {
  if (!existsSync(INFO_FILE)) return null
  try {
    const raw = readFileSync(INFO_FILE, 'utf-8')
    const parsed = JSON.parse(raw) as DaemonInfo
    if (
      typeof parsed.pid !== 'number' ||
      typeof parsed.port !== 'number' ||
      typeof parsed.token !== 'string' ||
      !parsed.token
    ) {
      return null
    }
    return parsed
  } catch {
    return null
  }
}

export function writeDaemonInfo(info: DaemonInfo): void {
  mkdirSync(JARVIS_DIR, { recursive: true })
  writeFileSync(INFO_FILE, JSON.stringify(info, null, 2), 'utf-8')
}

export function removeDaemonInfo(): void {
  try {
    if (existsSync(INFO_FILE)) unlinkSync(INFO_FILE)
  } catch {
    // ignore
  }
}

export function isProcessAlive(pid: number): boolean {
  if (!pid || pid <= 0) return false
  try {
    process.kill(pid, 0)
    return true
  } catch (err: any) {
    return err?.code === 'EPERM'
  }
}
