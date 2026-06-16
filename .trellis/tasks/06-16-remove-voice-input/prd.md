# 下线语音输入功能(删本地 SenseVoice + 云端豆包 ASR)

> 微信已出语音输入法,自维护 sherpa 模型下载 + 豆包 ASR 双引擎的成本不划算。整体下线,减负 + 开源就绪。
> 砍后原拆分任务(06-15-refactor-split-modules)的 Batch 2(voice.rs)自动取消,只剩 llm + App.vue 两批。

---

## 1. 背景

voice.rs(2019 行)挤了两套引擎:
- **本地 SenseVoice**:sherpa-onnx-offline 二进制(22MB)+ int8 ONNX 模型(228MB)首启下载,cpal 采音 → WAV → 子进程转写 → arboard+enigo 注入。
- **云端豆包 ASR**:火山引擎 Volc WebSocket 协议,30+ 常量 + gzip 帧编解码。

微信语音输入法已覆盖"录音转文字注入"这个场景,自己维护模型下载 + 两套引擎不划算。整体下线。

## 2. 范围

### In scope — 后端(src-tauri/src)
1. `voice.rs` 整删(2019 行)。
2. `lib.rs`:删 `mod voice`、`.plugin(voice::global_shortcut_plugin())`、setup 里启动热键注册(`crate::voice::sync_hotkey`)、7 个 voice 命令注册(voice_assets_status / voice_download_assets / voice_open_dir / voice_start / voice_stop_and_transcribe / voice_hotkey_sync / voice_cloud_status)。
3. `commands/mod.rs`:删默认配置 5 个 voice* 字段(voiceInputEnabled / voiceHotkey / voiceEngine / voiceCloud{volcAppId, volcAccessToken});删 `hydrate_secret_placeholders` 与 `strip_secrets_for_save` 里的 voiceCloud.volcAccessToken(2 处)。
4. `logging.rs`:诊断脱敏账号标签数组去掉 `voiceCloud`。
5. `Cargo.toml`:删 cpal / hound / enigo / arboard / bzip2-rs / tar / tauri-plugin-global-shortcut / flate2。**保留 tokio-tungstenite**(channels/qqbot.rs 的 WebSocket 网关在用)。

### In scope — 前端(desktop/src)
6. `components/settings/VoiceInputSection.vue` 整删。
7. `settings-menu.ts`:删 import + general 菜单数组里的 VoiceInputSection。
8. `App.vue`:删 JarvisState 的 `'listening' | 'transcribing'` 状态 + 对应状态描述项 + voice-state / voice-transcribed / voice-error 三个事件 listen 及 onUnmounted 卸载。
9. `stores/config.ts`:删 voice 字段类型(voiceInputEnabled / voiceHotkey / voiceEngine / voiceCloud)+ 默认值 + 远程合并(loadConfig)+ volcAccessToken 占位符处理。

### In scope — 遗留清理(已拍板:启动时一次性清理)
10. 启动时**幂等清理**:删 `~/.jarvis/voice/` 目录(sherpa 二进制 + 模型 + 词表)+ 钥匙串条目 `voice.cloud.volcAccessToken`。不存在则跳过(天然幂等,无需持久标记位)。

### In scope — 收尾
11. 更新拆分任务 06-15-refactor-split-modules 的 prd.md:删掉 Batch 2(voice.rs)及其验收项,只保留 llm + App.vue 两批。

### Out of scope
- 不动 PetAvatar.vue / notification.ts / CyberParticles.vue / MatrixRain.vue / FineReportSection.vue —— 均为 mediaSrc / markAsRead / canvasRef / hasRealName 里 "asr" 子串误命中,无语音逻辑。
- 不重命名其它公开 API、不改其它功能。
- 不动 tokio-tungstenite(qqbot 依赖)。

## 3. 执行顺序(先后端后前端,每步编译验证)

1. **后端删除**:voice.rs 删 → lib.rs 去引用 → commands/mod.rs 去配置 → logging.rs → Cargo.toml 删依赖。
2. **启动清理**:lib.rs setup 加幂等清理(删 voice 目录 + keychain token)。
3. **后端验证**:cargo check + cargo clippy --all-targets -D warnings。
4. **前端删除**:VoiceInputSection.vue 删 → settings-menu → App.vue → config.ts。
5. **前端验证**:vue-tsc / npm run build。
6. **残留核查**:grep voice/asr/sherpa/volc/录音/语音 全仓,确认无活引用(只剩 CHANGELOG/记忆等文案)。
7. **收尾**:更新拆分 PRD。

## 4. 验收标准

| # | 条件 | 验证方式 |
|---|------|---------|
| 1 | voice.rs 不存在,lib.rs 无 voice 引用 | ls + grep |
| 2 | cargo check + clippy --all-targets -D warnings 通过 | 本地 |
| 3 | Cargo.toml 删了 8 个 voice 专用依赖,tokio-tungstenite 保留 | diff |
| 4 | 前端无 voice 残留,VoiceInputSection.vue 已删 | grep + ls |
| 5 | 启动清理:首次启动后 ~/.jarvis/voice/ 与 keychain token 被删 | 手动 |
| 6 | 现有非语音测试全过 | CI |
| 7 | 拆分任务 PRD 去掉 Batch 2 | diff |

## 5. 风险

| 风险 | 应对 |
|------|------|
| 删依赖后别处隐式 use 导致编译断 | 已 grep 确认仅 voice.rs 用(tokio-tungstenite 除外保留);cargo check 兜底 |
| 启动清理误删非 voice 数据 | 路径写死 `jarvis_dir().join("voice")`,只删这一个子目录;remove_dir_all 失败忽略 |
| 前端 voice 事件/状态删不净导致运行时报错 | vue-tsc + grep 核查 + 启动冒烟 |
| 本地 cargo test 跑不起来(native DLL 入口) | 以 CI 为准(同拆分任务约定) |

## 6. 不做功能

纯删除 + 必要的遗留清理,不新增任何用户可见功能。
