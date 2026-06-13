/// 自定义宠物管理：上传、列表、删除
///
/// 自定义宠物存储在 ~/.jarvis/custom-pets/ 目录下，每个宠物一个 JSON 文件。
/// 文件名 = pet-id.json，内容包含元数据 + Lottie/图片/GIF 数据。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomPet {
    pub id: String,
    pub name: String,
    pub description: String,
    /// "lottie" | "image" | "gif"
    #[serde(rename = "type")]
    pub pet_type: String,
    /// Lottie JSON 对象，或 Base64 编码的图片/GIF 数据
    pub data: serde_json::Value,
    /// 仅 image 类型有效：动画效果 "breath" | "swing" | "none"
    #[serde(default)]
    pub animation: String,
}

/// 自定义宠物存储目录
fn custom_pets_dir() -> PathBuf {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_default();
    PathBuf::from(home).join(".jarvis").join("custom-pets")
}

fn pet_file_path(id: &str) -> PathBuf {
    custom_pets_dir().join(format!("{}.json", id))
}

/// 列出所有自定义宠物（只返回元数据，不含 data 字段以节省传输）
#[tauri::command]
pub fn custom_pet_list() -> Result<Vec<CustomPet>, String> {
    let dir = custom_pets_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut pets = Vec::new();
    let entries =
        std::fs::read_dir(&dir).map_err(|e| format!("读取自定义宠物目录失败: {}", e))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let content =
            std::fs::read_to_string(&path).map_err(|e| format!("读取宠物文件失败: {}", e))?;
        match serde_json::from_str::<CustomPet>(&content) {
            Ok(pet) => pets.push(pet),
            Err(_) => continue,
        }
    }
    pets.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(pets)
}

/// 保存自定义宠物（新建或更新）
#[tauri::command]
pub fn custom_pet_save(pet: CustomPet) -> Result<(), String> {
    let dir = custom_pets_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建自定义宠物目录失败: {}", e))?;

    let path = pet_file_path(&pet.id);
    let content =
        serde_json::to_string_pretty(&pet).map_err(|e| format!("序列化宠物数据失败: {}", e))?;

    crate::util::write_atomic(&path, &content).map_err(|e| format!("写入宠物文件失败: {}", e))?;
    Ok(())
}

/// 删除自定义宠物
#[tauri::command]
pub fn custom_pet_delete(id: String) -> Result<(), String> {
    let path = pet_file_path(&id);
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| format!("删除宠物文件失败: {}", e))?;
    }
    Ok(())
}
