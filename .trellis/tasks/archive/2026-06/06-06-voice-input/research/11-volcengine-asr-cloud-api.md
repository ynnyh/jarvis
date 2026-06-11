# Research: 豆包 / 火山引擎（Volcengine）云端 ASR 接入方案

- **Query**: 调研火山引擎语音识别云端 API，给出能落地到 Jarvis（设置里「本地/云端」二选一）的方案：接口选型 / 鉴权 / 请求响应 / 音频格式 / 免费额度 / 开通步骤 / Rust 接入 / 凭证存储
- **Scope**: external（火山引擎官方 + 两个可用社区客户端源码核实）+ internal（对齐现有 voice.rs / settings.rs 复用点）
- **Date**: 2026-06-08

## 一句话结论

**推荐接入，难度中等偏低，且零新依赖。** 选**流式语音识别大模型**（`sauc/bigmodel` WebSocket），鉴权是**最简单的 app_id + access_token 放握手 header**（无 AK/SK 签名、无 TOS 对象存储）。音频格式**与 Jarvis 现有 16k 单声道 16-bit PCM 完全匹配**，连重采样都省了。坑在「WebSocket 二进制分帧协议」（4 字节头 + gzip 压缩的 JSON/音频帧），但项目已有 `tokio-tungstenite` + `flate2`/`gzip` 能力，照本文协议表实现即可。**不要走「录音文件识别2.0」HTTP 接口**——那条要先把音频传到火山 TOS 对象存储拿公网 URL（需额外 IAM AK/SK + 建桶 + V4 签名，4 个环境变量跨 3 个控制台页），对「录一小段当场转」是杀鸡用牛刀。

---

## 1. API 选型

火山引擎语音识别（豆包大模型 ASR）主要有三类接口形态：

| 接口 | 形态 | 音频来源 | 鉴权 | 适合 Jarvis？ |
|---|---|---|---|---|
| **流式语音识别 `sauc/bigmodel`** | **WebSocket**（双向流） | **直接发原始音频字节** | **app_id + access_token（header）** | ✅ **首选** |
| 录音文件识别 `auc/bigmodel`（2.0） | HTTP submit + poll | **必须公网 URL**（要先传 TOS） | x-api-key（UUID） | ❌ 太重（需对象存储） |
| 一句话识别（旧版 small model） | WebSocket | 直接发音频 | app_id + token + **cluster** | ⚠️ 旧版，新项目用 bigmodel |

### 推荐：`wss://openspeech.bytedance.com/api/v3/sauc/bigmodel`

火山「流式语音识别大模型」有三个共用同一套二进制协议、鉴权、事件流的端点，仅 URL 与「服务器何时回结果」不同（官方文档 6561/1354869）：

| endpoint | URL | 行为 | 适用 |
|---|---|---|---|
| `bigmodel`（默认） | `wss://openspeech.bytedance.com/api/v3/sauc/bigmodel` | 每个输入包回一个响应 | 首字延迟最低；**整段录完一次性发也 OK** |
| `bigmodel_async` | `.../api/v3/sauc/bigmodel_async` | 仅当结果变化才回包 | 真·连续流式（RTF/首尾字延迟更好），官方推荐做实时默认 |
| `bigmodel_nostream` | `.../api/v3/sauc/bigmodel_nostream` | >15s 或最终包才回 | 整段上传、最高准度，支持 language 提示 |

**Jarvis 用法**：我们是「录完整段一次性识别」，用默认 `bigmodel` 最简单——建连后把整段 16k PCM 切成若干 200ms 块依次发，最后一块发负序号（结束标志），收到 `is_final` 即拿到完整文本。完全不依赖「边录边传」。

> 关键：流式接口虽然走 WebSocket，但**不强制你边录边发**。Jarvis 现有链路（录完 → 有完整 f32/PCM buffer）只需在 `transcribe` 阶段：连 WS → 发 init → 把 buffer 分块发完 → 发结束帧 → 收结果。语义上等价于「一次性识别」，对接成本可控。

---

## 2. 鉴权（最简单形态，已核实）

**流式 ASR 用 app_id + access_token，放在 WebSocket 握手的 HTTP header 上，无任何签名计算。**

握手 header（核实自可用客户端 `_build_headers`）：

