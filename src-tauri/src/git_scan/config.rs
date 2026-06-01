use std::collections::{HashMap, HashSet};

/// 业务线别名表：业务线名 → 补充关键词列表。
pub fn load_business_aliases() -> HashMap<String, Vec<String>> {
    let path = crate::settings::jarvis_dir().join("business-aliases.json");
    if !path.exists() {
        let _ = std::fs::create_dir_all(crate::settings::jarvis_dir());
        let default = serde_json::json!({ "示例业务线": ["门禁", "计量"] });
        let _ = crate::util::write_atomic(&path, &serde_json::to_string_pretty(&default).unwrap_or_default());
    }
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c, Err(_) => return HashMap::new(),
    };
    let parsed: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v, Err(_) => return HashMap::new(),
    };
    let mut out = HashMap::new();
    if let Some(obj) = parsed.as_object() {
        for (k, v) in obj {
            if let Some(arr) = v.as_array() {
                let kws: Vec<String> = arr.iter()
                    .filter_map(|x| x.as_str())
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty()).collect();
                if !kws.is_empty() { out.insert(k.clone(), kws); }
            }
        }
    }
    out
}

/// 排除的业务线集合（仓库的"业务线名"在这里就完全不计工作量）。
pub fn load_excluded_business_lines() -> HashSet<String> {
    let path = crate::settings::jarvis_dir().join("excluded-business-lines.json");
    if !path.exists() {
        let _ = std::fs::create_dir_all(crate::settings::jarvis_dir());
        let default = serde_json::json!(["my-mcp-servers"]);
        let _ = crate::util::write_atomic(&path, &serde_json::to_string_pretty(&default).unwrap_or_default());
    }
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c, Err(_) => return HashSet::new(),
    };
    let parsed: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v, Err(_) => return HashSet::new(),
    };
    let mut out = HashSet::new();
    if let Some(arr) = parsed.as_array() {
        for x in arr {
            if let Some(s) = x.as_str() {
                let s = s.trim();
                if !s.is_empty() { out.insert(s.to_string()); }
            }
        }
    }
    out
}

/// 从 settings + env 取 repoRoots（对齐 TS getRepoRoots）。
pub fn get_repo_roots() -> Vec<String> {
    let cfg = crate::settings::load_raw_config();
    let from_cfg: Vec<String> = cfg.as_ref()
        .and_then(|v| v.get("repoRoots")).and_then(|v| v.as_array())
        .map(|arr| arr.iter()
            .filter_map(|x| x.as_str()).map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()).collect())
        .unwrap_or_default();
    if !from_cfg.is_empty() { return from_cfg; }
    if let Ok(raw) = std::env::var("TENCENT_CODE_LOCAL_ROOTS") {
        return raw.split(|c| c == ';' || c == ',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
    }
    Vec::new()
}
