# Jarvis

个人桌面 AI 助手，基于 Tauri v2 (Rust) + Vue 3，集成禅道任务管理、帆软工时、多渠道消息（Telegram / QQ）和 LLM 对话。

## 功能

- 禅道任务查询、风险分析、工时写回
- 帆软 BI 工时明细解析
- Telegram / QQ 多渠道消息收发
- LLM 对话 + 工具调用（agent 模式）
- Git commit 自动归因到任务
- 自动更新（Gitee 托管 + tauri-plugin-updater）
- macOS / Windows 双平台

## 开发

```bash
npm ci
npm run check:text         # 主代码路径编码审计（CI 同步执行）
npm run check:text:full    # 仓库深扫（排除资产/抓包样本）
npm run desktop:dev        # 启动开发模式（端口 5174）
```

## 构建

```bash
npm run desktop:build              # Windows NSIS 安装包
npm run desktop:build-macos-dev    # macOS universal（dev 签名）
npm run desktop:portable           # 本地便携版（不需要签名 key）
```

## 发版

推 tag `vX.Y.Z` 触发 GitHub Actions 自动构建 + 发布到 Gitee。

```bash
# 1. 同步版本号（建议三处一致）
#    src-tauri/tauri.conf.json  "version": "X.Y.Z"
#    src-tauri/Cargo.toml        version = "X.Y.Z"
#    package.json                "version": "X.Y.Z"

# 2. 提交 + 推送
git commit -m "release: bump version to X.Y.Z"
git tag vX.Y.Z
git push origin main
git push origin vX.Y.Z
```

详见 `.github/workflows/release.yml` 和 `scripts/publish-to-gitee.mjs`。

## 项目结构

```
src-tauri/              # Rust 后端（Tauri 插件 + 业务逻辑）
├── src/
│   ├── commands.rs         # Tauri 命令（前端调用入口）
│   ├── chat_agent.rs       # LLM agent 循环 + 工具调度
│   ├── tools.rs            # 工具定义（禅道、帆软、Git 等）
│   ├── zentao.rs           # 禅道 API 客户端
│   ├── fine_report.rs      # 帆软 BI 工时解析
│   ├── daily_review.rs     # 日报 / 复盘
│   ├── channels/           # Telegram / QQ 消息通道
│   └── settings.rs         # 配置管理
desktop/                # Vue 3 前端
├── src/
│   ├── App.vue             # 主窗口
│   ├── components/         # UI 组件
│   └── composables/        # 组合式函数
scripts/                # CI / 构建脚本
```

## macOS 安装

未签名应用首次打开会被 Gatekeeper 拦截，用安装脚本自动处理：

```bash
curl -fsSL https://gitee.com/ynnyh/jarvis/releases/download/v0.7.4/install-macos-dev.sh | bash
```
