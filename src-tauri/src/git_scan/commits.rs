use std::path::Path;
use std::sync::Arc;
use tokio::sync::Semaphore;

use crate::git_scan::discovery::{find_git_repos, get_default_git_identities, run_git_cmd, ensure_git_available};
use crate::git_scan::types::*;

const RECORD_SEP: char = '\x1e';
const FIELD_SEP: char = '\x1f';

pub fn get_date_range(preset: RangePreset) -> DateRange {
    use chrono::{Datelike, Duration as ChronoDuration, Local, TimeZone};
    let now = Local::now();
    let today_start = Local
        .with_ymd_and_hms(now.year(), now.month(), now.day(), 0, 0, 0)
        .single().unwrap_or(now);
    let tomorrow_start = today_start + ChronoDuration::days(1);
    let (start, end) = match preset {
        RangePreset::Today => (today_start, tomorrow_start),
        RangePreset::Yesterday => (today_start - ChronoDuration::days(1), today_start),
        RangePreset::ThisWeek => {
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
            let first = Local.with_ymd_and_hms(now.year(), now.month(), 1, 0, 0, 0)
                .single().unwrap_or(today_start);
            (first, tomorrow_start)
        }
        RangePreset::All => {
            let epoch = Local.with_ymd_and_hms(2000, 1, 1, 0, 0, 0)
                .single().unwrap_or(today_start);
            (epoch, tomorrow_start)
        }
    };
    let label = match preset {
        RangePreset::All => "全部".to_string(),
        _ => format!("{} ~ {}",
            start.format("%Y-%m-%d"),
            (end - ChronoDuration::milliseconds(1)).format("%Y-%m-%d")),
    };
    DateRange { since: start.to_rfc3339(), until: end.to_rfc3339(), label }
}

pub struct GetCommitsOpts<'a> {
    pub authors: &'a [String],
    pub since: &'a str,
    pub until: &'a str,
    pub match_mode: MatchDimension,
    pub include_body: bool,
    pub include_stat: bool,
}

pub async fn get_local_commits(repo: &str, opts: GetCommitsOpts<'_>) -> Result<Vec<LocalCommit>, String> {
    let mut fields = vec!["%H", "%an", "%ae", "%aI", "%cn", "%ce", "%cI", "%s"];
    if opts.include_body { fields.push("%b"); }
    let format = format!("{}{}", fields.join(&FIELD_SEP.to_string()), RECORD_SEP);
    let mut args: Vec<String> = vec![
        "log".into(), "--all".into(), "--no-merges".into(),
        format!("--since={}", opts.since),
        format!("--until={}", opts.until),
        format!("--pretty=format:{}", format),
    ];
    let author_args: Vec<String> = match opts.match_mode {
        MatchDimension::Author => opts.authors.iter().filter(|s| !s.is_empty()).map(|s| format!("--author={}", s)).collect(),
        MatchDimension::Committer => opts.authors.iter().filter(|s| !s.is_empty()).map(|s| format!("--committer={}", s)).collect(),
        MatchDimension::Any => Vec::new(),
    };
    args.extend(author_args);
    let str_args: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let out = run_git_cmd(&str_args, Some(Path::new(repo))).await?;
    if out.is_empty() { return Ok(Vec::new()); }

    let mut commits: Vec<LocalCommit> = out.split(RECORD_SEP)
        .map(|line| line.trim_start_matches('\n').to_string())
        .filter(|s| !s.is_empty())
        .filter_map(|line| {
            let parts: Vec<&str> = line.split(FIELD_SEP).collect();
            if parts.len() < 8 { return None; }
            let sha = parts[0].trim().to_string();
            let short_sha = if sha.len() > 7 { sha[..7].to_string() } else { sha.clone() };
            let body = if opts.include_body {
                let body_str = parts.iter().skip(8).cloned().collect::<Vec<&str>>().join(&FIELD_SEP.to_string()).trim().to_string();
                if body_str.is_empty() { None } else { Some(body_str) }
            } else { None };
            Some(LocalCommit {
                sha, short_sha,
                author_name: parts[1].to_string(), author_email: parts[2].to_string(),
                authored_date: parts[3].to_string(), committer_name: parts[4].to_string(),
                committer_email: parts[5].to_string(), committed_date: parts[6].to_string(),
                title: parts[7].to_string(), body, stat: None,
            })
        }).collect();

    if matches!(opts.match_mode, MatchDimension::Any) && !opts.authors.is_empty() {
        let patterns: Vec<String> = opts.authors.iter().map(|s| s.to_lowercase()).collect();
        commits.retain(|c| {
            let fields = [&c.author_email, &c.author_name, &c.committer_email, &c.committer_name];
            patterns.iter().any(|p| fields.iter().any(|f| f.to_lowercase().contains(p)))
        });
    }

    if opts.include_stat && !commits.is_empty() {
        let sha_list: Vec<String> = commits.iter().map(|c| c.sha.clone()).collect();
        let stats = run_concurrent_stats(repo, &sha_list, 4).await;
        for (c, s) in commits.iter_mut().zip(stats) { c.stat = s; }
    }
    Ok(commits)
}

