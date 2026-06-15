# Public 前安全清理

> 阶段一(内功期)的预备任务。目标:清除仓库里的敏感信息(禅道密码/账号、公司内网地址),
> 使代码"配得上开源"的安全维度。本任务做完**不改 Public**,等阶段一全部完成后再公开。
>
> 性质:**破坏性操作**(批量删除 + git 历史重写),需谨慎执行 + 备份。

---

## 1. 背景

Public(Public 仓库 → GitHub Actions 无限免费)是解决发版额度问题的最终方案,
但当前仓库含敏感信息,直接 Public 会泄露:
- 禅道真实密码(`REDACTED_PASSWORD`)
- 禅道账号(`REDACTED_ACCOUNT`,遍布 ~50 处)
- 公司内网地址(`REDACTED_DOMAIN/19085`)
- 这些信息**同时在 git 历史里**(密码只在初始 commit `6ce769b`,公司域名在 2 个 commit)

## 2. 审计结论(已完成)

### 生产代码:干净 ✅
- `src-tauri/` / `desktop/src/` 的 token/secret 全走密钥链 + `SECRET_PLACEHOLDER`,无明文。
- `.env`(含真实密码)未被 git 追踪,历史里也没进过。
- `.env.example` 是干净占位符。

### scripts/ 目录:重灾区 🔴
- 73 个文件,其中 65 个 `.ts` 是禅道 API 探针/调试脚本(V2 开发期摸 API 用)。
- 后端 Rust 化后这些脚本已**无任何价值**,逻辑都在 `src-tauri/src/zentao.rs`。
- 含敏感信息:`find-my-tasks.ts` 硬编码明文密码,20+ 个脚本含禅道账号。

### 公司 URL:2 处 🟡
- `desktop/src/stores/config.ts:195` —— 帆软 baseUrl 默认值
- `desktop/src/components/settings/FineReportSection.vue:334` —— placeholder

