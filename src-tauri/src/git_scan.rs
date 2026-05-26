// 本地 git 仓库扫描 + commit 查询。
//
// 移植自 src/services/local-git/{scan,index}.ts。设计原则同 TS 版：
// - 只调外部 git 进程（spawn），不引第三方 git 库
// - 路径用 PathBuf；日期用 RFC3339 字符串（与 git --pretty=%aI/%cI 输出一致）
// - 字段命名跟 TS 一一对齐，前端契约不变
//
// 调用入口：list_my_local_commits（高级 API）和 get_local_commit_diff（diff 接口）。

#![allow(dead_code)]

use chrono::{Datelike, Duration as ChronoDuration, Local, TimeZone};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;

const RECORD_SEP: char = '\x1e';
const FIELD_SEP: char = '\x1f';

const SKIP_DIRS: &[&str] = &[
    "node_modules",
    "dist",
    "build",
    "out",
    ".next",
    ".cache",
    ".idea",
    ".vscode",
    "target",
    "venv",
    ".venv",
    "__pycache__",
    ".gradle",
];

// ============================================================================
// 类型
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalCommit {
    pub sha: String,
    pub short_sha: String,
    pub author_name: String,
    pub author_email: String,
    pub authored_date: String,
    pub committer_name: String,
    pub committer_email: String,
    pub committed_date: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stat: Option<CommitStat>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitStat {
    pub files_changed: usize,
    pub insertions: usize,
    pub deletions: usize,
    pub files: Vec<CommitStatFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitStatFile {
    pub path: String,
    pub insertions: usize,
    pub deletions: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub binary: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoCommits {
    pub repo_path: String,
    pub commits: Vec<LocalCommit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DateRange {
    pub since: String,
    pub until: String,
    pub label: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchDimension {
    Author,
    Committer,
    Any,
}

impl MatchDimension {
    pub fn parse(s: &str) -> Self {
        match s {
            "committer" => Self::Committer,
            "any" => Self::Any,
            _ => Self::Author,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum RangePreset {
    Today,
    Yesterday,
    ThisWeek,
    LastWeek,
    Last7Days,
    Last30Days,
    ThisMonth,
    All,
}

impl RangePreset {
    pub fn parse(s: &str) -> Self {
        match s {
            "yesterday" => Self::Yesterday,
            "thisWeek" => Self::ThisWeek,
            "lastWeek" => Self::LastWeek,
            "last7days" => Self::Last7Days,
            "last30days" => Self::Last30Days,
            "thisMonth" => Self::ThisMonth,
            "all" => Self::All,
            _ => Self::Today,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitDiff {
    pub sha: String,
    pub author_name: String,
    pub author_email: String,
    pub authored_date: String,
    pub title: String,
    pub body: String,
    pub stat: CommitStat,
    pub patch: String,
}

// ============================================================================
// 日期范围（本地时区）
// ============================================================================

/// 复刻 TS 行为：返回 ISO 字符串，对 git --since/--until 而言任何 ISO 都可解析。
pub fn get_date_range(preset: RangePreset) -> DateRange {
    let now = Local::now();
    let today_start = Local
        .with_ymd_and_hms(now.year(), now.month(), now.day(), 0, 0, 0)
        .single()
        .unwrap_or(now);
    let tomorrow_start = today_start + ChronoDuration::days(1);

    let (start, end) = match preset {
        RangePreset::Today => (today_start, tomorrow_start),
        RangePreset::Yesterday => (today_start - ChronoDuration::days(1), today_start),
        RangePreset::ThisWeek => {
            // ISO 周：周一为首日
            let dow0 = today_start.weekday().num_days_from_monday() as i64;
            (today_start - ChronoDuration::days(dow0), tomorrow_start)
        }
        RangePreset::LastWeek => {
            let dow0 = today_start.weekday().num_days_from_monday() as i64;
            let end = today_start - ChronoDuration::days(dow0);
            (end - ChronoDuration::days(7), end)
        }
        RangePreset::Last7Days => (today_start - ChronoDuration::days(7), tomorrow_start),
        RangePreset::Last30Days => (today_start - ChronoDuration::days(30), tomorrow_start),
        RangePreset::ThisMonth => {
            let first = Local
                .with_ymd_and_hms(now.year(), now.month(), 1, 0, 0, 0)
                .single()
                .unwrap_or(today_start);
            (first, tomorrow_start)
        }
        RangePreset::All => {
            // 起点 2000-01-01：足够久远
            let epoch = Local
                .with_ymd_and_hms(2000, 1, 1, 0, 0, 0)
                .single()
                .unwrap_or(today_start);
            (epoch, tomorrow_start)
        }
    };

    let label = match preset {
        RangePreset::All => "全部".to_string(),
        _ => format!(
            "{} ~ {}",
            start.format("%Y-%m-%d"),
            (end - ChronoDuration::milliseconds(1)).format("%Y-%m-%d")
        ),
    };

    DateRange {
        since: start.to_rfc3339(),
        until: end.to_rfc3339(),
        label,
    }
}

// ============================================================================
// git 子进程
// ============================================================================

/// 跑 git 命令，返回 stdout。失败抛错文本含 exit code + stderr。
async fn run_git_cmd(args: &[&str], cwd: Option<&Path>) -> Result<String, String> {
    let mut cmd = Command::new("git");
    cmd.args(args).stdin(Stdio::null());
    if let Some(c) = cwd {
        cmd.current_dir(c);
    }
    #[cfg(windows)]
    {
        // tokio::process::Command 在 Windows 上自带 creation_flags 方法，
        // 不需要 std::os::windows::process::CommandExt
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
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

// ============================================================================
// 仓库发现
// ============================================================================

pub async fn find_git_repos(roots: &[String], max_depth: usize) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    for root in roots {
        walk(Path::new(root), 0, max_depth, &mut found, &mut seen).await;
    }
    found
}

// 用 BoxFuture 包装递归 async：tokio::fs::read_dir + 自递归
fn walk<'a>(
    dir: &'a Path,
    depth: usize,
    max_depth: usize,
    found: &'a mut Vec<String>,
    seen: &'a mut std::collections::HashSet<String>,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> {
    Box::pin(async move {
        if depth > max_depth {
            return;
        }
        let mut entries = match tokio::fs::read_dir(dir).await {
            Ok(rd) => rd,
            Err(_) => return,
        };

        // 第一遍：判定是否是 git 仓（.git 既可以是目录也可以是文件）
        let mut subdirs: Vec<PathBuf> = Vec::new();
        let mut is_repo = false;
        while let Ok(Some(entry)) = entries.next_entry().await {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name == ".git" {
                is_repo = true;
            }
            if let Ok(ft) = entry.file_type().await {
                if ft.is_dir() {
                    let n = name.to_string();
                    if n.starts_with('.') || SKIP_DIRS.contains(&n.as_str()) {
                        continue;
                    }
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

        for sub in subdirs {
            walk(&sub, depth + 1, max_depth, found, seen).await;
        }
    })
}

// ============================================================================
// 身份解析
// ============================================================================

pub async fn get_default_git_identities(cwd: Option<&Path>) -> Vec<String> {
    let read_one = |args: Vec<&'static str>| async move {
        run_git_cmd(&args, cwd).await.ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
    };
    let email = read_one(vec!["config", "user.email"]).await.or(read_one(vec!["config", "--global", "user.email"]).await);
    let name = read_one(vec!["config", "user.name"]).await.or(read_one(vec!["config", "--global", "user.name"]).await);

    let mut out: Vec<String> = Vec::new();
    if let Some(e) = email.clone() {
        out.push(e);
    }
    if let Some(n) = name {
        if Some(&n) != email.as_ref() {
            out.push(n);
        }
    }
    out
}

// ============================================================================
// 提交查询
// ============================================================================

pub struct GetCommitsOpts<'a> {
    pub authors: &'a [String],
    pub since: &'a str,
    pub until: &'a str,
    pub match_mode: MatchDimension,
    pub include_body: bool,
    pub include_stat: bool,
}

pub async fn get_local_commits(repo: &str, opts: GetCommitsOpts<'_>) -> Result<Vec<LocalCommit>, String> {
    // 拼 pretty format
    let mut fields = vec!["%H", "%an", "%ae", "%aI", "%cn", "%ce", "%cI", "%s"];
    if opts.include_body {
        fields.push("%b");
    }
    let format = format!("{}{}", fields.join(&FIELD_SEP.to_string()), RECORD_SEP);

    let since_arg = format!("--since={}", opts.since);
    let until_arg = format!("--until={}", opts.until);
    let pretty_arg = format!("--pretty=format:{}", format);

    let mut args: Vec<String> = vec![
        "log".into(),
        "--all".into(),
        "--no-merges".into(),
        since_arg,
        until_arg,
        pretty_arg,
    ];
    let author_args: Vec<String> = match opts.match_mode {
        MatchDimension::Author => opts.authors.iter().filter(|s| !s.is_empty()).map(|s| format!("--author={}", s)).collect(),
        MatchDimension::Committer => opts.authors.iter().filter(|s| !s.is_empty()).map(|s| format!("--committer={}", s)).collect(),
        MatchDimension::Any => Vec::new(),
    };
    args.extend(author_args);

    let str_args: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let out = run_git_cmd(&str_args, Some(Path::new(repo))).await?;
    if out.is_empty() {
        return Ok(Vec::new());
    }

    let mut commits: Vec<LocalCommit> = out
        .split(RECORD_SEP)
        .map(|line| line.trim_start_matches('\n').to_string())
        .filter(|s| !s.is_empty())
        .filter_map(|line| {
            let parts: Vec<&str> = line.split(FIELD_SEP).collect();
            if parts.len() < 8 {
                return None;
            }
            let sha = parts[0].trim().to_string();
            let short_sha = if sha.len() > 7 { sha[..7].to_string() } else { sha.clone() };
            let body = if opts.include_body {
                let body_str = parts.iter().skip(8).cloned().collect::<Vec<&str>>().join(&FIELD_SEP.to_string()).trim().to_string();
                if body_str.is_empty() { None } else { Some(body_str) }
            } else {
                None
            };
            Some(LocalCommit {
                sha,
                short_sha,
                author_name: parts[1].to_string(),
                author_email: parts[2].to_string(),
                authored_date: parts[3].to_string(),
                committer_name: parts[4].to_string(),
                committer_email: parts[5].to_string(),
                committed_date: parts[6].to_string(),
                title: parts[7].to_string(),
                body,
                stat: None,
            })
        })
        .collect();

    // any 模式：JS 端事后 filter（git 不支持 author OR committer 同时匹配）
    if matches!(opts.match_mode, MatchDimension::Any) && !opts.authors.is_empty() {
        let patterns: Vec<String> = opts.authors.iter().map(|s| s.to_lowercase()).collect();
        commits.retain(|c| {
            let fields = [&c.author_email, &c.author_name, &c.committer_email, &c.committer_name];
            patterns.iter().any(|p| fields.iter().any(|f| f.to_lowercase().contains(p)))
        });
    }

    if opts.include_stat && !commits.is_empty() {
        // 并发 4 路跑 numstat
        let sha_list: Vec<String> = commits.iter().map(|c| c.sha.clone()).collect();
        let stats = run_concurrent_stats(repo, &sha_list, 4).await;
        for (c, s) in commits.iter_mut().zip(stats.into_iter()) {
            c.stat = s;
        }
    }

    Ok(commits)
}

async fn run_concurrent_stats(repo: &str, shas: &[String], limit: usize) -> Vec<Option<CommitStat>> {
    use std::sync::Arc;
    use tokio::sync::Semaphore;

    let sem = Arc::new(Semaphore::new(limit));
    let mut handles = Vec::with_capacity(shas.len());
    for sha in shas {
        let permit = sem.clone();
        let repo = repo.to_string();
        let sha = sha.clone();
        handles.push(tokio::spawn(async move {
            let _g = permit.acquire_owned().await.ok();
            get_commit_stat(&repo, &sha).await.ok()
        }));
    }
    let mut out = Vec::with_capacity(shas.len());
    for h in handles {
        out.push(h.await.ok().flatten());
    }
    out
}

pub async fn get_commit_stat(repo: &str, sha: &str) -> Result<CommitStat, String> {
    let out = run_git_cmd(&["show", "--numstat", "--format=", sha], Some(Path::new(repo))).await?;
    let mut files: Vec<CommitStatFile> = Vec::new();
    let mut insertions = 0usize;
    let mut deletions = 0usize;
    for line in out.split('\n') {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let parts: Vec<&str> = trimmed.splitn(3, '\t').collect();
        if parts.len() < 3 {
            continue;
        }
        let add_str = parts[0];
        let del_str = parts[1];
        let path = parts[2].to_string();
        if path.is_empty() {
            continue;
        }
        let binary = add_str == "-" && del_str == "-";
        let ins = if binary { 0 } else { add_str.parse().unwrap_or(0) };
        let del = if binary { 0 } else { del_str.parse().unwrap_or(0) };
        files.push(CommitStatFile {
            path,
            insertions: ins,
            deletions: del,
            binary: if binary { Some(true) } else { None },
        });
        insertions += ins;
        deletions += del;
    }
    Ok(CommitStat {
        files_changed: files.len(),
        insertions,
        deletions,
        files,
    })
}

pub async fn get_commit_diff(repo: &str, sha: &str) -> Result<CommitDiff, String> {
    let meta_format = ["%H", "%an", "%ae", "%aI", "%s", "%b"].join(&FIELD_SEP.to_string());
    let format_arg = format!("--format={}", meta_format);
    let meta_out = run_git_cmd(&["show", "-s", &format_arg, sha], Some(Path::new(repo))).await?;
    let meta = meta_out.trim_end_matches('\n');
    let parts: Vec<&str> = meta.split(FIELD_SEP).collect();
    if parts.len() < 5 {
        return Err(format!("git show meta 输出格式异常: {}", &meta[..meta.len().min(200)]));
    }
    let sha_out = parts[0].trim().to_string();
    let an = parts[1].to_string();
    let ae = parts[2].to_string();
    let ad = parts[3].to_string();
    let title = parts[4].to_string();
    let body = parts.iter().skip(5).cloned().collect::<Vec<&str>>().join(&FIELD_SEP.to_string()).trim().to_string();

    let stat = get_commit_stat(repo, sha).await?;
    let patch_raw = run_git_cmd(&["show", "--format=", sha], Some(Path::new(repo))).await?;
    let patch = patch_raw.trim_start_matches('\n').to_string();

    Ok(CommitDiff {
        sha: if sha_out.is_empty() { sha.to_string() } else { sha_out },
        author_name: an,
        author_email: ae,
        authored_date: ad,
        title,
        body,
        stat,
        patch,
    })
}

// ============================================================================
// 高级 API（对齐 TS listMyLocalCommits / getLocalCommitDiff）
// ============================================================================

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListMyLocalCommitsResult {
    pub range: DateRange,
    pub authors: Vec<String>,
    pub root_dirs: Vec<String>,
    pub repos: Vec<RepoCommits>,
    pub total_commits: usize,
    pub scanned_repos: usize,
    pub repos_with_commits: usize,
}

pub struct ListMyLocalCommitsInput<'a> {
    pub root_dirs: &'a [String],
    pub range: RangePreset,
    pub since: Option<&'a str>,
    pub until: Option<&'a str>,
    pub author: Option<&'a str>,
    pub match_mode: MatchDimension,
    pub include_body: bool,
    pub include_stat: bool,
    pub max_depth: usize,
}

pub async fn list_my_local_commits(input: ListMyLocalCommitsInput<'_>) -> Result<ListMyLocalCommitsResult, String> {
    if input.root_dirs.is_empty() {
        return Err("缺少 rootDir。请在 settings 里配置代码根目录".into());
    }
    for d in input.root_dirs {
        let p = Path::new(d);
        match tokio::fs::metadata(p).await {
            Ok(m) if m.is_dir() => {}
            _ => return Err(format!("rootDir 不存在或不是目录: {}", d)),
        }
    }
    ensure_git_available()
        .await
        .map_err(|e| format!("未找到 git，请确认 git 已安装并在 PATH 中。原始错误: {}", e))?;

    let preset_range = get_date_range(input.range);
    let since = input.since.unwrap_or(&preset_range.since).to_string();
    let until = input.until.unwrap_or(&preset_range.until).to_string();

    let authors: Vec<String> = if let Some(a) = input.author {
        a.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
    } else {
        get_default_git_identities(Some(Path::new(&input.root_dirs[0]))).await
    };

    let repos = find_git_repos(input.root_dirs, input.max_depth).await;
    let scanned_repos = repos.len();
    if repos.is_empty() {
        return Ok(ListMyLocalCommitsResult {
            range: preset_range,
            authors,
            root_dirs: input.root_dirs.to_vec(),
            repos: Vec::new(),
            total_commits: 0,
            scanned_repos: 0,
            repos_with_commits: 0,
        });
    }

    // 并发拉每个 repo 的 commits（concurrency 8，对齐 TS）
    let mut handles = Vec::with_capacity(repos.len());
    let authors_arc = std::sync::Arc::new(authors.clone());
    let since_arc = std::sync::Arc::new(since.clone());
    let until_arc = std::sync::Arc::new(until.clone());
    let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(8));
    let match_mode = input.match_mode;
    let include_body = input.include_body;
    let include_stat = input.include_stat;

    for repo in &repos {
        let repo = repo.clone();
        let authors = authors_arc.clone();
        let since = since_arc.clone();
        let until = until_arc.clone();
        let permit = sem.clone();
        handles.push(tokio::spawn(async move {
            let _g = permit.acquire_owned().await.ok();
            let opts = GetCommitsOpts {
                authors: &authors,
                since: &since,
                until: &until,
                match_mode,
                include_body,
                include_stat,
            };
            let commits = get_local_commits(&repo, opts).await.unwrap_or_default();
            RepoCommits { repo_path: repo, commits }
        }));
    }

    let mut repo_results: Vec<RepoCommits> = Vec::with_capacity(handles.len());
    for h in handles {
        if let Ok(r) = h.await {
            if !r.commits.is_empty() {
                repo_results.push(r);
            }
        }
    }
    // 每仓内按 authoredDate desc
    for r in &mut repo_results {
        r.commits.sort_by(|a, b| b.authored_date.cmp(&a.authored_date));
    }
    // 仓之间按最新 commit 时间 desc
    repo_results.sort_by(|a, b| {
        let av = a.commits.first().map(|c| c.authored_date.clone()).unwrap_or_default();
        let bv = b.commits.first().map(|c| c.authored_date.clone()).unwrap_or_default();
        bv.cmp(&av)
    });

    let total_commits = repo_results.iter().map(|r| r.commits.len()).sum();
    let repos_with_commits = repo_results.len();

    Ok(ListMyLocalCommitsResult {
        range: DateRange { since, until, label: preset_range.label },
        authors,
        root_dirs: input.root_dirs.to_vec(),
        repos: repo_results,
        total_commits,
        scanned_repos,
        repos_with_commits,
    })
}

// ============================================================================
// 配置读取（业务线别名 + 排除）
// ============================================================================

/// 业务线别名表：业务线名 → 补充关键词列表。
pub fn load_business_aliases() -> std::collections::HashMap<String, Vec<String>> {
    let path = crate::settings::jarvis_dir().join("business-aliases.json");

    // 文件不存在则写一份默认值（对齐 TS：ensureFileExists）
    if !path.exists() {
        let _ = std::fs::create_dir_all(crate::settings::jarvis_dir());
        let default = serde_json::json!({ "示例业务线": ["门禁", "计量"] });
        let _ = std::fs::write(&path, serde_json::to_string_pretty(&default).unwrap_or_default());
    }

    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return std::collections::HashMap::new(),
    };
    let parsed: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return std::collections::HashMap::new(),
    };
    let mut out: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
    if let Some(obj) = parsed.as_object() {
        for (k, v) in obj {
            if let Some(arr) = v.as_array() {
                let kws: Vec<String> = arr
                    .iter()
                    .filter_map(|x| x.as_str())
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                if !kws.is_empty() {
                    out.insert(k.clone(), kws);
                }
            }
        }
    }
    out
}

/// 排除的业务线集合（仓库的"业务线名"在这里就完全不计工作量）。
pub fn load_excluded_business_lines() -> std::collections::HashSet<String> {
    let path = crate::settings::jarvis_dir().join("excluded-business-lines.json");
    if !path.exists() {
        let _ = std::fs::create_dir_all(crate::settings::jarvis_dir());
        let default = serde_json::json!(["my-mcp-servers"]);
        let _ = std::fs::write(&path, serde_json::to_string_pretty(&default).unwrap_or_default());
    }
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return std::collections::HashSet::new(),
    };
    let parsed: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return std::collections::HashSet::new(),
    };
    let mut out = std::collections::HashSet::new();
    if let Some(arr) = parsed.as_array() {
        for x in arr {
            if let Some(s) = x.as_str() {
                let s = s.trim();
                if !s.is_empty() {
                    out.insert(s.to_string());
                }
            }
        }
    }
    out
}

/// 从 settings + env 取 repoRoots（对齐 TS getRepoRoots）。
pub fn get_repo_roots() -> Vec<String> {
    let cfg = crate::settings::load_raw_config();
    let from_cfg: Vec<String> = cfg
        .as_ref()
        .and_then(|v| v.get("repoRoots"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();
    if !from_cfg.is_empty() {
        return from_cfg;
    }
    // env 兜底
    if let Ok(raw) = std::env::var("TENCENT_CODE_LOCAL_ROOTS") {
        return raw
            .split(|c| c == ';' || c == ',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
    }
    Vec::new()
}

// ============================================================================
// commit cleanup + effort 估算（对齐 src/services/{clean-commit-title,commit-effort}.ts）
// ============================================================================

/// 清理 commit 标题：剥 emoji + conventional commit 前缀，截断到 maxLen。
pub fn clean_commit_title(title: &str, max_len: usize) -> String {
    if title.is_empty() {
        return String::new();
    }
    // 反复剥，因为顺序可能是 "emoji prefix" 或 "prefix emoji"
    let mut s = title.to_string();
    for _ in 0..3 {
        let before = s.clone();
        s = strip_leading_emoji(&s);
        s = strip_cc_prefix(&s);
        if s == before {
            break;
        }
    }
    s = s.trim().to_string();
    if s.chars().count() > max_len {
        let mut truncated: String = s.chars().take(max_len.saturating_sub(1)).collect();
        truncated = truncated.trim_end().to_string();
        truncated.push('…');
        s = truncated;
    }
    s
}

fn strip_leading_emoji(s: &str) -> String {
    // 简单策略：跳过开头连续的非字母数字非 ASCII 字符（覆盖 emoji + variation selector）
    let mut iter = s.char_indices();
    let mut cut = 0usize;
    while let Some((i, c)) = iter.next() {
        if c.is_whitespace() && cut > 0 {
            cut = i + c.len_utf8();
            continue;
        }
        // 非 ASCII 且不是中日韩文字 → 视为 emoji/符号剥掉
        if !c.is_ascii() && !is_cjk(c) {
            cut = i + c.len_utf8();
            continue;
        }
        // ASCII 字母/数字/常规标点：开始正文
        break;
    }
    s[cut..].to_string()
}

fn is_cjk(c: char) -> bool {
    let cp = c as u32;
    // 中日韩统一表意 + 常用全角符号（不全，但够用）
    (0x4E00..=0x9FFF).contains(&cp) || (0x3400..=0x4DBF).contains(&cp) || (0x3000..=0x303F).contains(&cp)
}

fn strip_cc_prefix(s: &str) -> String {
    use regex::Regex;
    // 同步 desktop/src/composables/cleanCommitTitle.ts
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
            if f.binary.unwrap_or(false) {
                continue;
            }
            if is_generated_path(&f.path) {
                continue;
            }
            loc += f.insertions + f.deletions;
        }
    } else {
        loc = stat.insertions + stat.deletions;
    }
    1.0 + (loc as f64).sqrt() / 10.0
}

pub fn is_generated_path(p: &str) -> bool {
    let lower = p.to_lowercase().replace('\\', "/");
    // 锁文件
    let lock_files = [
        "package-lock.json",
        "yarn.lock",
        "pnpm-lock.yaml",
        "cargo.lock",
        "composer.lock",
        "poetry.lock",
        "gemfile.lock",
        "go.sum",
        "bun.lockb",
    ];
    let last_seg = lower.rsplit('/').next().unwrap_or("");
    if lock_files.iter().any(|f| last_seg == *f) {
        return true;
    }
    if lower.ends_with(".min.js") || lower.ends_with(".min.css") {
        return true;
    }
    if lower.ends_with(".map") {
        return true;
    }
    for dir in ["node_modules", "dist", "build", "out", ".next", "target", ".cache", ".turbo"] {
        let pat = format!("/{}/", dir);
        if lower.starts_with(&format!("{}/", dir)) || lower.contains(&pat) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_title_strips_cc_prefix() {
        assert_eq!(clean_commit_title("feat: add login", 60), "add login");
        assert_eq!(clean_commit_title("feat(api): add login", 60), "add login");
        assert_eq!(clean_commit_title("FIX: bug", 60), "bug");
    }

    #[test]
    fn clean_title_strips_emoji() {
        // 中文应保留
        assert_eq!(clean_commit_title("✨ feat: 新增登录页", 60), "新增登录页");
        assert_eq!(clean_commit_title("🐛 修复 bug", 60), "修复 bug");
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
            sha: "abc".into(),
            short_sha: "abc".into(),
            author_name: "".into(),
            author_email: "".into(),
            authored_date: "".into(),
            committer_name: "".into(),
            committer_email: "".into(),
            committed_date: "".into(),
            title: "".into(),
            body: None,
            stat: None,
        };
        assert_eq!(effort_for_commit(&c), 1.0);
    }
}
