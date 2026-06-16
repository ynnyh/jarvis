# 下载

## 最新版本

从 [GitHub Releases](https://github.com/ynnyh/jarvis/releases) 下载最新版本。

<div class="download-cards">

<div class="download-card">
  <div class="platform-icon">🪟</div>
  <h3>Windows</h3>
  <p>支持 Windows 10/11</p>
  <a href="https://github.com/ynnyh/jarvis/releases/latest/download/Jarvis_x64-setup.exe" class="download-button">
    下载 .exe 安装包
  </a>
  <details class="install-steps">
    <summary>安装步骤</summary>
    <ol>
      <li>双击下载的 <code>.exe</code> 文件</li>
      <li>跟随安装向导完成安装</li>
      <li>首次启动会弹出欢迎引导</li>
      <li>依次配置禅道、代码目录、LLM</li>
    </ol>
  </details>
</div>

<div class="download-card">
  <div class="platform-icon">🍎</div>
  <h3>macOS</h3>
  <p>支持 macOS 11+ (Intel + Apple Silicon)</p>
  <a href="https://github.com/ynnyh/jarvis/releases/latest/download/Jarvis_universal.dmg" class="download-button">
    下载 .dmg 安装包
  </a>
  <details class="install-steps">
    <summary>安装步骤</summary>
    
    **方式一：自动安装（推荐）**
    ```bash
    curl -fsSL https://github.com/ynnyh/jarvis/releases/latest/download/install-macos-dev.sh | bash
    ```
    脚本会自动下载、安装、处理 Gatekeeper 权限。

    **方式二：手动安装**
    <ol>
      <li>打开下载的 <code>.dmg</code> 文件</li>
      <li>拖动 Jarvis 到 Applications 文件夹</li>
      <li>首次打开时，右键点击应用 → 选择「打开」（绕过 Gatekeeper）</li>
      <li>或在终端运行：<code>xattr -cr /Applications/Jarvis.app</code></li>
    </ol>
  </details>
</div>

</div>

## 系统要求

- **Windows**: Windows 10 (1809+) 或 Windows 11
- **macOS**: macOS 11 Big Sur 或更高版本
- **硬盘**: 至少 200 MB 可用空间
- **网络**: 需要联网访问禅道/LLM 服务

## 验证安装

安装完成后，启动 Jarvis：

1. **欢迎引导**会自动弹出（首次启动或配置不完整时）
2. 依次填写：
   - 禅道地址和账号（密码存进系统钥匙链）
   - 代码目录（用于 git 扫描）
   - LLM 配置（可选，不配也能用任务功能）
3. 配置完成后，桌宠会出现在屏幕角落

## 数据存储位置

- **Windows**: `C:\Users\<用户名>\.jarvis\`
- **macOS**: `~/.jarvis/`

包含：
- `config.json` - 明文配置
- `conversations.db` - 对话历史（SQLite + 向量库）
- `logs/` - 按天滚动的日志文件

密钥存储：
- **Windows**: Windows Credential Manager
- **macOS**: Keychain Access

## 更新

Jarvis 支持自动更新：

- 启动时自动检测新版本
- 通知栏提示有更新可用
- 点击「立即更新」自动下载安装
- 或手动下载最新版覆盖安装

## 卸载

**Windows**:
1. 控制面板 → 程序和功能 → 卸载 Jarvis
2. 手动删除 `C:\Users\<用户名>\.jarvis\`（可选）

**macOS**:
1. 将 `/Applications/Jarvis.app` 拖到废纸篓
2. 手动删除 `~/.jarvis/`（可选）
3. 删除钥匙链中的 Jarvis 相关密钥（可选）

---

## 遇到问题？

查看 [FAQ](/faq) 或提交 [GitHub Issue](https://github.com/ynnyh/jarvis/issues)。

<style>
.download-cards {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(320px, 1fr));
  gap: 32px;
  margin: 32px 0;
}

.download-card {
  padding: 32px;
  border-radius: 16px;
  background: var(--vp-c-bg-soft);
  border: 2px solid var(--vp-c-divider);
  text-align: center;
  transition: all 0.3s ease;
}

.download-card:hover {
  transform: translateY(-4px);
  box-shadow: 0 12px 32px rgba(0, 0, 0, 0.12);
  border-color: var(--vp-c-brand-1);
}

.platform-icon {
  font-size: 64px;
  margin-bottom: 16px;
}

.download-card h3 {
  margin: 0 0 8px 0;
  font-size: 1.5em;
}

.download-card > p {
  color: var(--vp-c-text-2);
  margin-bottom: 24px;
}

.download-button {
  display: inline-block;
  padding: 12px 32px;
  background: var(--vp-c-brand-1);
  color: white !important;
  border-radius: 8px;
  text-decoration: none;
  font-weight: 600;
  transition: all 0.3s ease;
}

.download-button:hover {
  background: var(--vp-c-brand-2);
  transform: scale(1.05);
}

.install-steps {
  margin-top: 24px;
  text-align: left;
  padding: 16px;
  background: var(--vp-c-bg);
  border-radius: 8px;
}

.install-steps summary {
  cursor: pointer;
  font-weight: 600;
  color: var(--vp-c-brand-1);
  user-select: none;
}

.install-steps summary:hover {
  color: var(--vp-c-brand-2);
}

.install-steps ol {
  margin-top: 12px;
  padding-left: 20px;
}

.install-steps li {
  line-height: 1.8;
  margin-bottom: 8px;
}

.install-steps code {
  font-size: 0.9em;
}
</style>