```
X-Api-App-Id:       <app_id>          # 火山语音控制台的 App ID（一串数字，如 1234567890）
X-Api-Access-Key:   <access_token>    # 控制台的 Access Token（形如 volc_xxxxx）
X-Api-Resource-Id:  volc.bigasr.sauc.duration   # 流式语音识别大模型（按时长计费）的资源 ID
X-Api-Request-Id:   <uuid>            # 本次请求 UUID（自己生成）
X-Api-Connect-Id:   <uuid>            # 本次连接 UUID（自己生成，每连一换）
```

- **没有 AK/SK / Signer V4 / TC3 那套签名**——纯把两个静态凭证塞 header。这是相对腾讯/阿里/火山 OpenAPI 管理类接口**最省事**的一种。
- `X-Api-App-Key` 有的端点也认（值同 app_id），保险起见可两个都带。
- `X-Api-Resource-Id` 决定走哪个计费模型，**流式大模型固定 `volc.bigasr.sauc.duration`**。
- ⚠️ **新旧控制台坑**：必须用**新版控制台** `https://console.volcengine.com/speech/service`（或 `/speech/new/`）拿凭证；旧版 `/speech/app` 的鉴权方式完全不同（旧版才需要 cluster），别混。

> 对比：**录音文件识别（HTTP）** 用的是 `x-api-key: <UUID>` 单 header（也很简单），但它的「音频要公网 URL」逼着你再配 IAM AK/SK + TOS 桶 + V4 预签名上传——综合下来反而比流式 WS 复杂得多。所以鉴权简单不代表整体简单，**流式 WS 才是整体最省**。

---

## 3. 请求 / 响应

### 3.1 连接

```
WSS 握手到 wss://openspeech.bytedance.com/api/v3/sauc/bigmodel
握手 header 见第 2 节
```

### 3.2 二进制分帧协议（核实自可用客户端 `Message.marshal`）

每个 WS message 都是二进制帧：**4 字节定长头 + 可选字段 + payload**。

**4 字节头**（大端）：
```
byte0 = (version<<4) | header_size      // version=1, header_size=1 → 固定 0x11（header 占 1*4=4 字节）
byte1 = (msg_type<<4) | flags
byte2 = (serialization<<4) | compression
byte3 = 0x00                            // 保留
```

枚举值：
```
msg_type:  FullClientRequest=0b0001  AudioOnlyClient=0b0010
           FullServerResponse=0b1001 AudioOnlyServer=0b1011  Error=0b1111
flags:     NoSeq=0b0000  PositiveSeq=0b0001  LastNoSeq=0b0010  NegativeSeq=0b0011
serialization: Raw=0  JSON=0b0001
compression:   None=0  Gzip=0b0001
```

头之后：若 flags 是 PositiveSeq/NegativeSeq → 写 4 字节大端 **序号 sequence (i32)**；然后写 4 字节大端 **payload 长度 (u32)**；然后写 payload 字节。

### 3.3 发送序列（整段一次性识别）

1. **init 帧**（FullClientRequest, flag=PositiveSeq, serialization=JSON, compression=Gzip, sequence=1）
   payload = `gzip(json(init_payload))`，init_payload：
   ```json
   {
     "user":  { "uid": "<X-Api-Request-Id 或任意稳定标识>" },
     "audio": { "format": "pcm", "codec": "raw", "rate": 16000, "bits": 16, "channel": 1 },
     "request": {
       "model_name": "bigmodel",
       "enable_itn":  true,
       "enable_punc": true,
       "enable_ddc":  true,
       "show_utterances": true,
       "enable_nonstream": false
     }
   }
   ```
2. **音频帧**（AudioOnlyClient, flag=PositiveSeq, compression=Gzip, sequence=2,3,...）
   - 把整段 PCM 按 ~200ms 切块（16000 * 2 bytes * 1ch * 0.2s = **6400 字节/块**）；
   - 每块 payload = `gzip(pcm_chunk)`，序号递增。
3. **最后一块**用 **flag=NegativeSeq, sequence = -seq**（负序号标记「最后一包」），payload = `gzip(最后一块 PCM)`（无剩余则发空）。

> 注：可用客户端的实现把音频帧的 serialization 也标了 JSON（实际 payload 是 gzip 后的裸 PCM）。火山服务端靠 msg_type=AudioOnlyClient 识别这是音频而非 JSON，serialization 位对音频帧不敏感；保守起见可对音频帧用 Raw=0，二者实测皆可。

