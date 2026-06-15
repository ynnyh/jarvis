# 开源就绪检查清单（Definition of Ready for Open Source）

> 这是项目"配得上开源"的硬指标清单。由 AI 质量守门人维护。
> **所有 P0 阻断项必须全绿，才能改 Public。P1 全绿才建议公开。**
> 状态符号：✅ 完成 / ⏳ 进行中 / ❌ 未开始 / ⚠️ 有风险

---

## P0 阻断项（不绿不许 Public）

### 安全

| # | 项目 | 状态 | 验证方法 | 备注 |
|---|------|------|---------|------|
| S1 | 生产代码无敏感信息 | ✅ | `git grep -E "密码\|账号\|公司域名\|内网IP"` 无输出 | pre-public-cleanup 已做 |
| S2 | git 历史无敏感信息 | ✅ | `git log --all -S "<敏感串>"` 无输出 | filter-repo 已重写 |
| S3 | .env 不入库 | ✅ | `.gitignore` 含 `.env`；`git log -- .env` 无历史 | |
| S4 | 远程 tag 状态一致 | ❌ | 本地/远程 `git ls-remote --tags` hash 一致 | **filter-repo 后本地变了，远程没同步。同步会触发 release workflow，必须先禁用 release.yml 或等额度刷新** |
| S5 | 备份目录已清理 | ❌ | `project-agent.backup-20260615\` 已删 | 含原始密码历史，Public 前必须删 |
| S6 | GitHub 旧历史缓存已清 | ⚠️ | 通过旧 hash 访问 commit URL 返回 404 | 需联系 GitHub Support 或等 90 天 GC。单人项目可降级为 P1 |

### 质量

| # | 项目 | 状态 | 验证方法 | 备注 |
|---|------|------|---------|------|
| Q1 | ci.yml 实跑且全绿 | ❌ | push 一个 PR，3 步(check:text/cargo test/clippy)在 Win+macOS 都绿 | **本地从未验证过 CI 是否复现 cargo test 崩溃。这是最大未知数** |
| Q2 | cargo test 在 CI 稳定通过 | ❌ | 同 Q1 | 崩溃判定为开发机问题，但 CI 上没验证过 |
| Q3 | 架构拆分完成 | ❌ | voice.rs/llm.rs/App.vue 拆成模块，单文件 < 600 行 | refactor-split-modules 任务 prd 已就绪，未实现 |
| Q4 | 无遗留 eprintln | ✅ | `git grep eprintln! src/` 为 0 | observability 任务已做 |
| Q5 | clippy 干净(-D warnings) | ❌ | ci.yml clippy 步骤用 -D warnings 且绿 | 现在是 -W warnings 过渡态 |
| Q6 | 现有测试全过 | ⏳ | CI cargo test 绿 | 依赖 Q1/Q2 |

### 文档

| # | 项目 | 状态 | 验证方法 | 备注 |
|---|------|------|---------|------|
| D1 | README 是项目介绍(非自用笔记) | ❌ | 含:一句话定位/截图/快速开始/功能列表/配置/FAQ | 现在的 README 是开发笔记 |
| D2 | LICENSE 清晰 | ❌ | 明确 MIT 或 Apache-2.0，说明专利授权 | 现在 ISC，生态不熟悉 |
| D3 | 隐私政策 | ❌ | 说清:LLM 数据流向/语音数据(本地 vs 云)/密钥链存储范围 | 法律 + 信任刚需 |
| D4 | CONTRIBUTING.md | ❌ | 贡献流程/代码规范/PR 模板 | 接受外部贡献的前提 |

---

## P1 强烈建议（影响口碑，应在 Public 前做）

| # | 项目 | 状态 | 验证方法 | 备注 |
|---|------|------|---------|------|
| R1 | 首启体验:陌生人 5 分钟跑通 | ❌ | 欢迎引导完整 + 配置文档 + FAQ | WelcomeWizard 要扎实 |
| R2 | 安装文档(Win/macOS) | ❌ | 截图 + Gatekeeper 处理说明 | |
| R3 | check:text 在 CI 通过 | ✅ | ci.yml 绿 | |
| R4 | 错误提示对用户友好 | ⏳ | LLM/语音/密钥链失败有可操作提示 | tracing 落盘后可诊断，但前端提示待查 |
| R5 | CHANGELOG 整理到最新 | ⏳ | 最新版本条目完整 | |
| R6 | i18n 框架接入(可选) | ❌ | 至少 UI 文案可抽取 | 面向"所有人"的门槛 |

---

## 关卡流程

每次推进一个任务后，更新本清单对应项的状态。
**只有当全部 P0 = ✅，且 P1 完成度 ≥ 80%，守门人才签署"可以 Public"。**

签署记录：
- [ ] 第一次评估(2026-06-15)：P0 完成 4/15，P1 完成 1/6。**不允许 Public。**

---

## 当前最大风险（按优先级）

1. **Q1/Q2: ci.yml 从没跑过** —— 如果 CI 上 cargo test 也崩溃，整个测试网就是空的，必须先解决。
2. **Q3: 架构没拆** —— 外部贡献者看 1400 行的单文件会直接退出。
3. **D1/D2/D3: 文档缺失** —— README/LICENSE/隐私是开源的"门面"。
4. **S4: tag 同步** —— 操作不当会重蹈 release workflow 事故。

---

## 守门人承诺

AI 质量守门人(我)承诺：
- 不迎合用户"想赶紧开源"的冲动 —— 没准备好就说没准备好。
- 每个 P0 项我亲自用文档里写的"验证方法"验证，给截图/命令输出，不靠嘴说。
- 全绿了我才签署放行；有一项没过，明确指出差在哪、怎么补。
- 每次重大改动后重新评估，避免"刚绿又红"。
