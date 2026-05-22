import { McpClient, McpServerParams, parseToolJsonResult, withMcpClient } from './client.js'

// ===== 类型（与 tencentcode-mcp 的 local-git.ts 保持同步） =====

export interface LocalCommit {
  sha: string
  shortSha: string
  authorName: string
  authorEmail: string
  authoredDate: string
  committerName: string
  committerEmail: string
  committedDate: string
  title: string
  body?: string
  stat?: {
    filesChanged: number
    insertions: number
    deletions: number
    files: Array<{ path: string; insertions: number; deletions: number; binary?: boolean }>
  }
}

export interface RepoCommits {
  repoPath: string
  commits: LocalCommit[]
}

export type RangePreset =
  | 'today'
  | 'yesterday'
  | 'thisWeek'
  | 'lastWeek'
  | 'last7days'
  | 'last30days'
  | 'thisMonth'

export interface ListMyLocalCommitsInput {
  rootDir?: string | string[]
  range?: RangePreset
  since?: string
  until?: string
  author?: string
  match?: 'author' | 'committer' | 'any'
  includeBody?: boolean
  includeStat?: boolean
}

export interface ListMyLocalCommitsResult {
  range: { since: string; until: string; label: string }
  authors: string[]
  rootDirs: string[]
  repos: RepoCommits[]
  totalCommits: number
  scannedRepos: number
  reposWithCommits?: number
}

// ===== 配置 =====

function getDefaultServerParams(): McpServerParams {
  const entry = process.env.TENCENTCODE_MCP_ENTRY
    || 'D:/coding/my-mcp-servers/tencentcode-mcp/dist/index.js'
  return {
    command: 'node',
    args: [entry],
    env: {
      // tencentcode-mcp 启动时强制校验 token；本地 git 工具其实不用，给一个占位让它启动起来
      TENCENT_CODE_ACCESS_TOKEN: process.env.TENCENT_CODE_ACCESS_TOKEN || 'unused-by-local-git-tools',
      TENCENT_CODE_LOCAL_ROOTS: process.env.TENCENT_CODE_LOCAL_ROOTS || 'D:/coding',
    },
  }
}

// ===== 高级 API =====

export class TencentCodeMcpClient {
  private mcp: McpClient

  constructor(params?: McpServerParams) {
    this.mcp = new McpClient(params ?? getDefaultServerParams(), 'project-agent-tencentcode')
  }

  async connect(): Promise<void> {
    await this.mcp.connect()
  }

  async close(): Promise<void> {
    await this.mcp.close()
  }

  async listMyLocalCommits(input: ListMyLocalCommitsInput = {}): Promise<ListMyLocalCommitsResult> {
    const result = await this.mcp.callTool('list_my_local_commits', input as Record<string, unknown>)
    return parseToolJsonResult<ListMyLocalCommitsResult>(result)
  }

  async getLocalCommitDiff(input: { repoPath: string; sha: string }): Promise<unknown> {
    const result = await this.mcp.callTool('get_local_commit_diff', input)
    return parseToolJsonResult(result)
  }
}

/**
 * 一次性调用封装：建连接 → 调工具 → 关闭。
 * 保留给极少数明确需要短连接的场景；常规调用走 sharedTencentCodeMcpClient。
 */
export async function listMyLocalCommitsOnce(
  input: ListMyLocalCommitsInput = {},
): Promise<ListMyLocalCommitsResult> {
  return withMcpClient(getDefaultServerParams(), async (client) => {
    const result = await client.callTool('list_my_local_commits', input as Record<string, unknown>)
    return parseToolJsonResult<ListMyLocalCommitsResult>(result)
  })
}

// ===== 共享单例（守护进程 / CLI 同进程内复用，避免每次 spawn tencentcode-mcp） =====

let sharedClient: TencentCodeMcpClient | null = null
let sharedConnectPromise: Promise<TencentCodeMcpClient> | null = null

/**
 * 取共享 client；首次调用懒启动，后续直接复用。
 * 连接失败会清空缓存让下次重试，不把脏状态留给调用方。
 */
export async function getSharedTencentCodeMcpClient(): Promise<TencentCodeMcpClient> {
  if (sharedClient) return sharedClient
  if (sharedConnectPromise) return sharedConnectPromise
  sharedConnectPromise = (async () => {
    const client = new TencentCodeMcpClient()
    try {
      await client.connect()
      sharedClient = client
      return client
    } catch (err) {
      sharedClient = null
      throw err
    } finally {
      sharedConnectPromise = null
    }
  })()
  return sharedConnectPromise
}

export async function closeSharedTencentCodeMcpClient(): Promise<void> {
  const c = sharedClient
  sharedClient = null
  sharedConnectPromise = null
  if (c) {
    try {
      await c.close()
    } catch {
      // ignore close errors
    }
  }
}

/**
 * 用共享 client 拉本地 commit。优先用这个，而不是 listMyLocalCommitsOnce。
 */
export async function listMyLocalCommitsShared(
  input: ListMyLocalCommitsInput = {},
): Promise<ListMyLocalCommitsResult> {
  const client = await getSharedTencentCodeMcpClient()
  return client.listMyLocalCommits(input)
}