### 3.4 接收 / 解析结果

服务端回 FullServerResponse 帧，payload 是 gzip 后的 JSON。解 gzip → `json` 后取：

```json
{ "result": { "text": "识别出的文本", "is_final": false, "utterances": [ ... ] } }
```

- **文本字段路径：`result.text`**。
- `result.is_final == true`（或帧 flag 带 LastNoSeq=0b0010 位）→ 本次识别结束，`result.text` 即最终整段文本。
- 中间会回多个 partial（text 渐长），Jarvis 整段模式**只取最终那一条**即可。

### 3.5 中英混说 / 标点

- `enable_punc=true` → 输出带标点。
- `enable_itn=true` → 逆文本归一（数字/日期规整）。
- `enable_ddc=true` → 顺滑（去口癖/重复）。
- 中英混说：豆包大模型 ASR 原生支持中文 + 中英混说、多方言（普通话/粤语/四川话等）+ 13+ 语言，无需指定语言（`bigmodel`/`bigmodel_async` 不需要 language；只有 `bigmodel_nostream` 支持 language 提示）。契合 Jarvis「中文夹英文术语」场景。

---

## 4. 音频格式（与 Jarvis 现有 WAV 是否匹配）

**完全匹配，零转换。** init_payload 的 audio 段：

| 字段 | Jarvis 现状（voice.rs） | 火山要求 | 结论 |
|---|---|---|---|
| 采样率 rate | 16000（`TARGET_SAMPLE_RATE`） | 16000 | ✅ 一致 |
| 位深 bits | 16-bit PCM（写 WAV 时 `bits_per_sample:16`） | 16 | ✅ 一致 |
| 声道 channel | 1（已混单声道） | 1 | ✅ 一致 |
| 编码 codec | 裸 PCM（写 WAV 前的 i16 样本流） | `raw` | ✅ 一致 |
| 容器 format | WAV（hound 写） / 或裸 PCM | `pcm`（裸）或 `wav` | ✅ 都支持 |

**最佳做法**：云端路径**不必写 WAV 临时文件**。`stop_recording()` 已返回归一好的 16k 单声道 `Vec<f32>`；只需把 f32 → i16（`(s.clamp(-1,1)*i16::MAX) as i16`，逻辑同 `write_temp_wav`）→ 小端字节流，作为裸 PCM 直接发（`format:"pcm", codec:"raw"`）。比本地路径还少一步落盘。

> 录音文件识别（HTTP）那条支持 WAV/MP3/MP4/M4A/OGG/FLAC，但需上传 URL，不在推荐路径内。

---

## 5. 免费额度 & 开通（给用户的「你要做什么」清单）

### 免费额度（概述，以控制台实时为准）

- 火山引擎语音识别开通后**有免费试用额度/资源包**，社区可用客户端文档原话：「free tier suffices for testing」（免费额度够测试）。
- 火山额度政策常变（按时长/并发的体验包），**金额/时长以开通时控制台「资源包/计费」页显示为准**，本文不写死数字。Jarvis 文案建议写「火山引擎语音识别有免费额度，开通后可直接试用」，并放控制台链接，不承诺具体数额。

### 用户开通步骤（写进设置页引导）

1. 注册并登录火山引擎：`https://www.volcengine.com/`（手机号/实名认证）。
2. 进**新版语音控制台**：`https://console.volcengine.com/speech/service`（务必新版，别进 `/speech/app` 旧版）。
3. 左侧「语音识别」→「**流式语音识别大模型**」→ 点「**开通**」（开通即获免费额度）。
4. 在该模型页找到 **App ID** 和 **Access Token**（新版控制台「鉴权信息 / API 调用」处；Access Token 形如 `volc_...`）。
5. 把 **App ID** 和 **Access Token** 填进 Jarvis 设置页「云端语音（火山引擎）」两个输入框。

> 仅此两项，无需 IAM AK/SK、无需建 TOS 桶（那是录音文件识别才要的，流式不需要）。这是相对其它云厂商**最少的填写项**。

---

## 6. Rust 接入要点

