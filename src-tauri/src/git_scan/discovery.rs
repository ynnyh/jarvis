use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::time::Duration;
use tokio::process::Command;

const SKIP_DIRS: &[&str] = &[
    "node_modules", "dist", "build", "out", ".next", ".cache",
    ".idea", ".vscode", "target", "venv", ".venv", "__pycache__", ".gradle",
];

pub(crate) async fn run_git_cmd(args: &[&str], cwd: Option<&Path>) -> Result<String, String> {
    let mut cmd = Command::new("git");
    cmd.args(args).stdin(std::process::Stdio::null());
    if let Some(c) = cwd {
        cmd.current_dir(c);
    }
    #[cfg(windows)]
    {
        cmd.creation_flags(0x08000000);
    }

    let fut = cmd.output();
    let output = tokio::time::timeout(Duration::from_secs(15), fut)
        .await
        .map_err(|_| format!("git {} 超时 (15s)", args.join(" ")))?
        .map_err(|e| format!("git {} 启动失败: {}", args.join(" "), e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let code = output.status.code().map(|c| c.to_string()).unwrap_or_else(|| "?".into());
        return Err(format!("git {} 退出码 {}: {}", args.join(" "), code, stderr));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

pub async fn ensure_git_available() -> Result<(), String> {
    run_git_cmd(&["--version"], None).await.map(|_| ())
}

pub async fn find_git_repos(roots: &[String], max_depth: usize) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for root in roots {
        walk(Path::new(root), 0, max_depth, &mut found, &mut seen).await;
    }
    found
}

fn walk<'a>(
    dir: &'a Path,
    depth: usize,
    max_depth: usize,
    found: &'a mut Vec<String>,
    seen: &'a mut std::collections::HashSet<String>,
) -> Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> {
    Box::pin(async move {
        if depth > max_depth { return; }
        let mut entries = match tokio::fs::read_dir(dir).await {
            Ok(rd) => rd,
            Err(_) => return,
        };
        let mut subdirs: Vec<PathBuf> = Vec::new();
        let mut is_repo = false;
        while let Ok(Some(entry)) = entries.next_entry().await {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name == ".git" { is_repo = true; }
            if let Ok(ft) = entry.file_type().await {
                if ft.is_dir() {
                    let n = name.to_string();
                    if n.starts_with('.') || SKIP_DIRS.contains(&n.as_str()) { continue; }
                    subdirs.push(entry.path());
                }
            }
        }
        if is_repo {
            let path_str = dir.to_string_lossy().to_string();
            if !seen.contains(&path_str) {
                seen.insert(path_str.clone());
                found.push(path_str);
            }
            return;
        }
        for sub in subdirs { walk(&sub, depth + 1, max_depth, found, seen).await; }
    })
}

pub async fn get_default_git_identities(cwd: Option<&Path>) -> Vec<String> {
    let read_one = |args: Vec<&'static str>| async move {
        run_git_cmd(&args, cwd).await.ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    };
    let email = read_one(vec!["config", "user.email"]).await
        .or(read_one(vec!["config", "--global", "user.email"]).await);
    let name = read_one(vec!["config", "user.name"]).await
        .or(read_one(vec!["config", "--global", "user.name"]).await);
    let mut out: Vec<String> = Vec::new();
    if let Some(e) = email.clone() { out.push(e); }
    if let Some(n) = name { if Some(&n) != email.as_ref() { out.push(n); } }
    out
}
