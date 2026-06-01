use serde::{Deserialize, Serialize};

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
