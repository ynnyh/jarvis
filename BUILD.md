# Jarvis 桌面端构建与分发

适用范围：把 Jarvis 打包成可以发给同事直接安装的 Windows .msi（macOS .dmg / Linux 包同理，但当前重点在 Windows）。

## 准备

- Node 20+
- Rust（stable，含 `cargo` 与 `rustup`）
- Windows 平台需安装 WiX Toolset（Tauri 第一次 `tauri build` 时会引导）

## 构建步骤

| 目标 | 命令 | 产物 | 网络要求 |
|---|---|---|---|
| **便携 zip（推荐）** | `npm run desktop:portable` | `dist/Jarvis-portable-<date>.zip`（~29 MB） | 仅首次拉 node.exe |
| 完整 .msi/.nsis 安装包 | `npm run desktop:build` | `src-tauri/target/release/bundle/...` | 首次还要拉 WiX 或 NSIS |
| 仅 binary（不打包） | `npm run desktop:build-binary` | `src-tauri/target/release/jarvis.exe` + `bundled/` | 仅首次拉 node.exe |

便携 zip 是最稳的路线——同事下载后解压即用，不需要安装器，也不依赖 GitHub 能否访问。MSI/NSIS 路线只有在构建机能访问 `github.com/wixtoolset/wix3/releases` 或 `github.com/tauri-apps/binary-releases` 时才走得通。

```bash
npm install              # 一次
npm run desktop:portable # 出 zip
```

`desktop:portable` 内部串了：

| 步骤 | 输出 |
|---|---|
| `desktop:bundle-daemon` | `src-tauri/bundled/daemon.mjs`（约 650 KB，所有 Node deps 内联） |
| `desktop:fetch-node` | `src-tauri/bundled/node.exe`（约 66 MB，Node 20 LTS Windows x64） |
| `tauri build --no-bundle` | `src-tauri/target/release/jarvis.exe` + 同级 `bundled/` 副本 |
| `node scripts/portable-zip.mjs` | `dist/Jarvis-portable-<date>.zip` |

## 第一次构建会下载什么

- `node.exe`（66 MB）从 `https://nodejs.org/dist/v20.18.1/win-x64/node.exe`
  - 内网无法访问公网时：手动下载 `node.exe` v20 win-x64，放到 `src-tauri/bundled/node.exe`，再跑 `desktop:build`。脚本检测到文件存在且大小合理（>10MB）会跳过下载。
  - 想换版本：`NODE_BUNDLE_VERSION=v20.20.0 npm run desktop:fetch-node`
- esbuild 不联网，本地把 `dist/daemon/server.js` 及全部运行时依赖打成单 ESM 文件。

## 产物布局（便携 zip 解压后 / 安装后）

便携 zip 解压：
```
Jarvis/
├── jarvis.exe
├── README.txt
└── bundled/
    ├── node.exe         (Node 20 LTS 便携)
    └── daemon.mjs       (Jarvis daemon + 全部依赖)
```

.msi 安装后：
```
C:\Program Files\Jarvis\
├── jarvis.exe
└── resources\
    └── bundled\
        ├── node.exe
        └── daemon.mjs
```

两种布局 Rust 端的 `daemon_client.rs::resolve_daemon_launch` 都能正确识别——它会按顺序探测 `<exe_dir>/resources/bundled/`、macOS `Resources/bundled/`、`<exe_dir>/bundled/`、dev 模式 `<root>/dist/daemon/server.js`。终端用户机器上**不需要**预装 Node。

## 终端用户首次启动流程

1. 双击 .msi 安装
2. 首次启动弹"欢迎引导"，5 步配置：禅道地址 → 账号 → 密码（写入 OS 密钥链） → 代码文件夹 → 完成
3. 之后所有改动都在右键菜单"设置"里

## 体积参考

| 文件 | 大小 |
|---|---|
| `Jarvis.exe`（Rust binary） | ~10 MB |
| `node.exe` | ~66 MB |
| `daemon.mjs` | ~650 KB |
| 其他 Tauri WebView 资源 | ~5 MB |
| **最终 .msi（压缩后）** | **~30-35 MB** |

## 故障排查

- **`fetch-node` 报 ECONNREFUSED**：内网 fw 卡 `nodejs.org`。同事手动下载 `node.exe` v20 win-x64，放到 `src-tauri/bundled/node.exe`，再跑 `desktop:portable`，脚本会自动跳过下载。
- **`tauri build` 报 `failed to bundle project: io: Connection refused`**：Tauri 想从 GitHub 下载 WiX 或 NSIS。两种解法：
  - 改走便携 zip：`npm run desktop:portable`
  - 在能访问 GitHub 的机器上构建一次后，把 `%LOCALAPPDATA%/tauri/` 拷过来缓存
- **白屏 / 启动后无 daemon 进程**：看 `~/.jarvis/daemon.json` 是否存在；不存在说明 spawn 失败。打开 `cmd`，到安装目录运行 `bundled\node.exe bundled\daemon.mjs`，看是否报错。
- **daemon 报 keychain 错**：用户尚未在欢迎引导里填密码。引导本身能跳过密码进入主界面，但任务同步会失败 → 让用户从设置 → 禅道连接里补。
- **构建机和目标机 OS 不一致**：当前打包脚本只抓 win-x64 的 node.exe。如果要给 macOS 同事打包，需要把 `fetch-node.mjs` 改成抓对应平台，或在 macOS 机上构建。

## 开发模式 vs 生产模式

| 模式 | daemon 入口 | node 来源 |
|---|---|---|
| `npm run desktop:dev` | `dist/daemon/server.js` | 系统 PATH 上的 node |
| 安装后启动 | `<resource_dir>/bundled/daemon.mjs` | `<resource_dir>/bundled/node.exe` |

Rust 端 `daemon_client.rs::resolve_daemon_launch` 自动判断：先找 exe 同级的 `resources/bundled/`，找不到再回落到项目根目录的 `dist/daemon/server.js` + 系统 node。
