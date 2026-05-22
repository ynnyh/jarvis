import { execSync } from 'child_process'
import { existsSync } from 'fs'
import { resolve } from 'path'

export interface GitCommit {
  hash: string
  shortHash: string
  message: string
  author: string
  date: string
  files: string[]
}

export interface GitStatus {
  branch: string
  ahead: number
  behind: number
  modified: string[]
  added: string[]
  deleted: string[]
  untracked: string[]
  staged: string[]
}

export interface GitBranch {
  name: string
  current: boolean
  remote?: string
}

export class GitProvider {
  private repoPath: string

  constructor(repoPath: string = process.cwd()) {
    this.repoPath = resolve(repoPath)
  }

  private exec(args: string[]): string {
    try {
      return execSync(`git ${args.join(' ')}`, {
        cwd: this.repoPath,
        encoding: 'utf-8',
        maxBuffer: 10 * 1024 * 1024,
      }).trim()
    } catch {
      return ''
    }
  }

  isRepo(): boolean {
    return existsSync(resolve(this.repoPath, '.git'))
  }

  getCurrentBranch(): string {
    return this.exec(['branch', '--show-current'])
  }

  getBranches(): GitBranch[] {
    const output = this.exec(['branch', '-a'])
    if (!output) return []

    return output.split('\n').map(line => {
      const current = line.startsWith('*')
      const name = line.replace(/^\*?\s+/, '').trim()
      return {
        name,
        current,
        remote: name.startsWith('remotes/') ? name.split('/')[1] : undefined,
      }
    })
  }

  getRecentCommits(limit: number = 10): GitCommit[] {
    const format = '%H|%h|%s|%an|%ad'
    const output = this.exec([
      'log',
      `--pretty=format:${format}`,
      '--date=iso',
      `-n ${limit}`,
    ])

    if (!output) return []

    return output.split('\n').map(line => {
      const [hash, shortHash, message, author, date] = line.split('|')
      return {
        hash,
        shortHash,
        message,
        author,
        date,
        files: this.getCommitFiles(hash),
      }
    })
  }

  private getCommitFiles(hash: string): string[] {
    const output = this.exec(['diff-tree', '--no-commit-id', '--name-only', '-r', hash])
    return output ? output.split('\n').filter(Boolean) : []
  }

  getStatus(): GitStatus {
    const branch = this.getCurrentBranch()

    // 获取 ahead/behind
    const aheadBehind = this.exec(['rev-list', '--left-right', '--count', `origin/${branch}...${branch}`])
    const [behind, ahead] = aheadBehind ? aheadBehind.split('\t').map(Number) : [0, 0]

    // 获取状态
    const statusOutput = this.exec(['status', '--porcelain'])
    const lines = statusOutput ? statusOutput.split('\n') : []

    const modified: string[] = []
    const added: string[] = []
    const deleted: string[] = []
    const untracked: string[] = []
    const staged: string[] = []

    for (const line of lines) {
      if (!line) continue
      const status = line.slice(0, 2)
      const file = line.slice(3).trim()

      if (status[0] === 'M' || status[0] === 'A' || status[0] === 'D') {
        staged.push(file)
      }
      if (status[1] === 'M') modified.push(file)
      if (status[1] === 'A') added.push(file)
      if (status[1] === 'D') deleted.push(file)
      if (status === '??') untracked.push(file)
    }

    return {
      branch,
      ahead: ahead || 0,
      behind: behind || 0,
      modified,
      added,
      deleted,
      untracked,
      staged,
    }
  }

  searchCode(query: string): Array<{ file: string; line: number; content: string }> {
    const output = this.exec(['grep', '-n', '-i', query, '--', '.'])
    if (!output) return []

    return output.split('\n').map(line => {
      const [file, lineNum, ...contentParts] = line.split(':')
      return {
        file: file || '',
        line: parseInt(lineNum) || 0,
        content: contentParts.join(':').trim(),
      }
    })
  }

  getDiff(): string {
    return this.exec(['diff'])
  }

  getFileHistory(file: string, limit: number = 5): GitCommit[] {
    const format = '%H|%h|%s|%an|%ad'
    const output = this.exec([
      'log',
      `--pretty=format:${format}`,
      '--date=iso',
      `-n ${limit}`,
      '--',
      file,
    ])

    if (!output) return []

    return output.split('\n').map(line => {
      const [hash, shortHash, message, author, date] = line.split('|')
      return { hash, shortHash, message, author, date, files: [file] }
    })
  }

  getRepoInfo(): {
    path: string
    branch: string
    commitCount: number
    lastCommit: GitCommit | null
    remoteUrl: string
  } {
    const branch = this.getCurrentBranch()
    const commitCount = parseInt(this.exec(['rev-list', '--count', 'HEAD'])) || 0
    const remoteUrl = this.exec(['remote', 'get-url', 'origin'])

    const commits = this.getRecentCommits(1)

    return {
      path: this.repoPath,
      branch,
      commitCount,
      lastCommit: commits[0] || null,
      remoteUrl,
    }
  }
}
