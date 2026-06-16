---
layout: home

hero:
  name: Jarvis
  text: 个人桌面 AI 助手
  tagline: 本地优先 · 隐私安全 · 智能高效
  image:
    src: /hero-illustration.svg
    alt: Jarvis AI Assistant
  actions:
    - theme: brand
      text: 立即下载
      link: /download
    - theme: alt
      text: 快速开始
      link: /guide/

features:
  - icon: 🔒
    title: 本地优先
    details: 所有数据存储在 ~/.jarvis/，密钥走操作系统钥匙链，不上传任何第三方服务器（除你自己配置的服务）
  - icon: 🤖
    title: 多 LLM 支持
    details: 兼容 OpenAI 协议（DeepSeek / Moonshot / Qwen），支持 Claude，对话式触发任务操作和自动化
  - icon: 📋
    title: 禅道任务管理
    details: 任务查询、到期提醒、风险分析、工时自动写回，按代码量反推工时，解放双手
  - icon: 💬
    title: 多渠道消息
    details: 支持 Telegram / QQ Bot，白名单控制，随时随地接收通知和执行命令
  - icon: 🎨
    title: 可定制桌宠
    details: Lottie / 图片 / GIF 自定义形象，状态气泡提醒，让 AI 助手更生动有趣
  - icon: ⚡️
    title: 快捷键控制
    details: 全局快捷键唤起，快速输入命令，高效操作不打断工作流

---

<style>
:root {
  --vp-home-hero-name-color: transparent;
  --vp-home-hero-name-background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
  --vp-home-hero-image-background-image: linear-gradient(135deg, #667eea20 0%, #764ba220 100%);
  --vp-home-hero-image-filter: blur(80px);
}

.VPFeature {
  transition: all 0.3s ease;
}

.VPFeature:hover {
  transform: translateY(-4px);
  box-shadow: 0 8px 16px rgba(0, 0, 0, 0.1);
}
</style>

## 为什么选择 Jarvis？

<div class="comparison-grid">

<div class="comparison-card">
  <h3>🏠 本地优先</h3>
  <p>你的数据属于你自己</p>
  <ul>
    <li>配置文件明文可编辑</li>
    <li>密钥存系统钥匙链</li>
    <li>对话记忆本地向量库</li>
    <li>日志按天滚动本地存储</li>
  </ul>
</div>

<div class="comparison-card">
  <h3>🚀 开箱即用</h3>
  <p>5 分钟完成配置</p>
  <ul>
    <li>欢迎引导自动弹出</li>
    <li>智能配置检测</li>
    <li>跨平台一致体验</li>
    <li>自动更新无感升级</li>
  </ul>
</div>

<div class="comparison-card">
  <h3>🔧 高度可定制</h3>
  <p>灵活适配你的工作流</p>
  <ul>
    <li>多种桌宠形象</li>
    <li>自定义快捷键</li>
    <li>插件化架构</li>
    <li>开源可审计</li>
  </ul>
</div>

</div>

<style>
.comparison-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));
  gap: 24px;
  margin: 32px 0;
}

.comparison-card {
  padding: 24px;
  border-radius: 12px;
  background: var(--vp-c-bg-soft);
  border: 1px solid var(--vp-c-divider);
  transition: all 0.3s ease;
}

.comparison-card:hover {
  transform: translateY(-4px);
  box-shadow: 0 8px 24px rgba(0, 0, 0, 0.08);
  border-color: var(--vp-c-brand-1);
}

.comparison-card h3 {
  margin-top: 0;
  color: var(--vp-c-brand-1);
  font-size: 1.1em;
}

.comparison-card p {
  color: var(--vp-c-text-2);
  margin-bottom: 16px;
}

.comparison-card ul {
  margin: 0;
  padding-left: 20px;
}

.comparison-card li {
  color: var(--vp-c-text-1);
  line-height: 1.8;
}
</style>

## 快速开始

```bash
# macOS
curl -fsSL https://github.com/ynnyh/jarvis/releases/latest/download/install-macos-dev.sh | bash

# Windows
# 从 Releases 下载 .exe 安装包，双击安装
```

启动后跟随欢迎引导配置禅道、代码目录和 LLM，5 分钟即可开始使用。

---

<div style="text-align: center; margin: 48px 0;">
  <a href="/download" style="display: inline-block; padding: 12px 32px; background: var(--vp-c-brand-1); color: white; border-radius: 8px; text-decoration: none; font-weight: 600; transition: all 0.3s ease;">
    立即下载 →
  </a>
</div>

