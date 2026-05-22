import type { LocalCommit } from '../mcp/tencentcode-client.js'

/**
 * 判断一个路径是否属于"生成 / 锁文件"，不应计入工作量。
 *
 * 思路：宁可漏放也不要误杀。这里只列高置信度的项：
 *   - 依赖锁文件：package-lock.json / yarn.lock / pnpm-lock.yaml / Cargo.lock 等
 *   - 编译产物路径：dist/ build/ out/ .next/ target/ node_modules/
 *   - 明显的构建产物：*.min.js / *.min.css / *.map
 *
 * 不试图识别"代码生成器输出"（protobuf、graphql codegen）——那些路径项目特定，
 * 容易误杀，留给后续真的有需要时再做白名单配置。
 */
export function isGeneratedPath(p: string): boolean {
  const lower = p.toLowerCase().replace(/\\/g, '/')

  // 锁文件
  if (/(^|\/)(package-lock\.json|yarn\.lock|pnpm-lock\.yaml|cargo\.lock|composer\.lock|poetry\.lock|gemfile\.lock|go\.sum|bun\.lockb)$/.test(lower)) {
    return true
  }
  // 压缩产物
  if (/\.min\.(js|css)$/.test(lower)) return true
  // sourcemap
  if (/\.map$/.test(lower)) return true
  // 构建/依赖目录（任意层级）
  if (/(^|\/)(node_modules|dist|build|out|\.next|target|\.cache|\.turbo)\//.test(lower)) return true

  return false
}

/**
 * 估算一个 commit 的"工作量分数"。
 *
 * effort(c) = 1 + sqrt(实际 loc 变更) / 10
 *
 * 设计要点：
 *   - 基础分 1：每个 commit 至少代表一次"动手"，哪怕只是空提交
 *   - sqrt：让大 commit 分高、但不让 5000 行的批量重构压死 50 个真实改动
 *   - 行数排除生成文件和二进制文件，避免锁文件 / minified 产物吃走工时
 *
 * 当 commit 没有 stat（commit-link 调用时没传 includeStat），fallback 到基础分 1。
 */
export function effortForCommit(c: LocalCommit): number {
  if (!c.stat) return 1

  let loc = 0
  if (c.stat.files && c.stat.files.length > 0) {
    for (const f of c.stat.files) {
      if (f.binary) continue
      if (isGeneratedPath(f.path)) continue
      loc += (f.insertions ?? 0) + (f.deletions ?? 0)
    }
  } else {
    // 没有 files 数组但有汇总值：保守用汇总值，可能高估但不至于 0
    loc = (c.stat.insertions ?? 0) + (c.stat.deletions ?? 0)
  }

  return 1 + Math.sqrt(Math.max(0, loc)) / 10
}
