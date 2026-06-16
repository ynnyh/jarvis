# Jarvis

> 个人桌面 AI 助手 —— 一只待在桌面角落的小宠物，帮你打理禅道任务、工时、日报和对话式操作。

基于 **Tauri v2 (Rust) + Vue 3**，本地优先：数据存在你自己机器的 `~/.jarvis/`，密钥走操作系统钥匙链，不上传任何第三方服务器（除你自己配置的禅道 / 帆软 / LLM）。

<!-- TODO(截图): 补一张桌宠 + 任务气泡 + 设置页的截图，放这里 -->

## 功能

- **禅道任务**：任务查询、到期/逾期提醒、风险分析、工时写回禅道
- **帆软工时**：解析帆软 BI 工时明细，按本人过滤汇总
- **LLM 对话 + Agent**：OpenAI 兼容（DeepSeek / Moonshot / Qwen / Claude 等），支持工具调用，可对话式触发禅道操作、Jenkins 发版等
- **Git 归因**：扫描本地 git 提交，自动关联到任务，按代码量反推工时
- **多渠道消息**：Telegram / QQ Bot 收发（白名单 + 主动通知）
- **桌宠 UI**：可自定义形象（Lottie / 图片 / GIF），状态气泡提醒
- **本地优先 + 自动更新**：配置明文可编辑，密钥进 OS 钥匙链；Windows / macOS 双平台

## 下载安装

从 [Releases](https://gitee.com/ynnyh/jarvis/releases) 下载对应平台的安装包：

- **Windows**：`.exe`（NSIS 安装包），双击安装
- **macOS**：`.dmg`（Universal），拖入 Applications

> macOS 未签名应用首次打开会被 Gatekeeper 拦截，运行安装脚本自动处理：
> ```bash
> curl -fsSL https://gitee.com/ynnyh/jarvis/releases/latest/download/install-macos-dev.sh | bash
> ```

## 首次配置

启动后，配置不完整时会自动弹出**欢迎引导**，依次填：

1. **禅道地址 + 账号**（如 `http://zentao.example.com/zentao`）——密码存进 OS 钥匙链
2. **代码目录**——用于扫描 git 提交、归因任务
3. **LLM**（可选）——服务商 / 模型 / API Key，开启对话与 agent 能力

配置随时可在「设置」里改。存储位置：

- `~/.jarvis/config.json`——明文配置（可手动编辑）
- OS 钥匙链——所有密钥（禅道密码、LLM Key、Bot Token 等），不落明文

## FAQ

- **我的数据存在哪？** 全部在本机 `~/.jarvis/`（配置、对话、记忆、日志）。除你自己配的禅道 / 帆软 / LLM 服务，不向任何服务器上传。
- **密钥安全吗？** API Key、密码、Token 一律存操作系统钥匙链（Windows Credential Manager / macOS Keychain），明文配置里只留占位符。
- **支持哪些大模型？** 任何 OpenAI 兼容接口：DeepSeek（默认）、Moonshot、Qwen，以及 Responses / Anthropic 协议。
- **必须联网吗？** 任务数据连你自己的禅道服务器；LLM 对话连你配的服务商。不配 LLM 也能用任务/工时/提醒功能。
- **数据隐私细节？** 见 [PRIVACY.md](PRIVACY.md)。

## 开发

```bash
npm ci
npm run check:text         # 主代码路径编码审计（CI 同步执行）
npm run check:text:full    # 仓库深扫（排除资产 / 抓包样本）
npm run desktop:dev        # 启动开发模式（端口 5174）
```

后端测试 / lint：

```bash
cargo test  --manifest-path src-tauri/Cargo.toml --lib
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets
```

## 构建

```bash
npm run desktop:build              # Windows NSIS 安装包
npm run desktop:build-macos-dev    # macOS universal（dev 签名）
npm run desktop:portable           # 本地便携版（不需要签名 key）
```

## 发版

推 tag `vX.Y.Z` 触发 GitHub Actions 自动构建 + 发布。**只有 tag 触发发版**；push 到 `main` 只跑 CI 质量门（测试 + lint），不发版。

```bash
# 1. 同步版本号（三处一致）：
#    src-tauri/tauri.conf.json  "version"
#    src-tauri/Cargo.toml        version
#    package.json                "version"
# 2. 提交 + 打 tag + 推送
git commit -am "release: bump version to X.Y.Z"
git tag vX.Y.Z
git push origin main && git push origin vX.Y.Z
```

详见 `.github/workflows/release.yml`。

## 项目结构

```
src-tauri/                  # Rust 后端（Tauri 插件 + 业务逻辑）
├── src/
│   ├── lib.rs              # 入口：插件注册 + 窗口/托盘 + 命令注册
│   ├── commands/           # Tauri 命令（前端调用入口，按域分文件）
│   ├── chat_agent.rs       # LLM agent 循环 + 工具调度
│   ├── llm/                # LLM 客户端（chat / responses / anthropic 三协议）
│   ├── tools/              # 工具定义（禅道 / 帆软 / Git / 发版等）
│   ├── channels/           # Telegram / QQ 消息通道
│   ├── fine_report/        # 帆软 BI 工时解析
│   ├── git_scan/           # git 提交扫描 + 归因
│   ├── memory/             # 记忆系统（sqlite-vec 向量检索）
│   ├── zentao.rs           # 禅道 API 客户端
│   └── settings.rs         # 配置 + 钥匙链
desktop/                    # Vue 3 前端
├── src/
│   ├── App.vue             # 主窗口（桌宠 + 气泡 + 菜单）
│   ├── components/         # UI 组件 + 各子窗口
│   └── composables/        # 组合式函数（提醒 / 拖拽 / 主题等）
scripts/                    # CI / 构建 / 发布脚本
```

## 贡献

欢迎 issue 和 PR，流程见 [CONTRIBUTING.md](CONTRIBUTING.md)。

## License

[MIT](LICENSE) © ynnyh
