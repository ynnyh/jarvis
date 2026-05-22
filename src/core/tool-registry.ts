import { z } from 'zod'

export interface ToolMetadata {
  name: string
  description: string
  category: string
  version: string
  inputSchema: z.ZodObject<any>
  outputSchema?: z.ZodType<any>
  examples?: Array<{
    input: Record<string, unknown>
    output?: unknown
    description: string
  }>
  requiresAuth?: boolean
  rateLimit?: number
}

export interface Tool {
  metadata: ToolMetadata
  execute: (input: Record<string, unknown>) => Promise<unknown>
}

export class ToolRegistry {
  private tools = new Map<string, Tool>()
  private static instance: ToolRegistry

  static getInstance(): ToolRegistry {
    if (!ToolRegistry.instance) {
      ToolRegistry.instance = new ToolRegistry()
    }
    return ToolRegistry.instance
  }

  register(tool: Tool): void {
    if (this.tools.has(tool.metadata.name)) {
      throw new Error(`Tool ${tool.metadata.name} already registered`)
    }
    this.tools.set(tool.metadata.name, tool)
  }

  get(name: string): Tool | undefined {
    return this.tools.get(name)
  }

  list(): ToolMetadata[] {
    return Array.from(this.tools.values()).map(t => t.metadata)
  }

  listByCategory(category: string): ToolMetadata[] {
    return this.list().filter(t => t.category === category)
  }

  async execute(name: string, input: Record<string, unknown>): Promise<unknown> {
    const tool = this.tools.get(name)
    if (!tool) {
      throw new Error(`Tool ${name} not found`)
    }

    // 输入校验
    const result = tool.metadata.inputSchema.safeParse(input)
    if (!result.success) {
      throw new Error(`Invalid input for tool ${name}: ${result.error.message}`)
    }

    return tool.execute(result.data)
  }

  search(query: string): ToolMetadata[] {
    const q = query.toLowerCase()
    return this.list().filter(
      t =>
        t.name.toLowerCase().includes(q) ||
        t.description.toLowerCase().includes(q) ||
        t.category.toLowerCase().includes(q)
    )
  }
}

export const toolRegistry = ToolRegistry.getInstance()
