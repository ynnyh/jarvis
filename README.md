# Project Agent

基于禅道（ZenTao）的 AI 任务助手框架，通过 CLI 和桌面端为团队提供任务查询、风险分析和工作流自动化能力。

## 快速开始

```bash
# 1. 安装依赖
npm install

# 2. 配置环境变量（复制并编辑）
cp .env.example .env

# 3. 构建
npm run build

# 4. 使用
npx agent tasks       # 查看所有指派给我的任务
npx agent today       # 查看今日到期任务
npx agent task 10259  # 查看单个任务详情
npx agent analyze     # 运行风险分析
```

## 环境变量

在项目根目录创建 `.env` 文件：

```env
ZENTAO_BASE_URL=http://your-zentao-server/zentao
ZENTAO_ACCOUNT=your_username
ZENTAO_PASSWORD=your_password
```

支持的变量名（两种写法均可）：

| 变量 | 备选 | 说明 |
|------|------|------|
| `ZENTAO_BASE_URL` | `ZENTAO_URL` | 禅道服务器地址，末尾需包含 `/zentao` |
| `ZENTAO_ACCOUNT` | `ZENTAO_USER` | 登录账号 |
| `ZENTAO_PASSWORD` | `ZENTAO_PASS` | 登录密码 |
| `ZENTAO_TOKEN` | - | 可选，预设的 API Token |
| `USE_MOCK` | - | 设为 `true` 使用模拟数据 |

## CLI 命令

### `agent` — 任务管理命令

```bash
agent tasks           # 列出所有指派给我的任务
agent today           # 列出今日截止的任务
agent task <id>       # 查看指定任务详情（含评论）
agent analyze         # 风险分析（延期、高优先级、依赖风险）
```

### `agent-core` — 高级框架命令

```bash
agent-core tools                  # 列出所有已注册工具
agent-core tool <name> [json]     # 执行指定工具
agent-core actions                # 列出所有预定义工作流
agent-core action <id>            # 执行工作流
agent-core memory add <type> <content> [tags] [importance]
agent-core memory list            # 查看所有记忆
agent-core memory stats           # 记忆统计
agent-core context                # 构建 AI 上下文
agent-core git                    # 查看 Git 仓库信息
agent-core state                  # 查看状态机状态
agent-core scheduler              # 查看调度器状态
agent-core start                  # 启动调度器（常驻运行）
```

## 项目架构

```
src/
├── cli/
│   ├── index.ts            # agent CLI 入口（任务管理）
│   └── agent-core.ts       # agent-core CLI 入口（完整框架）
├── core/
│   └── tool-registry.ts    # 工具注册中心
├── providers/
│   ├── base-provider.ts    # Provider 基类（抽象接口）
│   ├── zentao-provider.ts  # 禅道 Provider（主实现）
│   ├── mock-provider.ts    # 模拟数据 Provider
│   └── zentao/             # 禅道子模块（备用实现）
├── services/
│   └── task-service.ts     # 任务服务层（业务逻辑）
├── tools/
│   ├── get-tasks.ts        # 工具：获取任务列表
│   ├── get-today-tasks.ts  # 工具：获取今日任务
│   ├── get-task-detail.ts  # 工具：获取任务详情
│   └── analyze-risk.ts     # 工具：风险分析
├── actions/
│   └── predefined-actions.ts  # 预定义工作流
├── scheduler/              # Cron 调度器
├── memory/                 # 持久化记忆存储
├── events/                 # 事件总线
├── ai/                     # AI 上下文构建
└── shared/
    └── types.ts            # 共享类型定义
desktop/                    # Vue 3 + Tauri 桌面端
scripts/                    # 调试/测试脚本
```

### 分层设计

```
CLI / 桌面端
    ↓
Tools（工具层）        ← 注册到 ToolRegistry，Zod 校验输入
    ↓
Services（服务层）     ← TaskService，业务逻辑
    ↓
Providers（数据层）    ← ZenTaoProvider，对接外部 API
```

## 工具列表

| 工具名 | 输入参数 | 说明 |
|--------|----------|------|
| `get_tasks` | `status?`, `assignee?` | 获取所有任务，支持按状态/负责人过滤 |
| `get_today_tasks` | 无 | 获取今日截止的任务 |
| `get_task_detail` | `id: string` | 获取单个任务详情 |
| `analyze_risk` | 无 | 分析延期、高优先级和依赖风险 |

## 预定义工作流

| ID | 名称 | 说明 |
|----|------|------|
| `start_today_work` | 开始今日工作 | 获取今日任务 → 风险分析 → 获取详情 |
| `pre_commit_check` | 提交前检查 | Git 状态 → 风险分析 |
| `periodic_risk_check` | 定期风险检查 | 每 10 分钟自动执行 |
| `generate_daily_report` | 生成日报 | 今日任务 → 风险分析 → Git 状态 |
| `task_context_switch` | 任务上下文切换 | 任务详情 → Git 状态 |

## 禅道集成说明

### 数据获取方式

任务数据通过禅道工作台 `.json` 端点获取：

```
GET {BASE_URL}/my-work-task-assignedTo--id_desc.json
Header: Token: {API_TOKEN}
Cookie: pagerMyWork=200
```

- 先调用 `POST /api.php/v1/tokens` 获取 Token
- 使用 Token 访问工作台 `.json` 端点
- 通过 `pagerMyWork=200` Cookie 设置每页条数，一次性获取全部任务
- 返回数据与禅道界面上"指派给我"页面完全一致

