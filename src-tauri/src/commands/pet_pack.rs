//! 宠物包 v2 管理：多动作素材包（pack.json + 素材原样落盘）
//!
//! 与 custom_pet（单文件 base64 JSON）不同，宠物包是「目录包」：
//! ~/.jarvis/pets/<packId>/pack.json + 若干素材文件（gif/webm/spritesheet png），
//! 支持一个宠物挂 idle/walk/held/sleep + 任意多个小动作片段。
//! 前端按 clip 惰性经 pet_pack_read_file 换 data URL 渲染。
//!
//! 安全红线：
//! - packId / 素材文件名一律先 sanitize（剥路径成分），落盘路径再经 canonicalize
//!   校验仍在包目录内，杜绝 `../` 穿越到任意目录读写；
//! - 大小上限：单文件 20MB / 总量 60MB，在 base64 解码【前】按长度估算拦截，
//!   防止超大 payload 先把内存吃爆。

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::Emitter;

/// 单素材文件解码后上限 20MB
const MAX_SINGLE_FILE_BYTES: usize = 20 * 1024 * 1024;
/// 整包素材解码后总量上限 60MB
const MAX_TOTAL_BYTES: usize = 60 * 1024 * 1024;

/// 合法 clip 类型白名单（与前端 PetPackClipType 对齐）
const VALID_CLIP_TYPES: [&str; 3] = ["gif", "webm", "spritesheet"];

/// 前端上传的素材文件：原文件名 + base64 内容
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PetPackFileIn {
    pub name: String,
    pub data_base64: String,
}

/// pet_pack_list 返回的包元数据（不含素材内容）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PetPack {
    pub id: String,
    pub name: String,
    /// 渲染整体缩放倍率，缺省 1.0
    #[serde(default = "default_scale")]
    pub scale: f64,
    /// 素材默认朝向 "left" | "right"（walk 反向行走时前端据此翻转）
    #[serde(default = "default_facing")]
    pub facing: String,
    /// idle/walk/held/sleep + actions 片段清单，结构由前端 PetPackClips 约定
    pub clips: serde_json::Value,
}

fn default_scale() -> f64 {
    1.0
}

fn default_facing() -> String {
    "left".to_string()
}

/// 宠物包存储根目录 ~/.jarvis/pets/
fn pet_packs_dir() -> PathBuf {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_default();
    PathBuf::from(home).join(".jarvis").join("pets")
}

/// packId 白名单过滤（同 settings_open 的 safe_page 先例）：
/// 仅保留 ascii 字母/数字/-/_，把路径分隔符与中文等一律剥掉。
/// 剥完为空即拒绝 —— "../x" 会变 "x"、".." 变空串，天然防穿越。
fn sanitize_pack_id(id: &str) -> String {
    id.chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect()
}

/// 素材文件名净化：只取路径最后一段 file_name，再剥控制字符与分隔符。
/// ".." 经 Path::file_name() 得 None → 空串 → 拒绝。
fn sanitize_file_name(name: &str) -> Result<String, String> {
    let base = std::path::Path::new(name)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    let cleaned: String = base
        .chars()
        .filter(|c| !c.is_control() && *c != '/' && *c != '\\')
        .collect();
    if cleaned.is_empty() || cleaned == "." || cleaned == ".." {
        return Err(format!("非法的素材文件名: {}", name));
    }
    Ok(cleaned)
}

/// 解码前按 base64 长度估算解码后大小并拦截（≈ len/4*3，上界估算，
/// padding 情况只会更小），防止超大 payload 先解码再撑爆内存。
fn decode_pack_file(name: &str, data_base64: &str) -> Result<(String, Vec<u8>), String> {
    let safe_name = sanitize_file_name(name)?;
    let estimated = data_base64.len() / 4 * 3;
    if estimated > MAX_SINGLE_FILE_BYTES {
        return Err(format!(
            "文件 {} 超过单文件 20MB 上限（按 base64 长度估算）",
            safe_name
        ));
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data_base64.trim())
        .map_err(|e| format!("文件 {} base64 解码失败: {}", safe_name, e))?;
    if bytes.len() > MAX_SINGLE_FILE_BYTES {
        return Err(format!("文件 {} 超过单文件 20MB 上限", safe_name));
    }
    Ok((safe_name, bytes))
}

