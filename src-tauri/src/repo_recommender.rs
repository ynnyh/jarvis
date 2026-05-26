// LLM 服务 A：给定一个任务，从用户配置的 repoRoots 中挑出最可能的归属项目。
//
// 使用场景：
// - 新任务刚出现，绑定小窗需要"AI 推荐"按相关度排序的 repo 列表
// - 用户手动点任务卡上的"未绑定"图标，触发绑定窗时也走同一接口
//
// 输入：任务标题/描述/截止 + repoRoots 数组
// 输出：按相关度排序的 RepoRecommendation 列表，top1 标 is_top=true
//
// 性能注记：repoRoots 通常只有 1-3 个，profile 构建是磁盘 IO + git log，
// 没必要为这点 IO 引入并行调度的复杂度。LLM 调用占总耗时 90%+，那才是瓶颈。
//
// 降级路径：
// - repoRoots 只有 1 个 → 跳过 LLM 直接返回（省钱省时间）
// - LLM 返回内容解析不出 JSON → 按 repoRoots 原顺序返回 50 分，前端仍能渲染

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use serde::Serialize;
use tokio::process::Command;

#[derive(Debug, Clone, Serialize)]
pub struct RepoRecommendation {
    #[serde(rename = "repoRoot")]
    pub repo_root: String,
    pub score: u32,
    pub reason: String,
    #[serde(rename = "isTop")]
    pub is_top: bool,
}

struct RepoProfile {
    path: String,
    name: String,
    description: String,
    recent_commits: Vec<String>,
}