async fn run_concurrent_stats(repo: &str, shas: &[String], limit: usize) -> Vec<Option<CommitStat>> {
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
    for h in handles { out.push(h.await.ok().flatten()); }
    out
}

pub async fn get_commit_stat(repo: &str, sha: &str) -> Result<CommitStat, String> {
    let out = run_git_cmd(&["show", "--numstat", "--format=", sha], Some(Path::new(repo))).await?;
    let mut files: Vec<CommitStatFile> = Vec::new();
    let mut insertions = 0usize;
    let mut deletions = 0usize;
    for line in out.split('\n') {
        let trimmed = line.trim();
        if trimmed.is_empty() { continue; }
        let parts: Vec<&str> = trimmed.splitn(3, '\t').collect();
        if parts.len() < 3 { continue; }
        let path = parts[2].to_string();
        if path.is_empty() { continue; }
        let binary = parts[0] == "-" && parts[1] == "-";
        let ins = if binary { 0 } else { parts[0].parse().unwrap_or(0) };
        let del = if binary { 0 } else { parts[1].parse().unwrap_or(0) };
        files.push(CommitStatFile { path, insertions: ins, deletions: del, binary: if binary { Some(true) } else { None } });
        insertions += ins; deletions += del;
    }
    Ok(CommitStat { files_changed: files.len(), insertions, deletions, files })
}

pub async fn get_commit_diff(repo: &str, sha: &str) -> Result<CommitDiff, String> {
    let meta_format = ["%H", "%an", "%ae", "%aI", "%s", "%b"].join(&FIELD_SEP.to_string());
    let meta_out = run_git_cmd(&["show", "-s", &format!("--format={}", meta_format), sha], Some(Path::new(repo))).await?;
    let meta = meta_out.trim_end_matches('\n');
    let parts: Vec<&str> = meta.split(FIELD_SEP).collect();
    if parts.len() < 5 {
        return Err(format!("git show meta 输出格式异常: {}", crate::util::truncate_chars(meta, 200)));
    }
    let sha_out = parts[0].trim().to_string();
    let body = parts.iter().skip(5).cloned().collect::<Vec<&str>>().join(&FIELD_SEP.to_string()).trim().to_string();
    let stat = get_commit_stat(repo, sha).await?;
    let patch_raw = run_git_cmd(&["show", "--format=", sha], Some(Path::new(repo))).await?;
    Ok(CommitDiff {
        sha: if sha_out.is_empty() { sha.to_string() } else { sha_out },
        author_name: parts[1].to_string(), author_email: parts[2].to_string(),
        authored_date: parts[3].to_string(), title: parts[4].to_string(),
        body, stat, patch: patch_raw.trim_start_matches('\n').to_string(),
    })
}

#[derive(Debug, Clone, serde::Serialize)]
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
    ensure_git_available().await.map_err(|e| format!("未找到 git，请确认 git 已安装并在 PATH 中。原始错误: {}", e))?;

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
            range: preset_range, authors, root_dirs: input.root_dirs.to_vec(),
            repos: Vec::new(), total_commits: 0, scanned_repos: 0, repos_with_commits: 0,
        });
    }

    let authors_arc = Arc::new(authors.clone());
    let since_arc = Arc::new(since.clone());
    let until_arc = Arc::new(until.clone());
    let sem = Arc::new(Semaphore::new(8));
    let match_mode = input.match_mode;
    let include_body = input.include_body;
    let include_stat = input.include_stat;

    let mut handles = Vec::with_capacity(repos.len());
    for repo in &repos {
        let repo = repo.clone();
        let authors = authors_arc.clone();
        let since = since_arc.clone();
        let until = until_arc.clone();
        let permit = sem.clone();
        handles.push(tokio::spawn(async move {
            let _g = permit.acquire_owned().await.ok();
            let commits = get_local_commits(&repo, GetCommitsOpts {
                authors: &authors, since: &since, until: &until,
                match_mode, include_body, include_stat,
            }).await.unwrap_or_default();
            RepoCommits { repo_path: repo, commits }
        }));
    }

    let mut repo_results: Vec<RepoCommits> = Vec::with_capacity(handles.len());
    for h in handles {
        if let Ok(r) = h.await { if !r.commits.is_empty() { repo_results.push(r); } }
    }
    for r in &mut repo_results { r.commits.sort_by(|a, b| b.authored_date.cmp(&a.authored_date)); }
    repo_results.sort_by(|a, b| {
        let av = a.commits.first().map(|c| c.authored_date.clone()).unwrap_or_default();
        let bv = b.commits.first().map(|c| c.authored_date.clone()).unwrap_or_default();
        bv.cmp(&av)
    });

    let total_commits = repo_results.iter().map(|r| r.commits.len()).sum();
    let repos_with_commits = repo_results.len();
    Ok(ListMyLocalCommitsResult {
        range: DateRange { since, until, label: preset_range.label },
        authors, root_dirs: input.root_dirs.to_vec(),
        repos: repo_results, total_commits, scanned_repos, repos_with_commits,
    })
}
