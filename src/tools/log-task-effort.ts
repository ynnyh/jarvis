import { z } from 'zod'
import path from 'node:path'
import os from 'node:os'
import fs from 'node:fs/promises'
import { toolRegistry } from '../core/tool-registry.js'
import type { Tool } from '../core/tool-registry.js'
import { getZentaoCredentials } from '../config/settings.js'

const inputSchema = z.object({
  taskId: z.string().min(1).describe('禅道任务 ID'),
  hours: z.number().positive().describe('本次新增工时（小时），如 0.01 / 0.5 / 1.5'),
  work: z.string().min(1).describe('工作内容描述，会写到禅道工时记录的 work 字段'),
  date: z.string().regex(/^\d{4}-\d{2}-\d{2}$/).optional().describe('工时归属日期 YYYY-MM-DD，默认今天'),
})

const AUDIT_LOG_PATH = path.join(os.homedir(), '.jarvis', 'write-back.log')

/**
 * 写一行 JSONL 到 ~/.jarvis/write-back.log，记录所有写回禅道的尝试（成功+失败都记）。
 *
 * 为什么用 JSONL：grep/jq 友好，不需要维护 JSON 数组完整性。
 */
async function appendAuditLog(entry: Record<string, unknown>): Promise<void> {
  try {
    await fs.mkdir(path.dirname(AUDIT_LOG_PATH), { recursive: true })
    const line = JSON.stringify({ ts: new Date().toISOString(), ...entry }) + '\n'
    await fs.appendFile(AUDIT_LOG_PATH, line, 'utf-8')
  } catch (e) {
    // 审计日志失败不应阻塞主流程；只打到 stderr
    console.error('[log-task-effort] audit log 写入失败:', e)
  }
}

async function execute(input: z.infer<typeof inputSchema>) {
  const { ZenTaoProvider } = await import('../providers/zentao-provider.js')
  const { baseUrl, account, password, sessionCookie } = getZentaoCredentials()
  const provider = new ZenTaoProvider({ baseUrl, username: account, password, sessionCookie })

  try {
    const result = await provider.addEffort({
      taskId: input.taskId,
      hours: input.hours,
      work: input.work,
      date: input.date,
    })
    await appendAuditLog({
      action: 'log-task-effort',
      ok: true,
      taskId: input.taskId,
      hours: input.hours,
      work: input.work,
      date: input.date ?? null,
      account,
      effortId: result.id ?? null,
      endpoint: result.endpoint ?? null,
      preservedLeft: result.preservedLeft ?? null,
      consumedBefore: result.consumedBefore ?? null,
      consumedAfter: result.consumedAfter ?? null,
      responseText: result.responseText ?? null,
    })
    return {
      ok: true,
      effortId: result.id ?? null,
      endpoint: result.endpoint ?? null,
      preservedLeft: result.preservedLeft ?? null,
      consumedBefore: result.consumedBefore ?? null,
      consumedAfter: result.consumedAfter ?? null,
    }
  } catch (e) {
    const message = e instanceof Error ? e.message : String(e)
    await appendAuditLog({
      action: 'log-task-effort',
      ok: false,
      taskId: input.taskId,
      hours: input.hours,
      work: input.work,
      date: input.date ?? null,
      account,
      error: message,
    })
    throw e
  }
}

export const logTaskEffortTool: Tool = {
  metadata: {
    name: 'log-task-effort',
    description: '向禅道任务追加一条工时记录（不改任务状态）；所有调用落本地审计日志 ~/.jarvis/write-back.log',
    category: 'task',
    version: '1.0.0',
    inputSchema,
    examples: [
      {
        input: { taskId: '10257', hours: 0.01, work: '试发 0.01h 跑通写回链路' },
        description: '闸门 0 练手任务',
      },
    ],
  },
  execute: execute as any,
}

toolRegistry.register(logTaskEffortTool)