依赖**全部现成**（`src-tauri/Cargo.toml` 已有，零新增）：
- `tokio-tungstenite = { version = "0.24", features = ["rustls-tls-webpki-roots"] }`（WSS 客户端，qqbot.rs 已用过）
- `futures-util`（stream 收发）
- gzip：项目用 `reqwest` 但禁了自动解压；需要一个 gzip 编解码。**确认是否已有 `flate2`**——若无，加 `flate2`（纯 Rust，无 C 依赖，符合「无 cmake」铁律）。`bzip2-rs` 已在用但那是 bz2，gzip 仍需 flate2。
- `serde_json`、`base64`（base64 此路径用不到，流式发裸字节）、`uuid` 或用现成 ID 生成。

实现骨架（放 voice.rs，与本地路径并列；按 config 的「本地/云端」开关分流）：

```rust
// 伪代码要点
async fn transcribe_volcengine(pcm_i16: &[i16], app_id: &str, token: &str) -> Result<String, String> {
    use tokio_tungstenite::connect_async;
    use tungstenite::handshake::client::Request;
    // 1) 构造带 X-Api-* header 的握手请求
    let req = Request::builder()
        .uri("wss://openspeech.bytedance.com/api/v3/sauc/bigmodel")
        .header("X-Api-App-Id", app_id)
        .header("X-Api-Access-Key", token)
        .header("X-Api-Resource-Id", "volc.bigasr.sauc.duration")
        .header("X-Api-Request-Id", uuid())
        .header("X-Api-Connect-Id", uuid())
        // tungstenite 还需 Host/Upgrade/Connection/Sec-WebSocket-* 等标准头（用 IntoClientRequest 自动补，手动构造时要补全）
        .body(()).map_err(..)?;
    let (mut ws, _resp) = connect_async(req).await.map_err(..)?;

    // 2) 发 init 帧：marshal(FullClientRequest, PositiveSeq, JSON, Gzip, seq=1, gzip(json(init_payload)))
    ws.send(Message::Binary(marshal_full_client(&init_payload))).await?;

    // 3) 把 pcm_i16 转小端字节，按 6400 字节切块，逐块 marshal(AudioOnlyClient, PositiveSeq, Gzip, seq++, gzip(chunk))
    //    最后一块用 NegativeSeq, seq=-seq
    // 4) 循环 ws.next() 收二进制帧 → 去 4 字节头 → 读 seq/len → gzip 解 payload → json → result.text
    //    收到 is_final（或 flag&LastNoSeq）→ 返回 result.text
}
```

要点：
- **握手 header**：`tokio-tungstenite` 用 `IntoClientRequest`/手动 `Request` 加自定义 header。qqbot.rs 已有 WSS 连接范例，照搬连接姿势 + 加这几个 `X-Api-*` 头即可。
- **二进制帧编解码**：手写两个小函数 `marshal`（拼 4 字节头 + 可选 seq + len + payload）和 `parse`（反向）。协议表见第 3 节，逻辑很短。
- **超时 & 错误**：建连/收包都用 `tokio::time::timeout`（本地路径 sherpa 子进程一两秒，云端连超时设 ~10s、整体 ~30s 合理）。握手回 401/403 或服务端 Error 帧（msg_type=0b1111，payload 带 error_code）→ 映射成中文错误（凭证错/额度用尽/网络）。复用现有 `eprintln!("[voice] ...")` 留痕风格。
- **网络/代理**：国内直连 `openspeech.bytedance.com` 通常没问题（火山自家域名，无需翻墙）。若要兜底可复用 `download_proxy()` 的代理读取，但一般云端 ASR **不该走** Telegram 那个翻墙代理（反而更慢），默认直连即可。
- **gzip**：init 的 JSON 和每个音频块都要 `gzip.compress`；收到的响应 payload 要 `gzip.decompress`。用 flate2 的 `GzEncoder`/`GzDecoder`。
- **分流**：在 `stop_transcribe_inject()` 里按 config（如 `voiceBackend: "local" | "cloud"`）选 `transcribe_wav`（本地 sherpa）还是 `transcribe_volcengine`（云端）。注入/录音/热键/小人状态全链路**不动**，只换中间「转写」一段——与任务背景描述完全吻合。

---

## 7. 凭证存储建议

复用现有 keychain（`crate::settings::secret_set/secret_get`，service 名 `Jarvis-Secrets`），命名对齐现有 `llm.profile.{id}.apiKey` / `zentao.sessionCookie` 风格：

