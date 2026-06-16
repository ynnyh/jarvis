// 结构化日志初始化。
//
// 替换全量 eprintln!,改用 tracing 分级宏。日志双输出:
//   - stderr:带颜色,INFO+,开发模式终端可见
//   - 文件:无颜色,DEBUG+,按天滚动 ~/.jarvis/logs/jarvis.log.YYYY-MM-DD
//
// WorkerGuard 必须保活到进程结束 —— appender 在 drop 时 flush 最后一批日志,
// guard 提前 drop 会丢尾部日志。init_logging 返回 guard,调用方存进 App 生命周期。
//
// 降级:目录不可写等初始化失败 → 只挂 stderr 层,不阻断 app(日志缺失比 app 起不来轻)。
//
// 过滤策略:
//   - RUST_LOG 存在 → 完全交给 EnvFilter(开发时精细控制,如 RUST_LOG=jarvis=debug,reqwest=warn)
//   - RUST_LOG 不存在 → 用静态默认:jarvis 自己 INFO,第三方(reqwest/hyper/tokio 等)压到 WARN

use std::path::PathBuf;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Layer};

use crate::settings::jarvis_dir;

/// 日志目录:`~/.jarvis/logs/`。复用 jarvis_dir,与 config.json / memory.db 同根。
pub fn logs_dir() -> PathBuf {
    jarvis_dir().join("logs")
}

/// 默认过滤策略(RUST_LOG 未设时):第三方压 WARN,其余 INFO。
const DEFAULT_FILTER: &str = "info,reqwest=warn,hyper=warn,tokio=warn,tungstenite=warn,rusqlite=warn";

/// 初始化日志。返回 WorkerGuard(必须保活到进程结束)。
///
/// 失败降级:文件 appender 起不来 → 只用 stderr 层,返回 None。
/// 调用方对 None 不需要特殊处理(stderr 层已挂,只是没文件)。
pub fn init_logging() -> Option<WorkerGuard> {
    let logs = logs_dir();

    // 先建日志目录(rolling appender 不会自动建父目录)。
    // 建目录失败 → 降级 stderr-only,不阻断 app。
    let file_result = std::fs::create_dir_all(&logs).map(|_| {
        let file_appender = tracing_appender::rolling::daily(&logs, "jarvis.log");
        // non_blocking 包装:避免磁盘 IO 阻塞业务线程。
        let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
        (non_blocking, guard)
    });

    let (file_layer, guard) = match file_result {
        Ok((non_blocking, guard)) => {
            let filter = build_filter();
            let layer = fmt::layer()
                .with_ansi(false) // 文件里不要 ANSI 转义码
                .with_target(true)
                .with_writer(non_blocking)
                .with_filter(filter);
            (Some(layer), Some(guard))
        }
        Err(e) => {
            // 降级:只 stderr。不阻断 app,但要留痕 —— 此时 tracing 还没起,只能 eprintln。
            eprintln!(
                "[logging] 无法创建日志文件 appender({}),降级为 stderr-only",
                e
            );
            (None, None)
        }
    };

    // stderr 层:带颜色,开发模式终端可见。用同一份过滤策略。
    let stderr_filter = build_filter();
    let stderr_layer = fmt::layer()
        .with_ansi(true)
        .with_target(true)
        .with_filter(stderr_filter);

    let registry = tracing_subscriber::registry()
        .with(stderr_layer)
        .with(file_layer);

    // try_init:全局只能 set 一次。已 set(如测试里)返回 Err,这里 ok() 忽略。
    if registry.try_init().is_err() {
        eprintln!("[logging] tracing subscriber 已被初始化(可能是测试环境),跳过");
        return guard;
    }

    // 启动留痕:确认日志基础设施起来了(这条会同时进 stderr 和文件)。
    tracing::info!(target: "logging", "日志系统已初始化,日志目录: {}", logs.display());

    guard
}

/// 构建过滤策略。RUST_LOG 优先(开发调试),否则用静态默认(第三方压 WARN)。
fn build_filter() -> EnvFilter {
    match EnvFilter::try_from_default_env() {
        Ok(f) => f,
        Err(_) => EnvFilter::new(DEFAULT_FILTER),
    }
}

// ===== 诊断日志导出 =====
//
// 用户报 bug 时,点设置页「导出诊断日志」按钮 → 弹保存框 → 生成 zip。
// zip 含:最近 N 天的日志文件 + 一份环境摘要(app 版本、OS、功能开关状态——脱敏)。
// 红线:绝不含密钥链内容、apiKey、密码。摘要只记开关 on/off 和版本号。

use std::io::{Read, Write};
use zip::write::ZipWriter;

const DIAGNOSTIC_LOG_RETENTION_DAYS: i64 = 3;