/// 校验单个 clip 条目：file 非空且必须在上传清单内、type 在白名单内、
/// spritesheet 必须带全 5 个正整数参数。
fn validate_single_clip(label: &str, clip: &serde_json::Value, uploaded: &[String]) -> Result<(), String> {
    let obj = clip
        .as_object()
        .ok_or_else(|| format!("{} 必须是对象", label))?;
    let file = obj
        .get("file")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .unwrap_or("");
    if file.is_empty() {
        return Err(format!("{} 缺少 file 字段", label));
    }
    if !uploaded.iter().any(|u| u == file) {
        return Err(format!("{} 引用的文件「{}」不在上传清单中", label, file));
    }
    let clip_type = obj
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if !VALID_CLIP_TYPES.contains(&clip_type) {
        return Err(format!(
            "{} 的 type 非法: 「{}」（仅支持 gif/webm/spritesheet）",
            label, clip_type
        ));
    }
    if clip_type == "spritesheet" {
        for field in ["frameW", "frameH", "fps", "frames", "columns"] {
            let v = obj.get(field).and_then(serde_json::Value::as_f64).unwrap_or(0.0);
            if v <= 0.0 || v.fract() != 0.0 {
                return Err(format!(
                    "{}（spritesheet）缺少或非法字段 {}（须为正整数）",
                    label, field
                ));
            }
        }
    }
    Ok(())
}

/// 校验 clips 结构：idle 必填；walk/held/sleep 可选；actions 数组每项
/// 需要非空 name + 合法 clip 字段 + 可选正数 weight。
fn validate_clips(clips: &serde_json::Value, uploaded: &[String]) -> Result<(), String> {
    let obj = clips
        .as_object()
        .ok_or_else(|| "clips 必须是对象".to_string())?;
    let idle = obj
        .get("idle")
        .ok_or_else(|| "缺少 idle 片段（宠物包必须有 idle）".to_string())?;
    validate_single_clip("idle", idle, uploaded)?;
    for key in ["walk", "held", "sleep"] {
        if let Some(clip) = obj.get(key) {
            validate_single_clip(key, clip, uploaded)?;
        }
    }
    if let Some(actions) = obj.get("actions") {
        let arr = actions
            .as_array()
            .ok_or_else(|| "actions 必须是数组".to_string())?;
        for (i, action) in arr.iter().enumerate() {
            let label = format!("actions[{}]", i);
            validate_single_clip(&label, action, uploaded)?;
            let name = action
                .get("name")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .unwrap_or("");
            if name.is_empty() {
                return Err(format!("{} 缺少动作名 name（点播菜单要用）", label));
            }
            if let Some(w) = action.get("weight") {
                let wf = w.as_f64().unwrap_or(0.0);
                if wf <= 0.0 {
                    return Err(format!("{} 的 weight 必须是正数（缺省 1）", label));
                }
            }
        }
    }
    Ok(())
}

/// 按包目录读 pack.json 的通用入口（list 用）
fn read_pack_json(dir: &std::path::Path) -> Option<PetPack> {
    let path = dir.join("pack.json");
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

/// 列出所有宠物包（只返回元数据，素材由前端按 clip 惰性读）
#[tauri::command]
pub fn pet_pack_list() -> Result<Vec<PetPack>, String> {
    let dir = pet_packs_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let entries =
        std::fs::read_dir(&dir).map_err(|e| format!("读取宠物包目录失败: {}", e))?;
    let mut packs = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if let Some(pack) = read_pack_json(&path) {
            packs.push(pack);
        }
    }
    packs.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(packs)
}