### git 历史:3 个敏感字符串
| 字符串 | 引入 commit | 出现位置 |
|---|---|---|
| `REDACTED_PASSWORD`(密码) | `6ce769b`(V2 baseline) | scripts/find-my-tasks.ts |
| `REDACTED_DOMAIN`(公司域名) | `6ce769b` + `d4dfa43` | scripts/ + desktop/ 2 处 |
| `REDACTED_ACCOUNT`(账号) | 多个 commit | 21 个 scripts/*.ts |

## 3. 方案(用户已确认)

### 历史处理:**git filter-repo 替换敏感字符串**
- 用 `git filter-repo --replace-text` 把历史所有 commit 里的敏感字符串替换成占位符。
- 保留完整提交历史(不像"重新初始化"那样丢历史)。
- 副作用:所有 commit hash 改变 → 必须 force push → 远程旧引用要清理。
- 单人项目,影响可控。

### 安全保障:**先备份再操作**
- 开始前把当前仓库(含完整 .git 历史)整体复制一份到仓库外。
- filter-repo 不可逆,备份是唯一回滚保证。

## 4. 执行步骤

### Step 0:前置准备
1. 安装 git-filter-repo:`pip install git-filter-repo`(Python 3.12 已有)。
2. 确认本地工作树干净(`git status`)。
3. 确认远程同步(`git fetch`,本地不落后远程)。

### Step 1:备份
- 把整个项目目录(含 `.git`)复制到 `D:\coding\my-mcp-servers\project-agent.backup-YYYYMMDD\`。
- 验证备份可独立 `git log`(确认历史完整)。

### Step 2:删除 65 个探针脚本
保留以下 8 个有用脚本,删除其余所有 `scripts/*.ts` 探针:
- `scripts/ci/prepare-tauri-signing.mjs`(CI 签名)
- `scripts/dev.mjs`(开发启动)
- `scripts/install-macos-dev.sh`(macOS 安装)
- `scripts/portable-zip.mjs`(便携版打包)
- `scripts/pre-release.mjs`(发版前处理)
- `scripts/publish-to-gitee.mjs`(Gitee 发布)
- `scripts/scrape-my-tasks.ts`(读 env,干净,可保留)
- `scripts/check-mojibake.mjs`(CI 乱码审计)

**判断规则**:`scripts/*.ts` 里凡含 `REDACTED_ACCOUNT` / `REDACTED_DOMAIN` / 硬编码 password 的,一律删除(它们都是探针)。
对不含敏感信息但名字像探针的 `.ts`(如纯工具),逐一判断,有疑问保留并在 info.md 记录。

### Step 3:清理 2 处公司 URL
- `desktop/src/stores/config.ts:195`:帆软 baseUrl 默认值 `http://REDACTED_DOMAIN` → `https://your-fine-report.example.com`
- `desktop/src/components/settings/FineReportSection.vue:334`:placeholder 同上替换
- 检查 `config.ts:67` 的注释里是否也含公司域名,一并清理

### Step 4:git filter-repo 历史重写
创建替换规则文件(如 `.trellis/tasks/06-15-pre-public-cleanup/replacements.txt`):
```
REDACTED_PASSWORD==>REDACTED_PASSWORD
REDACTED_ACCOUNT==>REDACTED_ACCOUNT
REDACTED_DOMAIN==>REDACTED_DOMAIN
REDACTED_DOMAIN==>REDACTED_DOMAIN
REDACTED_DOMAIN==>REDACTED_DOMAIN
```

执行:
```bash
git filter-repo --replace-text .trellis/tasks/06-15-pre-public-cleanup/replacements.txt
```

### Step 5:验证
1. `git grep -E "REDACTED_ACCOUNT|REDACTED_DOMAIN|Aa123"` → 应无任何输出(当前代码干净)。
2. 在历史里搜:`git log --all -S "REDACTED_PASSWORD"` → 应无输出(历史也清了)。
3. `git log --all -S "REDACTED_DOMAIN"` → 应无输出。
4. 随机抽查几个历史 commit:`git show <hash> | grep -i REDACTED_DOMAIN` → 应为 REDACTED。
5. 确认有用脚本仍在,构建相关脚本未被误删。
6. `npm run check:text` 仍通过。
7. 构建冒烟:`cargo check --manifest-path src-tauri/Cargo.toml`(确认删 scripts 没破坏构建)。

### Step 6:force push(谨慎,需用户在场确认)
- filter-repo 会移除 `origin` remote(安全机制),需重新添加。
- `git remote add origin <url>`
- `git push --force origin main`
- 清理 GitHub/Gitee 远程的旧引用(否则历史还能从 PR/分支引用里挖出来):
  - GitHub:Settings → 删除所有旧分支引用;联系 GitHub Support 或用 API 清理引用(可选,高安全场景才需要)。
  - 对本项目:单人项目,无外部 PR/分支,force push 后基本干净。

## 5. 范围

### In scope
- 删除 65 个 `scripts/*.ts` 探针脚本。
- 清理 2-3 处 `desktop/` 公司 URL。
- git filter-repo 历史重写(替换 3-5 个敏感字符串)。
- force push 到远程(需用户确认)。
- 备份 + 验证。

### Out of scope
- **不改 Public**(等阶段一全部完成后再改)。
- 不补 README / LICENSE / CONTRIBUTING(那是阶段三开源期的任务)。
- 不重写 `scraper-my-tasks.ts`(它读 env,干净,保留)。
- 不审计二进制资源(icons、vendor 等 —— 它们不含文本敏感信息)。

## 6. 验收标准

| # | 条件 | 验证方式 |
|---|------|---------|
| 1 | 65 个探针脚本已删,8 个有用脚本保留 | `git ls-files scripts/` |
| 2 | desktop/ 公司 URL 改为占位符 | `git grep REDACTED_DOMAIN` 无输出 |
| 3 | 当前代码无敏感字符串 | `git grep -E "REDACTED_ACCOUNT\|REDACTED_DOMAIN\|Aa123"` 无输出 |
| 4 | **git 历史无敏感字符串** | `git log --all -S "REDACTED_PASSWORD"` 无输出;`git log --all -S "REDACTED_DOMAIN"` 无输出 |
| 5 | 备份存在且完整 | 备份目录可独立 `git log` |
| 6 | 构建未破坏 | `cargo check` + `npm run check:text` 通过 |
| 7 | force push 完成(用户确认后) | `git log origin/main` 与本地一致 |

## 7. 风险

| 风险 | 应对 |
|------|------|
| filter-repo 误操作丢历史 | Step 1 备份;filter-repo 也会在 `.git/filter-repo/` 留原始引用备份 |
| force push 后协作者 clone 冲突 | 单人项目,无协作者;但有 Gitee 镜像,需同步 force push |
| 删 scripts 误伤有用脚本 | 保留清单明确(8 个);有疑问的保留并记录 |
| 远程旧引用仍可挖出敏感信息 | 单人项目无外部 PR;如需彻底,GitHub Support 可清缓存(高安全场景) |
| commit hash 全变,CHANGELOG/文档引用失效 | 检查 CHANGELOG 是否引用具体 hash;本仓库 CHANGELOG 按版本号组织,不引用 hash |

## 8. 不做功能

遵循"阶段一不新增功能"。本任务只做安全清理 + 历史重写,不加任何用户可见功能。
