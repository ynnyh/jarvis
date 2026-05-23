#!/usr/bin/env node
/**
 * Jarvis Daemon Server
 *
 * 常驻 Node 进程，对 Tauri/CLI 暴露 HTTP API，避免每次调用都 spawn 子进程。
 *
 * 设计要点：
 * - 仅绑 127.0.0.1（loopback），杜绝外网访问
 * - 启动时生成随机 Bearer Token，写到 ~/.jarvis/daemon.json
 * - 端口由内核分配（port 0），避免冲突
 * - 同名 in-flight 请求去重：key = method+path+bodyHash
 * - 30 分钟无请求自动退出（idle 兜底）
 * - SIGINT/SIGTERM 优雅退出：close server + close MCP shared client + 删 daemon.json
 * - 启动前若发现存活的 daemon.json + pid 存活 + /health OK，则直接退出（避免重复实例）
 */

import 'dotenv/config'
import '../index.js'

import http, { IncomingMessage, ServerResponse } from 'http'
import { randomBytes, createHash } from 'crypto'
import { AddressInfo } from 'net'

import { toolRegistry } from '../core/tool-registry.js'
import { actionEngine } from '../actions/action-engine.js'
import { memoryStore } from '../memory/memory-store.js'
import { agentScheduler } from '../scheduler/agent-scheduler.js'
import { contextBuilder } from '../ai/context-builder.js'
import { agentState } from '../core/agent-state.js'
import { GitProvider } from '../providers/git/git-provider.js'
import { reloadSettings, settingsFilePath } from '../config/settings.js'
import { _clearCache as clearExcludedCache } from '../config/excluded-business-lines.js'

import {
  readDaemonInfo,
  writeDaemonInfo,
  removeDaemonInfo,
  isProcessAlive,
  getDaemonInfoPath,
} from './daemon-info.js'

// ===== 配置 =====

const VERSION = '1.0.0'
const IDLE_TIMEOUT_MS = 30 * 60 * 1000  // 30 分钟无请求自动退出
const BODY_LIMIT = 10 * 1024 * 1024     // 10MB

const startedAt = new Date().toISOString()
const startedAtMs = Date.now()
let lastActivityAt = startedAtMs

// ===== 路由 =====

type Handler = (req: IncomingMessage, body: any, params: Record<string, string>) => Promise<any>

interface Route {
  method: string
  // 正则匹配，命名捕获通过 ':param' 占位声明
  pattern: RegExp
  paramNames: string[]
  handler: Handler
}

function makeRoute(method: string, path: string, handler: Handler): Route {
  const paramNames: string[] = []
  const regexStr = path.replace(/:([a-zA-Z_]+)/g, (_, name) => {
    paramNames.push(name)
    return '([^/]+)'
  })
  return {
    method,
    pattern: new RegExp(`^${regexStr}$`),
    paramNames,
    handler,
  }
}

const routes: Route[] = []

function GET(path: string, handler: Handler) { routes.push(makeRoute('GET', path, handler)) }
function POST(path: string, handler: Handler) { routes.push(makeRoute('POST', path, handler)) }

// ===== 业务路由 =====

GET('/health', async () => ({
  ok: true,
  version: VERSION,
  pid: process.pid,
  startedAt,
  uptimeMs: Date.now() - startedAtMs,
  lastActivityAt: new Date(lastActivityAt).toISOString(),
}))

POST('/shutdown', async () => {
  // 不直接 exit，让 response 先返回完整
  setImmediate(() => shutdown('shutdown endpoint').catch(() => process.exit(0)))
  return { ok: true }
})

GET('/tools', async () => toolRegistry.list())

POST('/tool/:name', async (_req, body, { name }) => {
  const input = (body && typeof body === 'object') ? body : {}
  return toolRegistry.execute(name, input as Record<string, unknown>)
})

GET('/actions', async () => actionEngine.list().map(a => ({
  id: a.id,
  name: a.name,
  description: a.description,
  steps: a.steps.map(s => s.tool),
})))

POST('/action/:id', async (_req, _body, { id }) => actionEngine.execute(id))

GET('/state', async () => agentState.getStats())

GET('/scheduler', async () => ({
  status: agentScheduler.getStatus(),
  tasks: agentScheduler.list(),
}))

POST('/scheduler/start', async () => {
  agentScheduler.start()
  return { ok: true, status: agentScheduler.getStatus() }
})