/// 保存宠物包（整包覆盖写）：先全部校验 + 解码，再清目录重写，
/// 中途任何一步失败都不会留下半包。
#[tauri::command]
pub fn pet_pack_save(
    id: String,
    name: String,
    facing: String,
    clips: serde_json::Value,
    files: Vec<PetPackFileIn>,
    // 整体缩放倍率；缺省 1.0（老前端不传该参数也能存）
    scale: Option<f64>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let safe_id = sanitize_pack_id(&id);
    if safe_id.is_empty() {
        return Err("宠物包 ID 含非法字符或为空".to_string());
    }
    // 朝向只认 left/right，脏值兜底 left（素材默认朝向，错了只是翻转方向不合预期，不值得整包拒绝）
    let facing = if facing == "right" { "right" } else { "left" }.to_string();
    let name = name.trim();
    let name = if name.is_empty() { &safe_id } else { name };

    // 1) 解码全部素材（大小上限在此拦截）
    let mut decoded: Vec<(String, Vec<u8>)> = Vec::with_capacity(files.len());
    let mut total: usize = 0;
    let mut seen: Vec<String> = Vec::with_capacity(files.len());
    for f in &files {
        let (safe_name, bytes) = decode_pack_file(&f.name, &f.data_base64)?;
        if seen.contains(&safe_name) {
            return Err(format!("素材文件重名: {}", safe_name));
        }
        total += bytes.len();
        if total > MAX_TOTAL_BYTES {
            return Err("宠物包素材总量超过 60MB 上限".to_string());
        }
        seen.push(safe_name.clone());
        decoded.push((safe_name, bytes));
    }
    if decoded.is_empty() {
        return Err("没有可写入的素材文件".to_string());
    }

    // 2) 校验 clips 结构与文件引用
    validate_clips(&clips, &seen)?;

    // 3) 落盘：清掉旧目录整包重写（导入界面永远整包上传，覆盖语义最干净）
    let dir = pet_packs_dir().join(&safe_id);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|e| format!("清理旧宠物包失败: {}", e))?;
    }
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建宠物包目录失败: {}", e))?;
    for (file_name, bytes) in &decoded {
        std::fs::write(dir.join(file_name), bytes)
            .map_err(|e| format!("写入素材 {} 失败: {}", file_name, e))?;
    }
    let pack_json = serde_json::json!({
        "id": safe_id,
        "name": name,
        "scale": scale.unwrap_or(1.0).clamp(0.1, 10.0),
        "facing": facing,
        "clips": clips,
    });
    crate::util::write_atomic(
        &dir.join("pack.json"),
        &serde_json::to_string_pretty(&pack_json)
            .map_err(|e| format!("序列化 pack.json 失败: {}", e))?,
    )
    .map_err(|e| format!("写入 pack.json 失败: {}", e))?;

    // 广播各窗口刷新宠物包缓存（同 custom-pets-changed 先例）
    let _ = app.emit("pet-packs-changed", ());
    Ok(())
}

/// 删除宠物包（整目录删除）
#[tauri::command]
pub fn pet_pack_delete(id: String, app: tauri::AppHandle) -> Result<(), String> {
    let safe_id = sanitize_pack_id(&id);
    if safe_id.is_empty() {
        return Err("宠物包 ID 含非法字符或为空".to_string());
    }
    let dir = pet_packs_dir().join(&safe_id);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|e| format!("删除宠物包失败: {}", e))?;
    }
    let _ = app.emit("pet-packs-changed", ());
    Ok(())
}

