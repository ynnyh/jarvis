// 帆软 BI（FineReport / 决策平台）接入。
//
// 这是公司用来看禅道工时的 BI 工具，JDBC 直连禅道 MySQL，开源版禅道本身的
// HTTP API 没有 effort listing，只能借道帆软读。
//
// 鉴权链路：
//   1. POST /webroot/decision/login 用账密换 JWT（14 天有效），返回带 fine_auth_token
//   2. JWT 缓存在 ~/.jarvis/finereport.json，剩 <1 天自动用账密续期
//   3. 调报表前先打开 viewlet 页面，从 HTML 里抠 cid（绑定一份"参数化的报表实例"）
//   4. POST op=fr_dialog&cmd=parameters_d 提交日期 + REAL_NAME 过滤
//   5. GET op=fr_write&cmd=read_w_content&cid=...&reportIndex=N 拿 JSON.html
//
// 密码绝不写入磁盘，进 OS keychain，service="Jarvis-FineReport"。

pub mod client;
pub mod commands;
pub mod credentials;
pub mod html_parser;

// Re-export for crate-internal callers (e.g. tools::effort_report)
pub use commands::finereport_get_efforts;