/// 导出诊断日志。返回保存的文件路径(给前端提示用)。
///
/// 流程:收集最近 N 天日志 → 生成脱敏环境摘要 → 打包 zip → rfd 弹保存框。
pub fn export_diagnostic_logs() -> Result<String, String> {
    let logs = logs_dir();

    // 1. 收集最近 N 天的日志文件(按文件名日期过滤,容错:无文件也继续)。
    let mut entries: Vec<PathBuf> = Vec::new();
    if logs.exists() {
        let cutoff = chrono::Utc::now().date_naive() - chrono::Duration::days(DIAGNOSTIC_LOG_RETENTION_DAYS);
        for entry in std::fs::read_dir(&logs).map_err(|e| format!("读取日志目录失败: {e}"))? {
            let Ok(entry) = entry else { continue };
            let path = entry.path();
            // 文件名形如 jarvis.log.2026-06-15 或 jarvis.log(当天)
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if let Some(date_str) = name.strip_prefix("jarvis.log.") {
                    if let Ok(d) = chrono::NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
                        if d >= cutoff {
                            entries.push(path);
                        }
                        continue;
                    }
                }
                // 当天日志(无日期后缀)也收
                if name == "jarvis.log" {
                    entries.push(path);
                }
            }
        }
    }

    // 2. 生成脱敏环境摘要
    let summary = build_environment_summary();

    // 3. 打包 zip(在内存里,再交给 rfd 保存)
    let zip_bytes = build_diagnostic_zip(&entries, &summary)?;

    // 4. rfd 弹保存框
    let filename = format!(
        "jarvis-diagnostic-{}.zip",
        chrono::Utc::now().format("%Y%m%d-%H%M%S")
    );
    let save_path = rfd::FileDialog::new()
        .set_file_name(&filename)
        .add_filter("ZIP", &["zip"])
        .save_file()
        .ok_or_else(|| "用户取消了保存".to_string())?;

    std::fs::write(&save_path, &zip_bytes)
        .map_err(|e| format!("写入导出文件失败: {e}"))?;

    Ok(save_path.to_string_lossy().to_string())
}

/// 生成脱敏环境摘要。只含版本、OS、功能开关状态,**绝不含任何 secret**。
fn build_environment_summary() -> String {
    let mut s = String::new();
    s.push_str("# Jarvis 诊断环境摘要\n\n");
    s.push_str(&format!(
        "- 生成时间: {}\n",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S %:z")
    ));
    s.push_str(&format!("- app 版本: {}\n", env!("CARGO_PKG_VERSION")));
    s.push_str(&format!("- OS: {}\n", std::env::consts::OS));
    s.push_str(&format!("- OS 架构: {}\n", std::env::consts::ARCH));

    // 读取 config.json 的开关状态(脱敏:只看布尔开关,不读任何字符串凭据)。
    if let Ok(content) = std::fs::read_to_string(crate::settings::config_path()) {
        s.push_str("\n## 功能开关状态(脱敏,只记 on/off)\n");
        if let Ok(cfg) = serde_json::from_str::<serde_json::Value>(&content) {
            // 只提取已知的布尔开关字段,显式跳过任何含 key/token/secret/password 的字段。
            collect_bool_switches(&cfg, &mut s, "");
        } else {
            s.push_str("(config.json 解析失败)\n");
        }
    } else {
        s.push_str("\n(config.json 不存在或不可读)\n");
    }

    s.push_str("\n## 密钥链状态(只记存在性,不含内容)\n");
    for account_label in ["zentao", "fineReport", "llm"] {
        // 这里只检测密钥是否已设(secret_get 不调用,避免任何泄露风险)。
        // 用 secret_exists 而非 secret_get。
        let exists = crate::settings::secret_exists(account_label);
        s.push_str(&format!("- {}: {}\n", account_label, if exists { "已配置" } else { "未配置" }));
    }

    s
}

/// 递归收集 JSON 里的布尔字段(开关)。跳过任何疑似 secret 的 key。
fn collect_bool_switches(value: &serde_json::Value, out: &mut String, prefix: &str) {
    match value {
        serde_json::Value::Object(map) => {
            for (k, v) in map {
                // 脱敏红线:跳过疑似 secret 的 key(大小写不敏感)。
                let k_lower = k.to_lowercase();
                if k_lower.contains("key")
                    || k_lower.contains("token")
                    || k_lower.contains("secret")
                    || k_lower.contains("password")
                    || k_lower.contains("account")
                    || k_lower.contains("url")
                    || k_lower.contains("baseurl")
                    || k_lower.contains("appid")
                {
                    continue;
                }
                let path = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                collect_bool_switches(v, out, &path);
            }
        }
        serde_json::Value::Bool(b) => {
            out.push_str(&format!("- {prefix}: {b}\n"));
        }
        _ => {} // 非布尔、非对象:跳过(字符串/数字可能是敏感信息)
    }
}

/// 把日志文件 + 摘要打包成 zip 字节。
fn build_diagnostic_zip(log_files: &[PathBuf], summary: &str) -> Result<Vec<u8>, String> {
    let buf = std::io::Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(buf);
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    // 1. 写环境摘要
    zip.start_file("environment.txt", options)
        .map_err(|e| format!("zip 写摘要失败: {e}"))?;
    zip.write_all(summary.as_bytes())
        .map_err(|e| format!("zip 写摘要失败: {e}"))?;

    // 2. 写日志文件
    for path in log_files {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown.log");
        // 统一放到 logs/ 子目录,避免和摘要混在一起。
        let entry_name = format!("logs/{name}");
        if let Ok(mut file) = std::fs::File::open(path) {
            zip.start_file(&entry_name, options)
                .map_err(|e| format!("zip 写日志条目失败: {e}"))?;
            let mut buf = Vec::new();
            if file.read_to_end(&mut buf).is_ok() {
                let _ = zip.write_all(&buf);
            }
        }
    }

    let buf = zip
        .finish()
        .map_err(|e| format!("zip 收尾失败: {e}"))?;
    Ok(buf.into_inner())
}