| 凭证 | keychain account 建议 | 备注 |
|---|---|---|
| App ID | **不进 keychain**，明文存 config.json（如 `voice.cloud.volcAppId`） | App ID 非敏感（类似用户名），放 config 方便显示 |
| Access Token | **进 keychain**：`voice.cloud.volcAccessToken` | 敏感，走 keyring；config 里存 `********` 占位（同 LLM apiKey 的 strip/hydrate 套路） |

- 落盘前 `strip_secrets_for_save` 把 token 替成 `SECRET_PLACEHOLDER`，读回时 keychain 取真值（完全复刻 llm.apiKey 的处理，见 commands/llm.rs）。
- config 新增字段建议：`voiceBackend`（"local"/"cloud"）、`voice.cloud.volcAppId`（明文）、`voice.cloud.volcAccessToken`（占位，真值在 keychain）。

---

## 8. 备选云端 ASR（若火山不合适）

| 方案 | 鉴权复杂度 | 音频/接口 | 免费额度 | 对 Jarvis 评价 |
|---|---|---|---|---|
| **火山 流式 bigmodel**（本文推荐） | **低**（app_id+token header，无签名） | WSS 发裸 PCM，16k/16bit/mono 直配 | 有免费额度（够测） | ✅ 首选：格式零转换、零新依赖、中文强 |
| **OpenAI / 兼容 Whisper API** | **低**（`Authorization: Bearer <key>` 一个头） | **HTTP multipart**（`POST /v1/audio/transcriptions`，上传整个文件，`model=whisper-1`/`gpt-4o-transcribe`） | 无免费额度（按量付费，需海外卡/代理） | ⚠️ 接入最简单（普通 multipart，reqwest 直接发 WAV），但**国内需翻墙 + 无免费额度 + 中文不及豆包**。可作「已有 OpenAI key 的高级用户」选项 |
| **腾讯云 一句话识别 SentenceRecognition** | **高**（TC3-HMAC-SHA256 签名，要算 CanonicalRequest+签名串+派生密钥） | HTTP，body 放 base64 音频 | 有免费额度 | ⚠️ 接口本身简单（一次性 base64），但**签名繁琐**，比火山多一大坨签名代码 |
| 阿里 / 讯飞 | 中~高（多为 token 拉取 + 签名/鉴权混合） | WSS/HTTP | 有免费额度 | 不优先：鉴权比火山复杂 |

**结论**：火山流式在「鉴权简单 + 音频零转换 + 中文强 + 免费额度 + 零新依赖」四项上综合最优，作默认云端。若想给「无国内云、已有 OpenAI key」的用户兜底，可加 OpenAI Whisper（HTTP multipart，实现成本低，但需代理且付费）。腾讯/阿里因签名复杂，非首选。

---

## 来源（已核实）

- 火山官方文档（SPA，端点/资源 ID 经下列客户端源码交叉核实）：
  - 流式语音识别大模型：`https://www.volcengine.com/docs/6561/1354869`
  - 录音文件识别2.0：`https://www.volcengine.com/docs/6561/1354868`
- 官方 SDK org：`github.com/volcengine`（语音服务走独立 openspeech 协议，非 volc-sdk-* 的 AK/SK OpenAPI）
- 可用客户端源码（核实流式协议/鉴权/音频字段）：
  - `github.com/Hypnus-Yuan/doubao-speech` — `src/doubao_speech/_ws_client.py`（`asr_stream`：endpoint、`X-Api-*` header、二进制 marshal、init_payload audio 段 rate/bits/channel、result.text 解析），`config.py`（app_id/access_token 形态）
  - `github.com/vahnxu/doubao-asr` — `scripts/transcribe.py` + `SKILL.md`（录音文件识别2.0 的 HTTP submit/poll、x-api-key、TOS 上传为何重、新旧控制台坑、开通步骤）

## Caveats / 未决

- **免费额度具体数字未写死**：火山额度政策多变，文案让用户看控制台实时值（避免承诺过期数据）。
- **二进制协议细节以实测为准**：本文协议表来自可用 Python 客户端核实，落地时建议先用一段已知音频跑通、对照服务端回包验证帧格式（尤其音频帧 serialization 位、负序号结束语义）。
- **flate2 是否已在依赖树**：需确认；若无要新增（纯 Rust，符合无 cmake 铁律）。
- 手动构造 tungstenite 握手 Request 时要补全标准 WebSocket 头（Host/Upgrade/Connection/Sec-WebSocket-Key/Version），否则握手失败；优先用 `IntoClientRequest` 再追加 `X-Api-*`。
