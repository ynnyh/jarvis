# 贡献指南

欢迎 issue 和 PR。

## 开发环境

需要 Node 20+ 和 Rust stable。详见 [README](README.md#开发)。

```bash
npm ci
npm run desktop:dev        # 开发模式，端口 5174
```

## 代码规范

- **Commit**：中文 [Conventional Commits](https://www.conventionalcommits.org/)——`feat:` / `fix:` / `refactor(scope):` / `docs:` / `chore:` 等。标题说清"做了什么"，去技术腔。
- **Rust**：提交前过 `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets`，不留 warning。
- **编码**：纯文本文件不得有乱码——`npm run check:text` 必须过（CI 会卡这一步）。
- **前端**：项目暂无 vue-tsc 类型检查网，改完务必跑 `npx vite build --config vite.config.ts` 确认能编译；改了 UI / 交互的，手动跑 app 冒烟一遍。

## 提 PR

1. Fork，建分支（`feat/xxx`、`fix/xxx`）
2. 改动 + 本地验证（clippy / vite build / check:text）
3. 向 `main` 提 PR，说明改了什么、为什么
4. CI 在 Windows + macOS 跑 `check:text → cargo test → clippy`，全绿才合并

## CI 与发版

- `push` 到 `main` 或提 PR → 触发 `.github/workflows/ci.yml`（编码审计 + 测试 + lint），**不发版**
- 发版是独立流程：打 tag `vX.Y.Z` 才触发 `release.yml` 构建发布，详见 README

## 行为准则

对人友善，对事较真；技术讨论对事不对人。
