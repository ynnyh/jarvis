//! 跨模块复用的小工具。

use std::path::Path;

/// 按「字符」取前 `n` 个 char 返回 String。
///
/// 用来替代 `&s[..s.len().min(n)]` 这类按字节切片——后者在截断点落到多字节
/// UTF-8 字符中间时会 panic。本项目 release 档开了 `panic = "abort"`，任何
/// panic 都会直接杀掉整个进程，而错误信息里截断中文响应（禅道 / LLM 返回）
/// 又极常见，因此所有展示用截断都必须走字符边界。
pub fn truncate_chars(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// 原子写文件：先写同目录临时文件，再 rename 覆盖目标。
///
/// 避免 `fs::write` 直接覆盖时若写到一半崩溃 / 断电，留下被截断、无法解析的
/// 损坏文件（config.json / 会话 / 绑定表损坏代价都很高）。同卷 rename 在
/// Windows（MoveFileEx + REPLACE_EXISTING）与 macOS 上都是原子替换。
pub fn write_atomic(path: &Path, contents: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("jarvis");
    let tmp = path.with_file_name(format!(".{}.tmp{}", file_name, std::process::id()));
    std::fs::write(&tmp, contents)?;
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_chars_at_char_boundary() {
        assert_eq!(truncate_chars("禅道写工时失败", 3), "禅道写");
        assert_eq!(truncate_chars("abc", 10), "abc");
        assert_eq!(truncate_chars("", 5), "");
        assert_eq!(truncate_chars("a禅b", 2), "a禅");
    }

    #[test]
    fn write_atomic_creates_and_overwrites() {
        let dir = std::env::temp_dir().join(format!("jarvis_wa_{}", std::process::id()));
        let p = dir.join("cfg.json");
        write_atomic(&p, "{\"v\":1}").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "{\"v\":1}");
        write_atomic(&p, "{\"v\":2}").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "{\"v\":2}");
        std::fs::remove_dir_all(&dir).ok();
    }
}
