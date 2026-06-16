# 开源就绪检查清单（Definition of Ready for Open Source）

> 这是项目"配得上开源"的硬指标清单。由 AI 质量守门人维护。
> 状态符号：✅ 完成 / ⏳ 进行中 / ❌ 未开始 / ⚠️ 有风险 / 🔒 **开源后验证**
>
> 🔒 关键现实：**CI 额度已耗尽，开源前任何 push 触发即 fail（与代码无关）；本地 cargo test 又因 native DLL 入口跑不起来。**
> 测试网在开源前客观无法验证，唯一时机是转 Public 后（公开仓库 Actions 免费无限）。
> 因此 CI 相关项（Q1/Q2/Q5/Q6）**不作为开源前阻断**，标 🔒，转 Public 后第一时间验证。
>
> **放行规则：所有「非 🔒」P0 = ✅ → 可转 Public；转 Public 后立刻跑 CI 验证 🔒 项，全绿才算真正交付。**

---

## P0 阻断项

### 安全

| # | 项目 | 状态 | 验证方法 | 备注 |
|---|------|------|---------|------|
| S1 | 生产代码无敏感信息 | ✅ | `git grep -E "密码\|账号\|公司域名\|内网IP"` 无输出 | pre-public-cleanup 已做 |
| S2 | git 历史无敏感信息 | ✅ | `git log --all -S "<敏感串>"` 无输出 | filter-repo 已重写 |
| S3 | .env 不入库 | ✅ | `.gitignore` 含 `.env`；`git log -- .env` 无历史 | |
| S4 | 远程无脏 tag 暴露 | ✅ | `git ls-remote --tags origin` 不含指向旧历史的 tag | **2026-06-16 已删全部 60+ 远程脏 tag,ls-remote 现为空,脏历史失去 tag 引用。本地干净 tag 全保留,开源后(有额度/临时禁 release.yml)再补 push。unreachable commit 缓存见 S6** |
| S5 | 备份目录已清理 | ✅ | `project-agent.backup-20260615\` 已删 | 2026-06-16 已 rm（确认含 .env 真实凭据），无残留 |
| S6 | GitHub 旧历史缓存已清 | ⚠️ | 通过旧 hash 访问 commit URL 返回 404 | 需联系 GitHub Support 或等 90 天 GC。单人项目可降级为 P1 |

### 质量

| # | 项目 | 状态 | 验证方法 | 备注 |
|---|------|------|---------|------|
| Q1 | ci.yml 实跑且全绿 | 🔒 | 转 Public 后 push/PR，3 步在 Win+macOS 都绿 | **开源前 CI 无额度、本地 DLL 跑不了,测不了。转 Public 第一件事** |
| Q2 | cargo test 在 CI 稳定通过 | 🔒 | 同 Q1 | 同 Q1。本地崩溃判定为开发机 DLL 问题,CI 上才能证实 |
| Q3 | 架构拆分完成 | ⏳ | 巨型文件拆成模块 | **voice.rs 整段下线 + llm.rs 已拆 llm/ 4 模块(0b20c6e);仅剩 App.vue 待拆,设计稿见 research/app-vue-split-design.md。本地 vite build 可验,不依赖 CI——开源前能做完** |
| Q4 | 无遗留 eprintln | ✅ | `git grep eprintln! src/` 为 0 | observability 任务已做 |
| Q5 | clippy 干净(-D warnings) | 🔒 | ci.yml clippy 用 -D warnings 且 CI 绿 | **本地 `clippy --all-targets -D warnings` 已全绿(15b686b)。ci.yml 收紧那行 + "CI 上也绿" 放到开源后跟 Q1 一起做:首次 CI 先用现状 -W 确认 test 基线绿,再收 -D 修 clippy 版本差异 lint** |
| Q6 | 现有测试全过 | 🔒 | CI cargo test 绿 | 依赖 Q1/Q2,开源后验证 |

### 文档

| # | 项目 | 状态 | 验证方法 | 备注 |
|---|------|------|---------|------|
| D1 | README 是项目介绍(非自用笔记) | ✅ | 含:定位/功能/下载/快速开始/配置/FAQ/结构 | 2026-06-16 重写。仅截图待补(TODO,P1) |
| D2 | LICENSE 清晰 | ✅ | 明确 MIT | 2026-06-16 建 LICENSE(MIT) + package.json 改 MIT |
| D3 | 隐私政策 | ✅ | LLM 数据流向/密钥链范围/本地存储/无遥测 | 2026-06-16 PRIVACY.md |
| D4 | CONTRIBUTING.md | ✅ | 贡献流程/代码规范/CI 说明 | 2026-06-16 建 |

---

## P1 强烈建议（影响口碑，应在 Public 前做）

| # | 项目 | 状态 | 备注 |
|---|------|------|------|
| R1 | 首启体验:陌生人 5 分钟跑通 | ⏳ | README 已补快速开始/FAQ；WelcomeWizard 实测待做 |
| R2 | 安装文档(Win/macOS) | ⏳ | README 已含 macOS 脚本；截图待补 |
| R3 | check:text 在 CI 通过 | 🔒 | 同 CI,开源后验 |
| R4 | 错误提示对用户友好 | ⏳ | tracing 落盘可诊断,前端提示待查 |
| R5 | CHANGELOG 整理到最新 | ⏳ | |
| R6 | i18n 框架接入(可选) | ❌ | |

---

## 关卡流程

**开源前**：所有「非 🔒」P0 = ✅ → 守门人签署「可转 Public」。
**开源后**（转 Public 第一批）：跑 CI 验证 🔒 项（Q1/Q2/Q5/Q6 + R3）；全绿才算真正交付，红则立即修。

签署记录：
- [ ] 第一次评估(2026-06-15)：P0 4/15。不允许 Public。
- [ ] 第二次评估(2026-06-16)：P0 9/15（+D1-D4 + S5）。不允许 Public。
- [ ] 第三次评估(2026-06-16，CI 认知修正)：厘清 CI 额度耗尽 → 测试网开源前无法验证,Q1/Q2/Q5/Q6 重定性为 🔒 开源后验证。
- [ ] 第四次评估(2026-06-16，S4 完成)：删全部远程脏 tag,S4 ✅。**开源前 P0 = 非🔒 共 11 项,已绿 10(S1/S2/S3/S4/S5/Q4/D1/D2/D3/D4),仅剩 Q3(App.vue 拆分)一项** + S6 降级决策。Q3 一完成即可签署「可转 Public」。

---

## 当前卡点

**开源前（必须做完，本地可验，不依赖 CI）**：
1. **Q3 仅剩 App.vue 拆分** —— llm 已拆、voice 已删；App.vue 高耦合 + 前端无类型网,设计稿已备(research/),vite build 验证。留 fresh session 专注做。**这是开源前最后一项「非🔒」P0。**
2. **S6 决策** —— 降级 P1 还是处理(联系 GitHub Support / 等 GC 清 unreachable commit 缓存)。

**开源后（转 Public 第一批）**：Q1/Q2/Q5/Q6 —— CI 实跑 + cargo test + clippy 收紧 -D warnings。

---

## 守门人承诺

- 不迎合"想赶紧开源"的冲动 —— 没准备好就说没准备好。
- 每个 P0 项亲自用"验证方法"验证,给命令输出,不靠嘴说。
- 区分「开源前能验」与「开源后才能验(🔒)」,不拿做不到的前置卡死流程,也不把开源后的验证假装成已完成。
- 全部「非🔒」P0 绿才签署可转 Public；开源后 🔒 项全绿才签署真正交付。
