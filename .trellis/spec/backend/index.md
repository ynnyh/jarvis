# Backend Development Guidelines

> Best practices and contracts for Rust backend development.

---

## Specs Index

| Spec | Description | Status |
|------|-------------|--------|
| [FineReport Integration](./fine-report-integration.md) | FR client patterns, timeout tiers, data source | Active |
| [Cost Analysis](./cost-analysis.md) | Project cost aggregation, overtime splitting, channel wiring | Active |
| [Memory System](./memory-system.md) | 记忆存储契约、FTS 更新顺序、嵌入降级、去重策略 | Active |
| [MCP Client 接入](./mcp-client.md) | rmcp stdio client 范式、mcp-servers.json 合约、两层错误、env 注入坑 | Active |
| [MCP 工具接入 agent](./mcp-agent-integration.md) | 工具注入 agent、`mcp__` 命名空间路由、动态安全分类（toolPolicy>annotations>默认confirm）、门禁红线 | Active |
| [对话式发版确认流](./mcp-deploy-confirm.md) | prepare/confirm 拆分、deploy-presets 合约、环境显式回显、trigger_build 参数形态、keychain 注入 | Active |

---

## Language

All documentation should be written in **Chinese** where it helps readability.
