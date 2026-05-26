// 任务↔本地项目绑定表。
//
// 数据源：~/.jarvis/task-bindings.json
//
// 结构：
//   {
//     "<taskId>": {
//       "repoRoots": ["D:/work/proj-a", "D:/work/proj-b"],
//       "boundAt": "2026-05-26T10:00:00+08:00",
//       "lastConfirmedBy": "llm-1click" | "manual" | "manual-multi" | "skipped"
//     },
//     ...
//   }
//
// 设计要点：
// - `repoRoots` 从第一天就是数组，绝大多数任务只有一个元素（1:1），但极个别跨仓任务
//   要支持 1:N。未来扩展时不动 schema、不迁数据。
// - `lastConfirmedBy` 用来分析"AI 推荐 1 click 命中率"，后续优化 prompt 时有据可依。
// - `skipped` 表示用户在绑定卡片点了"暂不绑定"，下次仍会作为未绑定任务参与 commit
//   匹配兜底，但不会再主动弹卡片烦用户（直到用户点任务卡上的图标手动触发）。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

fn jarvis_dir() -> PathBuf {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_default();
    PathBuf::from(home).join(".jarvis")
}

fn bindings_path() -> PathBuf {
    jarvis_dir().join("task-bindings.json")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskBinding {
    #[serde(rename = "repoRoots")]
    pub repo_roots: Vec<String>,
    #[serde(rename = "boundAt")]
    pub bound_at: String,
    #[serde(rename = "lastConfirmedBy")]
    pub last_confirmed_by: String,
}

pub type BindingMap = HashMap<String, TaskBinding>;

fn read_all() -> BindingMap {
    let path = bindings_path();
    if !path.exists() {
        return HashMap::new();
    }
    let Ok(raw) = fs::read_to_string(&path) else {
        return HashMap::new();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

fn write_all(map: &BindingMap) -> Result<(), String> {
    let dir = jarvis_dir();
    fs::create_dir_all(&dir).map_err(|e| format!("创建配置目录失败: {}", e))?;
    let content = serde_json::to_string_pretty(map)
        .map_err(|e| format!("绑定表序列化失败: {}", e))?;
    fs::write(bindings_path(), content).map_err(|e| format!("写入绑定表失败: {}", e))?;
    Ok(())
}

#[tauri::command]
pub fn task_bindings_load() -> Result<BindingMap, String> {
    Ok(read_all())
}

#[tauri::command]
pub fn task_bindings_get(task_id: String) -> Result<Option<TaskBinding>, String> {
    Ok(read_all().get(&task_id).cloned())
}

/// 创建或覆盖绑定。`repo_roots` 若为空数组，等价于 task_bindings_delete。
#[tauri::command]
pub fn task_bindings_set(
    task_id: String,
    repo_roots: Vec<String>,
    last_confirmed_by: String,
) -> Result<(), String> {
    let mut map = read_all();
    if repo_roots.is_empty() {
        map.remove(&task_id);
    } else {
        map.insert(
            task_id,
            TaskBinding {
                repo_roots,
                bound_at: chrono::Local::now().to_rfc3339(),
                last_confirmed_by,
            },
        );
    }
    write_all(&map)
}

#[tauri::command]
pub fn task_bindings_delete(task_id: String) -> Result<(), String> {
    let mut map = read_all();
    map.remove(&task_id);
    write_all(&map)
}

/// 反向查询：给定 repo 路径，返回所有绑定到它的 task_id 列表。
/// commit_link 改造时会用：扫到一条 commit → 拿 repo path 反查候选任务集 →
/// 集合 ≤1 直接归属，>1 走 LLM 判定。
#[allow(dead_code)]
pub fn task_ids_for_repo(repo_root: &str) -> Vec<String> {
    let target = normalize_path(repo_root);
    read_all()
        .into_iter()
        .filter_map(|(tid, b)| {
            let hit = b
                .repo_roots
                .iter()
                .any(|r| normalize_path(r) == target);
            hit.then_some(tid)
        })
        .collect()
}

/// 大小写不敏感（Windows 文件系统）+ 统一斜杠 + 去尾部分隔符。
/// 不解析符号链接 —— 用户配的就是字面路径，我们尊重输入。
#[allow(dead_code)]
fn normalize_path(p: &str) -> String {
    let s = p.replace('\\', "/").trim_end_matches('/').to_string();
    if cfg!(windows) {
        s.to_lowercase()
    } else {
        s
    }
}