/// 按扩展名读取包内素材，返回带正确 mime 的 data URL，前端直接塞 img/video src。
#[tauri::command]
pub fn pet_pack_read_file(pack_id: String, file: String) -> Result<String, String> {
    let safe_id = sanitize_pack_id(&pack_id);
    if safe_id.is_empty() {
        return Err("宠物包 ID 含非法字符或为空".to_string());
    }
    let dir = pet_packs_dir().join(&safe_id);
    if !dir.exists() {
        return Err(format!("宠物包「{}」不存在", safe_id));
    }
    let safe_name = sanitize_file_name(&file)?;
    let path = dir.join(&safe_name);
    // 防穿越双保险：文件名已剥路径成分，join 后仍 canonicalize 校验终路径在包目录内
    // （Windows canonicalize 返回 \\?\ 前缀路径，两侧同源比较一致）。
    let canon_path = path.canonicalize().map_err(|_| format!("素材不存在: {}", safe_name))?;
    let canon_dir = dir.canonicalize().map_err(|e| format!("解析包目录失败: {}", e))?;
    if !canon_path.starts_with(&canon_dir) {
        return Err("非法的素材路径".to_string());
    }
    if !canon_path.is_file() {
        return Err(format!("素材不是文件: {}", safe_name));
    }
    let ext = canon_path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    let mime = match ext.as_str() {
        "gif" => "image/gif",
        "webm" => "video/webm",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        _ => return Err(format!("不支持的素材格式: .{}", ext)),
    };
    let bytes = std::fs::read(&canon_path).map_err(|e| format!("读取素材失败: {}", e))?;
    if bytes.len() > MAX_SINGLE_FILE_BYTES {
        return Err(format!("素材 {} 超过单文件 20MB 上限", safe_name));
    }
    Ok(format!(
        "data:{};base64,{}",
        mime,
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ===== sanitize_pack_id =====

    #[test]
    fn sanitize_pack_id_keeps_safe_chars_only() {
        assert_eq!(sanitize_pack_id("test-pack_1"), "test-pack_1");
        assert_eq!(sanitize_pack_id("my pet!"), "mypet");
        // 中文整段剥掉
        assert_eq!(sanitize_pack_id("我的宠物"), "");
    }

    #[test]
    fn sanitize_pack_id_strips_traversal() {
        // 路径穿越成分必须被剥干净：点号全滤掉
        assert_eq!(sanitize_pack_id("../../etc/passwd"), "etcpasswd");
        assert_eq!(sanitize_pack_id("..\\..\\win"), "win");
        assert_eq!(sanitize_pack_id(".."), "");
    }

    // ===== sanitize_file_name =====

    #[test]
    fn sanitize_file_name_takes_basename_and_rejects_dotdot() {
        assert_eq!(sanitize_file_name("idle.gif").unwrap(), "idle.gif");
        assert_eq!(
            sanitize_file_name("assets/sub/walk.webm").unwrap(),
            "walk.webm"
        );
        assert!(sanitize_file_name("..").is_err());
        assert!(sanitize_file_name("").is_err());
    }

    // ===== decode_pack_file 大小上限 =====

    #[test]
    fn decode_rejects_oversize_by_estimate() {
        // 28MB base64 串 → 估算 21MB > 20MB，解码前即拒
        let big = "QUFB".repeat(1024 * 1024 * 7); // ~28MB
        let err = decode_pack_file("big.gif", &big).unwrap_err();
        assert!(err.contains("20MB"), "should hit size cap: {}", err);
    }

    #[test]
    fn decode_rejects_bad_base64() {
        assert!(decode_pack_file("a.gif", "!!!!not-base64!!!!").is_err());
    }

    #[test]
    fn decode_roundtrip() {
        let (name, bytes) = decode_pack_file("dir/a.gif", "QUJB").unwrap();
        assert_eq!(name, "a.gif");
        assert_eq!(bytes, b"ABA");
    }

    // ===== validate_clips =====

    const UPLOADED: [&str; 3] = ["idle.gif", "walk.webm", "cube.webm"];

    fn uploaded_vec() -> Vec<String> {
        UPLOADED.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn validate_rejects_missing_idle() {
        let clips = json!({ "walk": { "file": "walk.webm", "type": "webm" } });
        let err = validate_clips(&clips, &uploaded_vec()).unwrap_err();
        assert!(err.contains("idle"), "should name idle: {}", err);
    }

    #[test]
    fn validate_rejects_bad_type() {
        let clips = json!({ "idle": { "file": "idle.gif", "type": "mp4" } });
        let err = validate_clips(&clips, &uploaded_vec()).unwrap_err();
        assert!(err.contains("type 非法"), "should name type: {}", err);
    }

    #[test]
    fn validate_rejects_file_not_uploaded() {
        let clips = json!({ "idle": { "file": "ghost.gif", "type": "gif" } });
        let err = validate_clips(&clips, &uploaded_vec()).unwrap_err();
        assert!(err.contains("ghost.gif"), "should name file: {}", err);
    }

    #[test]
    fn validate_rejects_incomplete_spritesheet() {
        let clips = json!({ "idle": { "file": "idle.gif", "type": "spritesheet", "frameW": 64 } });
        let err = validate_clips(&clips, &uploaded_vec()).unwrap_err();
        assert!(err.contains("spritesheet"), "should name spritesheet: {}", err);
    }

    #[test]
    fn validate_rejects_action_without_name() {
        let clips = json!({
            "idle": { "file": "idle.gif", "type": "gif" },
            "actions": [{ "file": "cube.webm", "type": "webm" }],
        });
        let err = validate_clips(&clips, &uploaded_vec()).unwrap_err();
        assert!(err.contains("name"), "should name action name: {}", err);
    }

    #[test]
    fn validate_rejects_non_positive_weight() {
        let clips = json!({
            "idle": { "file": "idle.gif", "type": "gif" },
            "actions": [{ "name": "玩魔方", "file": "cube.webm", "type": "webm", "weight": 0 }],
        });
        let err = validate_clips(&clips, &uploaded_vec()).unwrap_err();
        assert!(err.contains("weight"), "should name weight: {}", err);
    }

    #[test]
    fn validate_accepts_full_pack() {
        let clips = json!({
            "idle": { "file": "idle.gif", "type": "gif" },
            "walk": { "file": "walk.webm", "type": "webm" },
            "actions": [
                { "name": "玩魔方", "file": "cube.webm", "type": "webm", "weight": 3 },
            ],
        });
        assert!(validate_clips(&clips, &uploaded_vec()).is_ok());
    }
}