POST('/scheduler/stop', async () => {
  agentScheduler.stop()
  return { ok: true, status: agentScheduler.getStatus() }
})

GET('/context', async () => ({ prompt: await contextBuilder.buildPrompt() }))

GET('/git', async () => {
  const git = new GitProvider()
  if (!git.isRepo()) return { isRepo: false }
  return {
    isRepo: true,
    info: git.getRepoInfo(),
    status: git.getStatus(),
    recentCommits: git.getRecentCommits(5),
  }
})

GET('/memory', async (_req, _body) => memoryStore.query({}))
POST('/memory', async (_req, body) => memoryStore.add(body))

// 设置改动后由 Tauri config_save 调用，让 daemon 内的缓存失效。
POST('/settings/reload', async () => {
  const s = reloadSettings()
  clearExcludedCache()
  return { ok: true, file: settingsFilePath(), hasZentao: !!s.zentao.baseUrl, repoRoots: s.repoRoots.length }
})

// ===== HTTP 处理 =====

const token = randomBytes(24).toString('base64url')

function unauthorized(res: ServerResponse, msg: string) {
  res.writeHead(401, { 'content-type': 'application/json; charset=utf-8' })
  res.end(JSON.stringify({ error: msg }))
}

function notFound(res: ServerResponse) {
  res.writeHead(404, { 'content-type': 'application/json; charset=utf-8' })
  res.end(JSON.stringify({ error: 'not found' }))
}

function methodNotAllowed(res: ServerResponse) {
  res.writeHead(405, { 'content-type': 'application/json; charset=utf-8' })
  res.end(JSON.stringify({ error: 'method not allowed' }))
}

function serverError(res: ServerResponse, err: unknown) {
  const message = err instanceof Error ? err.message : String(err)
  const stack = err instanceof Error ? err.stack : undefined
  res.writeHead(500, { 'content-type': 'application/json; charset=utf-8' })
  res.end(JSON.stringify({ error: message, stack }))
}

async function readBody(req: IncomingMessage): Promise<any> {
  return new Promise((resolve, reject) => {
    let size = 0
    const chunks: Buffer[] = []
    req.on('data', (chunk: Buffer) => {
      size += chunk.length
      if (size > BODY_LIMIT) {
        reject(new Error(`request body exceeds ${BODY_LIMIT} bytes`))
        req.destroy()
        return
      }
      chunks.push(chunk)
    })
    req.on('end', () => {
      if (chunks.length === 0) return resolve(undefined)
      const raw = Buffer.concat(chunks).toString('utf-8')
      if (!raw.trim()) return resolve(undefined)
      try { resolve(JSON.parse(raw)) }
      catch (e) { reject(new Error('invalid JSON body')) }
    })
    req.on('error', reject)
  })
}

// in-flight 去重
const inflight = new Map<string, Promise<any>>()

function inflightKey(method: string, path: string, body: any): string {
  const bodyStr = body === undefined ? '' : JSON.stringify(body)
  return `${method} ${path} ${createHash('sha1').update(bodyStr).digest('hex')}`
}

async function handleRequest(req: IncomingMessage, res: ServerResponse): Promise<void> {
  const method = req.method || 'GET'
  const url = new URL(req.url || '/', 'http://localhost')
  const path = url.pathname

  // 鉴权（/health 仍要 token，避免端口扫描泄露版本）
  const auth = req.headers['authorization']
  if (!auth || auth !== `Bearer ${token}`) {
    return unauthorized(res, 'missing or invalid token')
  }

  // 查路由
  let matched: Route | null = null
  let params: Record<string, string> = {}
  let pathMatchedButMethodDiffers = false
  for (const r of routes) {
    const m = r.pattern.exec(path)
    if (!m) continue
    if (r.method !== method) {
      pathMatchedButMethodDiffers = true
      continue
    }
    matched = r
    r.paramNames.forEach((name, i) => { params[name] = decodeURIComponent(m[i + 1]) })
    break
  }
  if (!matched) {
    return pathMatchedButMethodDiffers ? methodNotAllowed(res) : notFound(res)
  }

  let body: any
  try {
    body = await readBody(req)
  } catch (err) {
    res.writeHead(400, { 'content-type': 'application/json; charset=utf-8' })
    res.end(JSON.stringify({ error: err instanceof Error ? err.message : String(err) }))
    return
  }

  lastActivityAt = Date.now()

  // in-flight 去重：相同 method+path+body 并发时共享结果
  const key = inflightKey(method, path, body)
  let resultPromise = inflight.get(key)
  let isOwner = false
  if (!resultPromise) {
    isOwner = true
    resultPromise = matched.handler(req, body, params)
    inflight.set(key, resultPromise)
    resultPromise.finally(() => { if (isOwner) inflight.delete(key) })
  }

  try {
    const result = await resultPromise
    res.writeHead(200, { 'content-type': 'application/json; charset=utf-8' })
    res.end(JSON.stringify(result ?? null))
  } catch (err) {
    serverError(res, err)
  }
}

