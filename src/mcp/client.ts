import { Client } from '@modelcontextprotocol/sdk/client/index.js'
import { StdioClientTransport } from '@modelcontextprotocol/sdk/client/stdio.js'

export interface McpServerParams {
  command: string
  args?: string[]
  env?: Record<string, string>
  cwd?: string
}

export interface CallToolResult {
  content: Array<{ type: string; text?: string; [k: string]: unknown }>
  isError?: boolean
}

export class McpClient {
  private client: Client
  private transport: StdioClientTransport
  private connected = false

  constructor(serverParams: McpServerParams, clientName = 'project-agent') {
    this.transport = new StdioClientTransport({
      command: serverParams.command,
      args: serverParams.args ?? [],
      env: { ...process.env as Record<string, string>, ...(serverParams.env ?? {}) },
      cwd: serverParams.cwd,
      stderr: 'pipe',
    })
    this.client = new Client(
      { name: clientName, version: '1.0.0' },
      { capabilities: {} },
    )
  }

  async connect(): Promise<void> {
    if (this.connected) return
    await this.client.connect(this.transport)
    this.connected = true
  }

  async callTool(name: string, args: Record<string, unknown>): Promise<CallToolResult> {
    if (!this.connected) await this.connect()
    const result = await this.client.callTool({ name, arguments: args })
    return result as CallToolResult
  }

  async listTools(): Promise<Array<{ name: string; description?: string }>> {
    if (!this.connected) await this.connect()
    const result = await this.client.listTools()
    return result.tools
  }

  async close(): Promise<void> {
    if (!this.connected) return
    try {
      await this.client.close()
    } finally {
      this.connected = false
    }
  }
}

/**
 * 单次调用的便捷包装：建连接 → 执行 → 关闭。
 * 适合 CLI 一次性 spawn 的场景。
 */
export async function withMcpClient<T>(
  params: McpServerParams,
  fn: (client: McpClient) => Promise<T>,
): Promise<T> {
  const client = new McpClient(params)
  try {
    await client.connect()
    return await fn(client)
  } finally {
    await client.close().catch(() => {})
  }
}

/**
 * 解析 MCP 工具返回的 content：通常是 [{ type: 'text', text: '<json string>' }]，
 * 把第一段 text 当 JSON 解出来。如果不是 JSON 字符串就原样返回。
 */
export function parseToolJsonResult<T = unknown>(result: CallToolResult): T {
  if (result.isError) {
    const msg = result.content?.[0]?.text ?? 'MCP tool returned error'
    throw new Error(`MCP tool error: ${msg}`)
  }
  const first = result.content?.[0]
  if (!first || first.type !== 'text' || !first.text) {
    throw new Error('MCP tool returned no text content')
  }
  try {
    return JSON.parse(first.text) as T
  } catch {
    return first.text as unknown as T
  }
}
