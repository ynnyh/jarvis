# 常见问题

## 安装和使用

### 我的数据存在哪？

全部在本机 `~/.jarvis/` 目录：
- `config.json` - 配置文件（明文，可手动编辑）
- `conversations.db` - 对话历史和向量记忆（SQLite）
- `logs/` - 按天滚动的日志文件

**密钥不落盘**：所有 API Key、密码、Token 存操作系统钥匙链（Windows Credential Manager / macOS Keychain），明文配置里只留占位符。

除你自己配置的禅道/帆软/LLM 服务外，**不向任何第三方服务器上传数据**。

### 密钥安全吗？

✅ 是的。Jarvis 使用操作系统原生钥匙链存储所有敏感信息：

- **Windows**: Windows Credential Manager
- **macOS**: Keychain Access

读取需要操作系统权限验证，其他应用和用户无法访问。配置文件 `config.json` 里只存占位符（如 `${KEYRING:zentao_password}`），不会泄露明文密钥。

### 支持哪些大模型？

支持任何 **OpenAI 兼容接口**：
- DeepSeek（默认推荐）
- Moonshot（月之暗面）
- Qwen（通义千问）
- OpenAI
- 其他兼容 `/v1/chat/completions` 的服务

同时支持：
- **Claude** (Anthropic Messages API)
- **自定义 baseURL**

配置时只需填 `baseURL`、`apiKey` 和 `model` 即可。

### 必须联网吗？

**部分功能需要联网**：
- 禅道任务查询/工时写回 → 连你自己的禅道服务器
- LLM 对话 → 连你配置的模型服务商
- 自动更新 → 连 GitHub Releases

**本地功能不需要联网**：
- 桌宠显示
- 本地 git 扫描和代码量统计
- 配置管理
- 日志查看

不配置 LLM 也能用任务管理、工时统计、提醒等功能。

### macOS 打开提示"已损坏"或"无法验证开发者"怎么办？

这是 macOS Gatekeeper 对未签名应用的拦截。**两种解决方式**：

**方式一（推荐）**：运行自动安装脚本
```bash
curl -fsSL https://github.com/ynnyh/jarvis/releases/latest/download/install-macos-dev.sh | bash
```
脚本会自动处理权限。

**方式二**：手动解除隔离
```bash
xattr -cr /Applications/Jarvis.app
```
或者右键点击应用 → 选择「打开」。

### Windows Defender 提示"发现病毒"怎么办？

这是**误报**。Tauri 应用打包后，部分杀毒软件会将未知签名的 `.exe` 标记为可疑。

**解决方式**：
1. Windows Defender → 病毒和威胁防护 → 保护历史记录
2. 找到被隔离的 `Jarvis.exe`，选择「允许」
3. 或直接在安装目录右键 → 属性 → 解除阻止

Jarvis 代码开源可审计，安装包从 GitHub Releases 官方下载。

---

## 功能

### 禅道工时怎么自动计算？

Jarvis 通过 **git 归因 + 代码量反推**：

1. 扫描你配置的代码目录（`~/.jarvis/config.json` 里的 `codeRoots`）
2. 找到你的 git commit（按 `git config user.email` 匹配）
3. 统计每个任务相关的代码改动量（+/- 行数）
4. 用公式 `effort = 1 + sqrt(loc) / 10` 反推工时（小时）
5. 可选：自动写回禅道对应任务

**示例**：改动 100 行代码 → `1 + sqrt(100) / 10 = 2` 小时。

### 桌宠怎么自定义？

在设置 → 外观 → 桌宠形象，支持三种格式：

- **Lottie JSON**（推荐）：矢量动画，文件小，支持交互状态
- **图片**：PNG/JPG，静态或序列帧
- **GIF**：动图，但文件较大

官方提供几套预设形象，也可以自己导入。

### 如何添加自定义快捷键？

在 `~/.jarvis/config.json` 手动编辑 `hotkeys` 字段：

```json
{
  "hotkeys": {
    "toggle_window": "Cmd+Shift+J",
    "quick_input": "Cmd+Space"
  }
}
```

