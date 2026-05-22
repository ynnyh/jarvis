import { readFileSync, writeFileSync, existsSync, mkdirSync } from 'fs'
import { join, dirname } from 'path'
import { fileURLToPath } from 'url'

const __filename = fileURLToPath(import.meta.url)
const __dirname = dirname(__filename)

export interface MemoryEntry {
  id: string
  type: 'project' | 'risk' | 'habit' | 'analysis' | 'preference'
  content: string
  tags: string[]
  importance: number // 1-10
  createdAt: string
  updatedAt: string
  accessCount: number
  lastAccessed: string
}

export interface MemoryQuery {
  type?: MemoryEntry['type']
  tags?: string[]
  minImportance?: number
  search?: string
}

export class MemoryStore {
  private dataDir: string
  private memoryFile: string
  private memories: MemoryEntry[] = []
  private static instance: MemoryStore

  static getInstance(): MemoryStore {
    if (!MemoryStore.instance) {
      MemoryStore.instance = new MemoryStore()
    }
    return MemoryStore.instance
  }

  constructor() {
    // 使用项目根目录下的 .jarvis/memory，避免 Windows 用户目录权限问题
    this.dataDir = join(__dirname, '..', '..', '.jarvis', 'memory')
    this.memoryFile = join(this.dataDir, 'memories.json')
    this.ensureDir()
    this.load()
  }

  private ensureDir(): void {
    if (!existsSync(this.dataDir)) {
      mkdirSync(this.dataDir, { recursive: true })
    }
  }

  private load(): void {
    if (existsSync(this.memoryFile)) {
      try {
        const data = readFileSync(this.memoryFile, 'utf-8')
        this.memories = JSON.parse(data)
      } catch {
        this.memories = []
      }
    }
  }

  private save(): void {
    writeFileSync(this.memoryFile, JSON.stringify(this.memories, null, 2))
  }

  add(entry: Omit<MemoryEntry, 'id' | 'createdAt' | 'updatedAt' | 'accessCount' | 'lastAccessed'>): MemoryEntry {
    const now = new Date().toISOString()
    const memory: MemoryEntry = {
      ...entry,
      id: `mem_${Date.now()}_${Math.random().toString(36).substr(2, 9)}`,
      createdAt: now,
      updatedAt: now,
      accessCount: 0,
      lastAccessed: now,
    }
    this.memories.push(memory)
    this.save()
    return memory
  }

  get(id: string): MemoryEntry | undefined {
    const mem = this.memories.find(m => m.id === id)
    if (mem) {
      mem.accessCount++
      mem.lastAccessed = new Date().toISOString()
      this.save()
    }
    return mem
  }

  query(query: MemoryQuery): MemoryEntry[] {
    let results = [...this.memories]

    if (query.type) {
      results = results.filter(m => m.type === query.type)
    }

    if (query.tags && query.tags.length > 0) {
      results = results.filter(m =>
        query.tags!.some(tag => m.tags.includes(tag))
      )
    }

    if (query.minImportance !== undefined && query.minImportance !== null) {
      results = results.filter(m => m.importance >= (query.minImportance ?? 0))
    }

    if (query.search) {
      const q = query.search.toLowerCase()
      results = results.filter(
        m =>
          m.content.toLowerCase().includes(q) ||
          m.tags.some(t => t.toLowerCase().includes(q))
      )
    }

    // 按重要性和最近访问排序
    return results.sort((a, b) => {
      const scoreA = a.importance * 0.6 + (a.accessCount * 0.1)
      const scoreB = b.importance * 0.6 + (b.accessCount * 0.1)
      return scoreB - scoreA
    })
  }

  update(id: string, updates: Partial<Omit<MemoryEntry, 'id' | 'createdAt'>>): MemoryEntry | undefined {
    const index = this.memories.findIndex(m => m.id === id)
    if (index === -1) return undefined

    this.memories[index] = {
      ...this.memories[index],
      ...updates,
      updatedAt: new Date().toISOString(),
    }
    this.save()
    return this.memories[index]
  }

  delete(id: string): boolean {
    const index = this.memories.findIndex(m => m.id === id)
    if (index === -1) return false
    this.memories.splice(index, 1)
    this.save()
    return true
  }

  getRelevantMemories(context: string, limit: number = 5): MemoryEntry[] {
    const keywords = context.toLowerCase().split(/\s+/)
    return this.memories
      .map(m => ({
        memory: m,
        score: keywords.filter(k =>
          m.content.toLowerCase().includes(k) ||
          m.tags.some(t => t.toLowerCase().includes(k))
        ).length,
      }))
      .filter(item => item.score > 0)
      .sort((a, b) => b.score - a.score)
      .slice(0, limit)
      .map(item => item.memory)
  }

  getStats(): { total: number; byType: Record<string, number> } {
    const byType: Record<string, number> = {}
    this.memories.forEach(m => {
      byType[m.type] = (byType[m.type] || 0) + 1
    })
    return { total: this.memories.length, byType }
  }
}

export const memoryStore = MemoryStore.getInstance()
