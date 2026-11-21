use crate::git_scan::types::LocalCommit;

/// 清理 commit 标题：剥 emoji + conventional commit 前缀；不截断——
/// 工时/日报内容要求完整展示（UI 侧用限高+滚动/展开兜底）。
pub fn clean_commit_title(title: &str) -> String {
    if title.is_empty() { return String::new(); }
    let mut s = title.to_string();
    for _ in 0..3 {
        let before = s.clone();
        s = strip_leading_emoji(&s);
        s = strip_cc_prefix(&s);
        if s == before { break; }
    }
    s.trim().to_string()
}

/// 剥掉标题自带的序号前缀（"1、xxx" "2. xxx" "(3) xxx" "（4）xxx" "⑤xxx" "五、xxx"），
/// 避免拼装工时内容时出现「2、1、xxx」双编号。
/// "." 后必须跟空白才视为序号（防误伤 "1.5 版本发布" 这类小数）。
pub fn strip_leading_index(s: &str) -> String {
    use regex::Regex;
    let re = Regex::new(
        r"^(?:\s*(?:[0-9０-９]{1,3}\s*[、,，:：;；)）]\s*|[0-9０-９]{1,3}\.\s+|\([0-9]{1,3}\)\s*|（[0-9０-９]{1,3}）\s*|[①-⑳]\s*|[一二三四五六七八九十]{1,3}\s*[、.．]\s*))+",
    ).unwrap();
    re.replace(s, "").trim().to_string()
}

fn strip_leading_emoji(s: &str) -> String {
    let iter = s.char_indices();
    let mut cut = 0usize;
    for (i, c) in iter {
        if c.is_whitespace() && cut > 0 { cut = i + c.len_utf8(); continue; }
        if !c.is_ascii() && !is_cjk(c) { cut = i + c.len_utf8(); continue; }
        break;
    }
    s[cut..].to_string()
}

fn is_cjk(c: char) -> bool {
    let cp = c as u32;
    (0x4E00..=0x9FFF).contains(&cp)
        || (0x3400..=0x4DBF).contains(&cp)
        || (0x3000..=0x303F).contains(&cp)
}

fn strip_cc_prefix(s: &str) -> String {
    use regex::Regex;
    let re = Regex::new(r"(?i)^(feat|fix|refactor|build|chore|docs|test|style|perf|ci|revert|wip)(?:\([^)]+\))?!?\s*:\s*").unwrap();
    re.replace(s, "").to_string()
}

/// 估算单个 commit 的工作量分数：1 + sqrt(loc)/10，排除生成文件/二进制。
pub fn effort_for_commit(c: &LocalCommit) -> f64 {
    let stat = match &c.stat {
        Some(s) => s,
        None => return 1.0,
    };
    let mut loc = 0usize;
    if !stat.files.is_empty() {
        for f in &stat.files {
            if f.binary.unwrap_or(false) { continue; }
            if is_generated_path(&f.path) { continue; }
            loc += f.insertions + f.deletions;
        }
    } else {
        loc = stat.insertions + stat.deletions;
    }
    1.0 + (loc as f64).sqrt() / 10.0
}

pub(crate) fn is_generated_path(p: &str) -> bool {
    let lower = p.to_lowercase().replace('\\', "/");
    let lock_files = [
        "package-lock.json", "yarn.lock", "pnpm-lock.yaml", "cargo.lock",
        "composer.lock", "poetry.lock", "gemfile.lock", "go.sum", "bun.lockb",
    ];
    let last_seg = lower.rsplit('/').next().unwrap_or("");
    if lock_files.contains(&last_seg) { return true; }
    if lower.ends_with(".min.js") || lower.ends_with(".min.css") { return true; }
    if lower.ends_with(".map") { return true; }
    for dir in ["node_modules", "dist", "build", "out", ".next", "target", ".cache", ".turbo"] {
        let pat = format!("/{}/", dir);
        if lower.starts_with(&format!("{}/", dir)) || lower.contains(&pat) { return true; }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_title_strips_cc_prefix() {
        assert_eq!(clean_commit_title("feat: add login"), "add login");
        assert_eq!(clean_commit_title("feat(api): add login"), "add login");
        assert_eq!(clean_commit_title("FIX: bug"), "bug");
    }

    #[test]
    fn clean_title_strips_emoji() {
        assert_eq!(clean_commit_title("✨ feat: 新增登录页"), "新增登录页");
        assert_eq!(clean_commit_title("🐛 修复 bug"), "修复 bug");
    }

    #[test]
    fn clean_title_no_truncation() {
        let long = "长".repeat(500);
        assert_eq!(clean_commit_title(&long), long); // 不截断，完整保留
    }

    #[test]
    fn strip_leading_index_variants() {
        assert_eq!(strip_leading_index("1、修复登录"), "修复登录");
        assert_eq!(strip_leading_index("1、1、重复前缀"), "重复前缀"); // 重复剥
        assert_eq!(strip_leading_index("2. 修复登录"), "修复登录");
        assert_eq!(strip_leading_index("（3）修复登录"), "修复登录");
        assert_eq!(strip_leading_index("(4)修复登录"), "修复登录");
        assert_eq!(strip_leading_index("⑤修复登录"), "修复登录");
        assert_eq!(strip_leading_index("五、修复登录"), "修复登录");
        // 非序号数字不误伤
        assert_eq!(strip_leading_index("1.5 版本发布"), "1.5 版本发布");
        assert_eq!(strip_leading_index("60秒超时修改"), "60秒超时修改");
        assert_eq!(strip_leading_index("3D 渲染优化"), "3D 渲染优化");
    }

    #[test]
    fn generated_path_lockfiles() {
        assert!(is_generated_path("package-lock.json"));
        assert!(is_generated_path("frontend/pnpm-lock.yaml"));
        assert!(is_generated_path("dist/bundle.js"));
        assert!(is_generated_path("a/node_modules/foo/index.js"));
        assert!(!is_generated_path("src/main.rs"));
    }

    #[test]
    fn effort_no_stat() {
        let c = LocalCommit {
            sha: "abc".into(), short_sha: "abc".into(),
            author_name: "".into(), author_email: "".into(),
            authored_date: "".into(), committer_name: "".into(),
            committer_email: "".into(), committed_date: "".into(),
            title: "".into(), body: None, stat: None,
        };
        assert_eq!(effort_for_commit(&c), 1.0);
    }
}