// ===== 启动 / 关闭 =====

let server: http.Server | null = null
let idleTimer: ReturnType<typeof setInterval> | null = null
let shuttingDown = false

async function checkExistingDaemon(): Promise<boolean> {
  const info = readDaemonInfo()
  if (!info) return false
  if (!isProcessAlive(info.pid)) {
    removeDaemonInfo()
    return false
  }
  // 试 ping /health 确认对方真是 daemon
  try {
    const result = await fetch(`http://127.0.0.1:${info.port}/health`, {
      headers: { authorization: `Bearer ${info.token}` },
      signal: AbortSignal.timeout(2000),
    })
    if (result.ok) {
      const data = await result.json()
      if (data && typeof data === 'object' && (data as any).ok === true) {
        return true
      }
    }
  } catch {
    // 进程在但不响应 → 视为僵尸，清理后重启
  }
  removeDaemonInfo()
  return false
}

async function shutdown(reason: string): Promise<void> {
  if (shuttingDown) return
  shuttingDown = true
  console.error(`[daemon] shutting down: ${reason}`)

  if (idleTimer) {
    clearInterval(idleTimer)
    idleTimer = null
  }

  // 停 server（拒绝新连接，等现有响应完成）
  const closePromise = new Promise<void>((resolve) => {
    if (!server) return resolve()
    server.close(() => resolve())
  })

  // git 扫描现在直接在本进程内运行，没有子进程要关

  // 5 秒兜底
  const timeout = new Promise<void>((resolve) => setTimeout(resolve, 5000))
  await Promise.race([closePromise, timeout])

  removeDaemonInfo()
  process.exit(0)
}

async function main() {
  if (await checkExistingDaemon()) {
    console.error(`[daemon] another daemon is already running (info: ${getDaemonInfoPath()}). exiting.`)
    process.exit(0)
  }

  server = http.createServer((req, res) => {
    handleRequest(req, res).catch((err) => serverError(res, err))
  })

  // 防止单个 socket 阻塞 shutdown
  server.keepAliveTimeout = 5000
  server.requestTimeout = 60_000

  server.listen(0, '127.0.0.1', () => {
    const addr = server!.address() as AddressInfo
    writeDaemonInfo({
      pid: process.pid,
      port: addr.port,
      token,
      startedAt,
      version: VERSION,
    })
    console.error(`[daemon] listening on 127.0.0.1:${addr.port} (pid ${process.pid})`)
    console.error(`[daemon] info: ${getDaemonInfoPath()}`)
  })

  // idle 自杀检查
  idleTimer = setInterval(() => {
    if (Date.now() - lastActivityAt > IDLE_TIMEOUT_MS) {
      shutdown(`idle for ${IDLE_TIMEOUT_MS / 60000} min`).catch(() => process.exit(0))
    }
  }, 60_000)

  // 信号处理
  process.on('SIGINT', () => shutdown('SIGINT').catch(() => process.exit(0)))
  process.on('SIGTERM', () => shutdown('SIGTERM').catch(() => process.exit(0)))
  // Windows 上 Ctrl+C 会发 SIGBREAK
  process.on('SIGBREAK' as any, () => shutdown('SIGBREAK').catch(() => process.exit(0)))

  // 兜底：未捕获异常打印不退出，避免 daemon 因偶发错误整死
  process.on('uncaughtException', (err) => {
    console.error('[daemon] uncaughtException:', err)
  })
  process.on('unhandledRejection', (reason) => {
    console.error('[daemon] unhandledRejection:', reason)
  })
}

main().catch((err) => {
  console.error('[daemon] failed to start:', err)
  process.exit(1)
})
