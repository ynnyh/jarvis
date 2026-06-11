# Research: 豆包/火山 云端 ASR —— 决策与回报摘要

- **Query**: 综合回报——推荐接口与鉴权（越简单越好）、音频是否匹配现有 WAV、用户要注册填什么、Rust 实现要点、最大坑、是否推荐
- **Scope**: 决策摘要（完整证据见 `11-volcengine-asr-cloud-api.md`）
- **Date**: 2026-06-08

## 一句话结论

**推荐接入，难度中等偏低。** 选火山**流式语音识别大模型** WebSocket（`wss://openspeech.bytedance.com/api/v3/sauc/bigmodel`），鉴权是**最简单的 app_id + access_token 放握手 header（无签名、无对象存储）**，音频格式**与 Jarvis 现有 16k/16bit/单声道 PCM 完全匹配**（连重采样都省），且 `tokio-tungstenite` 等依赖**全部现成、零新增**（gzip 可能需补 flate2）。**别用「录音文件识别2.0」HTTP 接口**——那条要先把音频传火山 TOS 对象存储拿公网 URL（额外 IAM AK/SK + 建桶 + V4 签名，4 个变量跨 3 个控制台页），对「录一段当场转」过重。

## 推荐接口与鉴权（越简单越好）

- **接口**：`wss://openspeech.bytedance.com/api/v3/sauc/bigmodel`（流式大模型，默认端点；整段录完一次性发也 OK，不强制边录边传）。
- **鉴权**：WebSocket 握手加 5 个 header，**无任何签名**：
  - `X-Api-App-Id: <app_id>`、`X-Api-Access-Key: <access_token>`、`X-Api-Resource-Id: volc.bigasr.sauc.duration`、`X-Api-Request-Id: <uuid>`、`X-Api-Connect-Id: <uuid>`。
- 这是相对腾讯（TC3-HMAC 签名）/火山 OpenAPI（AK/SK 签名）**最省事**的形态。

## 音频格式：完全匹配现有 WAV

| 字段 | Jarvis 现状 | 火山要求 | 结论 |
|---|---|---|---|
| 采样率 | 16000 | 16000 | ✅ |
| 位深 | 16-bit PCM | 16 | ✅ |
| 声道 | 1（已混单声道） | 1 | ✅ |
| 编码 | 裸 PCM（写 WAV 前的 i16） | `codec:raw, format:pcm` | ✅ |

**云端路径连临时 WAV 都不用写**：`stop_recording()` 已返回 16k 单声道 `Vec<f32>`，转 i16 小端字节直接当裸 PCM 发即可（f32→i16 逻辑与 `write_temp_wav` 同）。

## 用户需要注册/填什么（写进设置页引导）

1. 注册登录火山引擎 `https://www.volcengine.com/`（实名）。
2. 进**新版**语音控制台 `https://console.volcengine.com/speech/service`（**别进旧版 `/speech/app`，鉴权完全不同**）。
3. 「语音识别」→「流式语音识别大模型」→ **开通**（开通即得免费额度）。
4. 复制 **App ID**（数字）与 **Access Token**（形如 `volc_...`）。
5. 填进 Jarvis 设置页两个框即可——**无需 IAM AK/SK、无需建 TOS 桶**。

> 免费额度：有（社区客户端文档：免费额度够测）。**具体数额以控制台实时为准，文案不写死**（火山额度政策常变）。

## Rust 实现要点

- **零新依赖**（`Cargo.toml` 已有）：`tokio-tungstenite`（WSS，qqbot.rs 已用）+ `futures-util` + `serde_json`。**唯一可能要补 `flate2`**（gzip 编解码；现有 `bzip2-rs` 是 bz2 不通用）——纯 Rust、符合「无 cmake」铁律。
- **二进制分帧**：手写 `marshal`/`parse`——帧 = 4 字节头 `[0x11, (type<<4)|flag, (ser<<4)|comp, 0x00]` + 可选 4 字节序号(i32) + 4 字节 payload 长度(u32) + payload。
- **发送序列**：① init 帧（FullClientRequest=0b0001, gzip(json{user,audio,request}), seq=1）；② 音频帧（AudioOnlyClient=0b0010, gzip(6400 字节/块 PCM), 序号递增）；③ 末块用 NegativeSeq=0b0011 + seq=-seq。
- **收包**：FullServerResponse=0b1001，payload gzip 解开取 **`result.text`**；`result.is_final`（或 flag&LastNoSeq=0b0010）→ 拿最终整段文本。
- **超时/错误**：`tokio::time::timeout`（连 ~10s、整体 ~30s）；握手 401/403 或 Error 帧（type=0b1111 带 error_code）映射中文（凭证错/额度尽/网络）。
- **分流改造极小**：只在 `stop_transcribe_inject()` 按 config（如 `voiceBackend: local|cloud`）选 `transcribe_wav`（本地 sherpa）或 `transcribe_volcengine`（云端）。**录音/注入/热键/小人状态全链路不动**——与任务背景「只换转写一段」吻合。
- **网络**：火山自家域名国内直连通常 OK，**默认不走** Telegram 翻墙代理（反而慢）。

## 凭证存储

- **App ID**：非敏感，明文存 config.json（`voice.cloud.volcAppId`）。
- **Access Token**：进 keychain `voice.cloud.volcAccessToken`（service `Jarvis-Secrets`），config 存 `********` 占位 + 读回 keychain 取真值——**完全复刻现有 `llm.apiKey` 的 strip/hydrate 套路**（commands/llm.rs）。

## 最大坑

1. **新旧控制台**：必须新版 `/speech/service`（`/speech/new/`）；旧版 `/speech/app` 的鉴权方式完全不同（旧版才要 cluster），凭证不通用。
2. **别选错接口**：录音文件识别（HTTP）鉴权虽简单（x-api-key），但逼你上传 TOS 对象存储 → 反而需 IAM AK/SK+建桶+V4 签名，整体最重。流式 WS 才是整体最省。
3. **二进制协议**：4 字节头 + gzip + 负序号结束，是唯一「费点神」的地方；协议表见 11 号文档，建议先拿已知音频跑通对照服务端回包再上链路。
4. **gzip 别漏**：init 的 JSON 和每个音频块都要 gzip，响应也要解 gzip；漏了直接 45000002 参数错。

## 备选（若火山不合适）

- **OpenAI/兼容 Whisper API**：鉴权最简单（`Authorization: Bearer`，HTTP multipart 传 WAV），但**国内需翻墙 + 无免费额度 + 中文不及豆包**。适合「已有 OpenAI key」的高级用户兜底。
- **腾讯一句话识别**：有免费额度，但 **TC3-HMAC-SHA256 签名繁琐**，比火山多一大坨代码，非首选。

## 落地改动清单（供主代理排期）

- `voice.rs`：加 `transcribe_volcengine(pcm, app_id, token)` + 二进制 marshal/parse + gzip 收发；`stop_transcribe_inject` 加 local/cloud 分流。
- `Cargo.toml`：确认/新增 `flate2`。
- config + 设置页：`voiceBackend` 开关 + 火山 App ID/Access Token 两个输入（token 走 keychain 占位）。
- 设置页引导文案：开通步骤（第「用户需要注册/填什么」节）。
