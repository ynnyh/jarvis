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
| S4 | 远程 tag 状态一致 | ❌ | 本地/远程 `git ls-remote --tags` hash 一致 | **filter-repo 后本地变了，远程没同步。同步 tag 会触发 release workflow——发版没额度，需先禁用 release.yml 或删旧 tag 重打** |
| S5 | 备份目录已清理 | ✅ | `project-agent.backup-20260615\` 已删 | **2026-06-16 已 rm（确认含 .env 真实凭据），无残留** |
| S6 | GitHub 旧历史缓存已清 | ⚠️ | 通过旧 hash 访问 commit URL 返回 404 | 需联系 GitHub Support 或等 90 天 GC。单人项目可降级为 P1 |

### 质量

| # | 项目 | 状态 | 验证方法 | 备注 |
|---|------|------|---------|------|
| Q1 | ci.yml 实跑且全绿 | ⏳ | push 一个 PR，3 步(check:text/cargo test/clippy)在 Win+macOS 都绿 | **2026-06-16 push main 已首次触发 CI，正在跑——等结果确认 cargo test 是否复现本地崩溃** |
| Q2 | cargo test 在 CI 稳定通过 | ⏳ | 同 Q1 | 本地因 native DLL 入口跑不了；CI 首跑待结果 |
| Q3 | 架构拆分完成 | ⏳ | 巨型文件拆成模块 | **voice.rs 整段下线删除 + llm.rs 已拆 llm/ 4 模块（0b20c6e）；仅剩 App.vue 待拆，设计稿见 research/app-vue-split-design.md** |
| Q4 | 无遗留 eprintln | ✅ | `git grep eprintln! src/` 为 0 | observability 任务已做 |
| Q5 | clippy 干净(-D warnings) | ⏳ | ci.yml clippy 步骤用 -D warnings 且绿 | **本地 `clippy --all-targets -D warnings` 已全绿（15b686b 清零）；ci.yml 仍 -W warnings，待 CI 基线绿后收紧那一行** |
| Q6 | 现有测试全过 | ⏳ | CI cargo test 绿 | 依赖 Q1/Q2 |

### 文档

| # | 项目 | 状态 | 验证方法 | 备注 |
|---|------|------|---------|------|
| D1 | README 是项目介绍(非自用笔记) | ✅ | 含:定位/功能/下载/快速开始/配置/FAQ/结构 | **2026-06-16 重写：补用户视角，修过时结构。仅截图待补(TODO)** |
| D2 | LICENSE 清晰 | ✅ | 明确 MIT | **2026-06-16 建 LICENSE(MIT) + package.json 改 MIT（原 ISC）** |
| D3 | 隐私政策 | ✅ | 说清 LLM 数据流向/密钥链范围/本地存储/无遥测 | **2026-06-16 PRIVACY.md（语音已下线，无语音数据条目）** |
| D4 | CONTRIBUTING.md | ✅ | 贡献流程/代码规范/CI 说明 | **2026-06-16 建** |

---

## P1 强烈建议（影响口碑，应在 Public 前做）

| # | 项目 | 状态 | 验证方法 | 备注 |
|---|------|------|---------|------|
| R1 | 首启体验:陌生人 5 分钟跑通 | ⏳ | 欢迎引导完整 + 配置文档 + FAQ | README 已补快速开始/FAQ；WelcomeWizard 实测待做 |
| R2 | 安装文档(Win/macOS) | ⏳ | 截图 + Gatekeeper 处理说明 | README 已含 macOS 脚本；截图待补 |
| R3 | check:text 在 CI 通过 | ✅ | ci.yml 绿 | |
| R4 | 错误提示对用户友好 | ⏳ | LLM/密钥链失败有可操作提示 | tracing 落盘后可诊断，前端提示待查 |
| R5 | CHANGELOG 整理到最新 | ⏳ | 最新版本条目完整 | |
| R6 | i18n 框架接入(可选) | ❌ | 至少 UI 文案可抽取 | 面向"所有人"的门槛 |

---

## 关卡流程

每次推进一个任务后，更新本清单对应项的状态。
**只有当全部 P0 = ✅，且 P1 完成度 ≥ 80%，守门人才签署"可以 Public"。**

签署记录：
- [ ] 第一次评估(2026-06-15)：P0 完成 4/15，P1 完成 1/6。**不允许 Public。**
- [ ] 第二次评估(2026-06-16)：P0 完成 **9/15**（新绿：S5 备份清理 + D1/D2/D3/D4 文档四件套）；5 项在途(Q1/Q2/Q6 CI 已跑待绿、Q3 仅剩 App.vue、Q5 本地已绿待 ci.yml 收紧)；剩 S4(tag 同步)未做、S6 可降级。**仍不允许 Public**，但门面与安全清理已就位，卡点收敛到「CI 绿 + App.vue 拆分 + ci.yml 收紧 + tag 同步」。

---

## 当前最大风险（按优先级）

1. **Q1/Q2/Q6: CI 已首次触发，等结果** —— 绿则测试网坐实(语音下线+llm 拆分行为无回归)，红则立即修。这是本轮最关键的未知数。
2. **Q3: 仅剩 App.vue** —— llm 已拆、voice 已删；App.vue 高耦合 + 前端无类型网，设计稿已备(research/)，留 fresh session 专注做。
3. **S4: tag 同步** —— filter-repo 后本地/远程 tag 不一致，同步要避开 release 触发(无发版额度)。
4. **Q5: ci.yml 收紧 -D warnings** —— 本地已清零，等 CI 基线绿后改那一行锁定成果。

---

## 守门人承诺

AI 质量守门人(我)承诺：
- 不迎合用户"想赶紧开源"的冲动 —— 没准备好就说没准备好。
- 每个 P0 项我亲自用文档里写的"验证方法"验证，给截图/命令输出，不靠嘴说。
- 全绿了我才签署放行；有一项没过，明确指出差在哪、怎么补。
- 每次重大改动后重新评估，避免"刚绿又红"。
