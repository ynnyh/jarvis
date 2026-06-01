// 本地 git 仓库扫描 + commit 查询。
//
// 移植自 src/services/local-git/{scan,index}.ts。设计原则同 TS 版：
// - 只调外部 git 进程（spawn），不引第三方 git 库
// - 路径用 PathBuf；日期用 RFC3339 字符串（与 git --pretty=%aI/%cI 输出一致）
// - 字段命名跟 TS 一一对齐，前端契约不变
//
// 调用入口：list_my_local_commits（高级 API）和 get_local_commit_diff（diff 接口）。

#![allow(dead_code)]

pub mod types;
pub mod discovery;
pub mod commits;
pub mod config;
pub mod cleanup;

// 公开 API 重新导出（外部模块通过 crate::git_scan::xxx 访问）
pub use types::{LocalCommit, DateRange, MatchDimension, RangePreset};
pub use discovery::find_git_repos;
pub use commits::{list_my_local_commits, ListMyLocalCommitsInput};
pub use config::{load_excluded_business_lines, get_repo_roots};
pub use cleanup::{clean_commit_title, effort_for_commit};