async fn run_git(args: &[&str], cwd: &Path) -> Result<String, String> {
    let mut cmd = Command::new("git");
    cmd.args(args).current_dir(cwd).stdin(Stdio::null());
    #[cfg(windows)]
    {
        // tokio::process::Command 在 Windows 上自带 creation_flags 方法
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    let fut = cmd.output();
    let output = tokio::time::timeout(Duration::from_secs(5), fut)
        .await
        .map_err(|_| format!("git {} 超时", args.join(" ")))?
        .map_err(|e| format!("git {} 启动失败: {}", args.join(" "), e))?;
    if !output.status.success() {
        return Err(format!(
            "git {} 失败: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn read_package_json(path: &Path) -> (Option<String>, Option<String>) {
    let raw = match std::fs::read_to_string(path.join("package.json")) {
        Ok(s) => s,
        Err(_) => return (None, None),
    };
    let v: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(_) => return (None, None),
    };
    let name = v.get("name").and_then(|x| x.as_str()).map(String::from);
    let desc = v
        .get("description")
        .and_then(|x| x.as_str())
        .map(String::from);
    (name, desc)
}

/// 读 README 的第一个非空、非标题段落。
/// 标题段落（# 开头）跳过 —— 通常只是项目名重复。
fn read_readme(path: &Path) -> String {
    for fname in ["README.md", "readme.md", "Readme.md", "README", "readme"] {
        let Ok(s) = std::fs::read_to_string(path.join(fname)) else {
            continue;
        };
        for para in s.split("\n\n") {
            let cleaned = para.trim();
            if !cleaned.is_empty() && !cleaned.starts_with('#') {
                return cleaned.chars().take(400).collect();
            }
        }
        return s.chars().take(400).collect();
    }
    String::new()
}

async fn build_profile(repo_root: &str) -> RepoProfile {
    let path = Path::new(repo_root);
    let folder_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(repo_root)
        .to_string();

    let (pkg_name, pkg_desc) = read_package_json(path);
    let readme = read_readme(path);

    let recent_commits = run_git(&["log", "-5", "--format=%s"], path)
        .await
        .ok()
        .map(|s| {
            s.lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect()
        })
        .unwrap_or_default();

    RepoProfile {
        path: repo_root.to_string(),
        name: pkg_name.unwrap_or(folder_name),
        description: pkg_desc.unwrap_or(readme),
        recent_commits,
    }
}

fn build_prompt(
    task_title: &str,
    task_description: &str,
    deadline: &str,
    profiles: &[RepoProfile],
) -> String {
    let mut s = String::new();
    s.push_str("你是 Jarvis，用户的个人任务助手。用户接到一个新任务，请帮他/她判断该任务最可能归属到哪个本地代码项目（repo）。\n\n");
    s.push_str("任务信息：\n");
    s.push_str(&format!("- 标题：{}\n", task_title));
    if !task_description.is_empty() {
        let desc: String = task_description.chars().take(300).collect();
        s.push_str(&format!("- 描述：{}\n", desc));
    }
    if !deadline.is_empty() {
        s.push_str(&format!("- 截止：{}\n", deadline));
    }
    s.push_str("\n候选项目：\n");
    for (i, p) in profiles.iter().enumerate() {
        s.push_str(&format!("[{}] 路径：{}\n", i + 1, p.path));
        s.push_str(&format!("    名称：{}\n", p.name));
        if !p.description.is_empty() {
            let desc: String = p.description.chars().take(200).collect();
            s.push_str(&format!("    简介：{}\n", desc));
        }
        if !p.recent_commits.is_empty() {
            s.push_str("    最近 commit:\n");
            for c in &p.recent_commits {
                let line: String = c.chars().take(80).collect();
                s.push_str(&format!("      - {}\n", line));
            }
        }
        s.push('\n');
    }
    s.push_str("请返回 JSON 数组，按相关度从高到低排序。每个元素：\n");
    s.push_str("  - index: 1-based 候选项目序号\n");
    s.push_str("  - score: 0-100 整数，关联度评分\n");
    s.push_str("  - reason: 一句话理由（<=25 字）\n");
    s.push_str("所有候选项目都要给分。只输出 JSON 数组本体，不要任何前后说明文字。\n");
    s.push_str("示例：[{\"index\":1,\"score\":85,\"reason\":\"任务关键词出现在最近 commit\"}]\n");
    s
}

/// 从 LLM 返回的文本里抠出 JSON 数组并解析成 RepoRecommendation 列表。
/// 失败时返回空 Vec，调用方做降级。
fn parse_recommendations(text: &str, profiles: &[RepoProfile]) -> Vec<RepoRecommendation> {
    let trimmed = text.trim();
    // 容错：模型可能加前缀"以下是评分结果："之类的话
    let (start, end) = (trimmed.find('['), trimmed.rfind(']'));
    let json_str = match (start, end) {
        (Some(s), Some(e)) if e > s => &trimmed[s..=e],
        _ => return vec![],
    };
    let arr: Vec<serde_json::Value> = match serde_json::from_str(json_str) {
        Ok(v) => v,
        Err(_) => return vec![],
    };

    let mut result = Vec::new();
    for item in arr {
        let index = item.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        if index == 0 || index > profiles.len() {
            continue;
        }
        let score = item.get("score").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let reason = item
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        result.push(RepoRecommendation {
            repo_root: profiles[index - 1].path.clone(),
            score: score.min(100),
            reason,
            is_top: false,
        });
    }
    // 防御性二次排序：LLM 没排好我们自己排
    result.sort_by(|a, b| b.score.cmp(&a.score));
    if let Some(first) = result.first_mut() {
        first.is_top = true;
    }
    result
}

#[tauri::command]
pub async fn recommend_repos_for_task(
    task_title: String,
    task_description: String,
    deadline: String,
    repo_roots: Vec<String>,
) -> Result<Vec<RepoRecommendation>, String> {
    if repo_roots.is_empty() {
        return Ok(vec![]);
    }

    // 唯一候选直接置顶，跳过 LLM
    if repo_roots.len() == 1 {
        return Ok(vec![RepoRecommendation {
            repo_root: repo_roots[0].clone(),
            score: 100,
            reason: "唯一候选项目".to_string(),
            is_top: true,
        }]);
    }

    let mut profiles = Vec::with_capacity(repo_roots.len());
    for r in &repo_roots {
        profiles.push(build_profile(r).await);
    }

    let prompt = build_prompt(&task_title, &task_description, &deadline, &profiles);

    let req = crate::llm::ChatRequest {
        messages: vec![crate::llm::ChatMessage {
            role: crate::llm::Role::User,
            content: prompt,
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }],
        temperature: Some(0.2),
        max_tokens: Some(512),
        model: None,
        timeout_ms: Some(30_000),
        tools: None,
        tool_choice: None,
    };
    let resp = match crate::llm::chat(req).await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[recommend_repos_for_task] LLM 调用失败: {}", e);
            return Ok(fallback_ranking(&profiles));
        }
    };

    let recs = parse_recommendations(&resp.text, &profiles);
    if recs.is_empty() {
        eprintln!(
            "[recommend_repos_for_task] LLM 返回内容无法解析 JSON, raw={}",
            resp.text.chars().take(200).collect::<String>()
        );
        return Ok(fallback_ranking(&profiles));
    }
    Ok(recs)
}

/// LLM 不可用 / 解析失败时的兜底排序：按配置顺序，全部 50 分，第一个标 top。
/// 前端仍能让用户手选 —— 比"功能不可用"好得多。
fn fallback_ranking(profiles: &[RepoProfile]) -> Vec<RepoRecommendation> {
    profiles
        .iter()
        .enumerate()
        .map(|(i, p)| RepoRecommendation {
            repo_root: p.path.clone(),
            score: 50,
            reason: "（AI 不可用，按配置顺序展示）".to_string(),
            is_top: i == 0,
        })
        .collect()
}
