// 任务 snapshot：跟踪上次拉到的"我的任务"集合，下次拉时 diff 出新任务。
//
// 文件：~/.jarvis/task-snapshot.json，内容是任务 id 的字符串数组。
//
// 设计要点：
// - 首次运行（文件不存在）静默落盘，返回空 diff。避免老用户首次升到 v0.6.0
//   后被几十张绑定卡片轰炸 —— 存量任务走"任务卡上的未绑定图标"懒触发路径。
// - 只关心 id，不关心 title/priority。title 可能在禅道被改名，但 id 不变。
// - 单元素 hash set 比逐字符串遍历快，但本场景任务量百级别，纯遍历也够，
//   挑 HashSet 主要是代码更清晰。

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

fn jarvis_dir() -> PathBuf {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_default();
    PathBuf::from(home).join(".jarvis")
}

fn snapshot_path() -> PathBuf {
    jarvis_dir().join("task-snapshot.json")
}

/// 事件 payload，前端绑定窗会用 title/priority/deadline 渲染任务卡。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRef {
    pub id: String,
    pub title: String,
    pub priority: String,
    pub deadline: String,
}

fn read_snapshot() -> Option<HashSet<String>> {
    let path = snapshot_path();
    if !path.exists() {
        return None;
    }
    let raw = fs::read_to_string(&path).ok()?;
    let arr: Vec<String> = serde_json::from_str(&raw).ok()?;
    Some(arr.into_iter().collect())
}

fn write_snapshot(ids: &HashSet<String>) {
    let dir = jarvis_dir();
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    let arr: Vec<&String> = ids.iter().collect();
    if let Ok(content) = serde_json::to_string_pretty(&arr) {
        let _ = fs::write(snapshot_path(), content);
    }
}

/// 把当前任务集合刷到 snapshot 文件，并返回新增的任务。
/// 首次运行返回空数组（避免初装提醒爆炸）。
pub fn diff_and_persist(tasks: &[TaskRef]) -> Vec<TaskRef> {
    let current_ids: HashSet<String> = tasks.iter().map(|t| t.id.clone()).collect();
    let prev = read_snapshot();
    write_snapshot(&current_ids);
    match prev {
        None => Vec::new(),
        Some(prev) => tasks
            .iter()
            .filter(|t| !prev.contains(&t.id))
            .cloned()
            .collect(),
    }
}