### 关键实现（zentao-provider.ts）

```typescript
// 1. 认证：获取 API Token
POST {baseUrl}/api.php/v1/tokens
Body: { account, password }
→ { token, expires }

// 2. 获取任务：通过工作台 JSON 端点
GET {baseUrl}/my-work-task-assignedTo--id_desc.json
Headers: Token: {token}, Cookie: pagerMyWork=200
→ { status: "success", data: "{JSON字符串}" }

// 3. 解析 data 字段（是字符串化的 JSON）
const innerData = JSON.parse(json.data);
// innerData.tasks  → 任务数组
// innerData.pager  → 分页信息（recTotal, pageTotal）
```

### 不要使用的旧方式（已废弃）

~~遍历所有 executions 再按 assignedTo 过滤~~ — 会返回用户参与的所有执行中的任务（294 条），而非禅道"指派给我"页面的正确数据（76 条）。

## 桌面端

基于 Vue 3 + Tauri，与 CLI 共享同一套 Provider。

```bash
npm run desktop:dev    # 开发模式（端口 5174）
npm run desktop:build  # 构建 Tauri 应用
```

### macOS 发版说明

macOS 用户如果看到 `"Jarvis.app" 已损坏，无法打开。你应该将它移到废纸篓。`，通常不是包真的损坏，而是 Gatekeeper 拒绝了未签名、未公证，或架构不匹配的应用。当前 CI 会构建 `universal-apple-darwin`，同一个 DMG 兼容 Intel Mac 和 Apple Silicon Mac。

当前项目按内部开发包分发，不配置正式 Apple Developer ID 签名/公证。内部用户推荐用安装脚本安装，它会下载 DMG、复制到 `/Applications`，并清理浏览器下载带来的 quarantine 标记：

```bash
curl -fsSL https://gitee.com/ynnyh/jarvis/releases/download/v0.6.4/install-macos-dev.sh | bash
```

如果要安装指定版本：

```bash
curl -fsSL https://gitee.com/ynnyh/jarvis/releases/download/v0.6.4/install-macos-dev.sh | JARVIS_VERSION=v0.6.4 bash
```

如果用户已经手动拖拽安装过，可以临时执行：

```bash
xattr -dr com.apple.quarantine /Applications/Jarvis.app
open /Applications/Jarvis.app
```

推荐在 GitHub Actions secrets 配置以下变量，让 CI 自动进行 Developer ID 签名和公证：

| Secret | 说明 |
|--------|------|
| `APPLE_CERTIFICATE` | Developer ID Application 证书 `.p12` 的 base64 内容 |
| `APPLE_CERTIFICATE_PASSWORD` | `.p12` 导出密码 |
| `KEYCHAIN_PASSWORD` | CI 临时 keychain 密码 |
| `APPLE_ID` | Apple 开发者账号邮箱 |
| `APPLE_PASSWORD` | App 专用密码 |
| `APPLE_TEAM_ID` | Apple Team ID |

没有 Apple 证书时，CI 会退回 ad-hoc 签名。它能改善部分 Apple Silicon 启动问题，但浏览器下载后仍可能被 Gatekeeper 拦截；正式分发给外部用户时应使用 Developer ID 签名和公证。

### 发版托底方案

默认发版走 GitHub Actions；CircleCI 是手动保底，不会在推 tag 时自动抢跑。只有当 GitHub Actions 免费额度耗尽或临时失败时，才在 CircleCI 手动触发 `run_release=true` 的 pipeline。CircleCI 使用独立免费 credits，会分别构建 Windows 和 macOS universal 包，再上传到 Gitee Release。

CircleCI 项目环境变量需要配置：

| 变量 | 说明 |
|------|------|
| `GITEE_TOKEN` | Gitee 私人访问令牌，用于创建 release、上传附件和写 `latest.json` |
| `TAURI_SIGNING_PRIVATE_KEY` | Tauri updater 私钥 |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | Tauri updater 私钥密码 |
| `GITEE_OWNER` | 可选，默认 `ynnyh` |
| `GITEE_REPO` | 可选，默认 `jarvis` |

如果 GitHub Actions 和 CircleCI 都不可用，最后兜底是本地打包后上传 Gitee：

CircleCI 手动触发方式：

1. 进入 CircleCI 项目页面。
2. 点击 **Trigger Pipeline**。
3. Branch 填要发布的分支，通常是 `main`。
4. 添加 boolean 参数 `run_release=true`。
5. 运行后产物会上传到 Gitee Release，并更新 `latest.json`。

```bash
# Windows 机器
npm ci
npm run desktop:build
$env:GITEE_TOKEN="..."
npm run release:gitee

# Mac 机器
rustup target add aarch64-apple-darwin x86_64-apple-darwin
npm ci
npm run desktop:build-macos-dev
GITEE_TOKEN="..." npm run release:gitee
```

## 开发

```bash
npm run build          # TypeScript 编译
npm run dev            # 开发模式（tsx 直接运行）
npm run start          # 生产模式（运行编译产物）

# 调试脚本
npx tsx scripts/xxx.ts
```

## 调试脚本

`scripts/` 目录下有大量调试和测试脚本，常用：

| 脚本 | 说明 |
|------|------|
| `fetch-my-assigned-tasks.ts` | 获取并输出全部指派任务 |
| `analyze-html.ts` | 分析禅道页面 HTML 结构 |
| `debug-login.ts` | 调试禅道登录流程 |
| `probe-zentao*.ts` | 探索禅道 API 端点 |