保存后重启 Jarvis 生效。支持的修饰键：`Cmd`/`Ctrl`/`Alt`/`Shift`。

### 多个禅道账号怎么切换？

当前版本仅支持单账号。如需切换：
1. 设置 → 禅道 → 修改账号和密码
2. 或直接编辑 `~/.jarvis/config.json` 里的 `zentao` 字段

未来版本会支持多账号配置。

---

## 隐私和安全

### Jarvis 会上传我的数据吗？

**不会**。Jarvis 是本地优先的应用，除以下情况外不上传任何数据：

1. **你主动配置的服务**：
   - 禅道：任务查询、工时写回 → 连你自己的禅道服务器
   - LLM：对话请求 → 连你配置的模型服务（DeepSeek/OpenAI/Claude 等）
   - Bot：消息收发 → 连 Telegram/QQ 服务器

2. **自动更新检测**：定期访问 GitHub Releases 检查新版本

3. **崩溃报告**：无。Jarvis 不内置遥测或崩溃上报。

所有对话历史、配置、日志都存本地 `~/.jarvis/`。

### LLM 对话会训练模型吗？

**取决于你选的服务商**。Jarvis 只负责发送请求，数据是否用于训练由服务商政策决定：

- **DeepSeek / Moonshot / Qwen**：通常 API 调用不用于训练（查阅各自隐私政策）
- **OpenAI**：默认不训练，但需在账号设置里确认
- **Claude**：Anthropic API 调用不用于训练

建议查阅你使用的服务商隐私政策。详见 [PRIVACY.md](/guide/privacy)。

### 可以离线使用吗？

**部分功能可以**：
- 桌宠显示、本地 git 扫描、配置管理无需联网
- 任务查询、LLM 对话、自动更新需要联网

如果你的禅道部署在局域网，内网环境也能用（不需要公网）。

---

## 开发和贡献

### 如何参与开发？

欢迎贡献！查看 [贡献指南](/guide/contributing) 和 [GitHub Issues](https://github.com/ynnyh/jarvis/issues)。

技术栈：
- **前端**：Vue 3 + TypeScript + Vite
- **后端**：Rust + Tauri v2
- **数据库**：SQLite + sqlite-vec（向量库）

### 如何报告 Bug？

1. 前往 [GitHub Issues](https://github.com/ynnyh/jarvis/issues)
2. 搜索是否已有相同问题
3. 如无，点击「New Issue」，提供：
   - 操作系统和版本
   - Jarvis 版本（设置 → 关于）
   - 复现步骤
   - 日志文件（`~/.jarvis/logs/` 最新的日志）

### 路线图里有什么？

计划中的功能（优先级从高到低）：

- [ ] 多禅道账号支持
- [ ] Jenkins 一键发版（已有 Agent 基础）
- [ ] 自定义 Agent 工作流编辑器
- [ ] 移动端伴侣 App（消息推送）
- [ ] 更多 IM 集成（企业微信/钉钉/Slack）
- [ ] 插件市场

---

## 其他

### Jarvis 这个名字的来源？

致敬钢铁侠的 J.A.R.V.I.S.（Just A Rather Very Intelligent System）—— 一个智能助手应该像 Jarvis 那样，安静、高效、随叫随到。

### 为什么开源？

- **透明**：隐私工具必须可审计
- **自由**：数据属于你，代码也应该属于你
- **社区**：一个人的想法有限，社区的智慧无限

### 还有问题？

- 查看 [文档](/guide/)
- 提交 [GitHub Issue](https://github.com/ynnyh/jarvis/issues)
- 或通过 README 里的联系方式反馈

<style>
h3 {
  margin-top: 32px;
  padding-top: 16px;
  border-top: 1px solid var(--vp-c-divider);
}

h3:first-of-type {
  margin-top: 0;
  padding-top: 0;
  border-top: none;
}

code {
  background: var(--vp-code-bg);
  padding: 2px 6px;
  border-radius: 4px;
  font-size: 0.9em;
}

blockquote {
  border-left: 4px solid var(--vp-c-brand-1);
  padding-left: 16px;
  color: var(--vp-c-text-2);
}
</style>
