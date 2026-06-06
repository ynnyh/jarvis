/// LLM Profile 管理

use crate::commands;

// ===== Profile CRUD =====

/// 将当前 llm 配置保存为一个新的 profile（或更新已有 profile）
#[tauri::command]
pub async fn llm_profile_save(
    profile_id: String,
    name: String,
) -> Result<serde_json::Value, String> {
    let path = commands::config_path();
    let mut cfg: serde_json::Value = if path.exists() {
        let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        serde_json::json!({})
    };
    let llm = cfg.get("llm").cloned().unwrap_or_default();
    let mut profile = llm.clone();
    if let Some(obj) = profile.as_object_mut() {
        obj.insert("id".into(), serde_json::Value::String(profile_id.clone()));
        obj.insert("name".into(), serde_json::Value::String(name));
    }

    if let Some(key) = crate::settings::secret_get("llm.apiKey") {
        let _ = crate::settings::secret_set(&format!("llm.profile.{}.apiKey", profile_id), &key);
    }

    if cfg.get("llmProfiles").is_none() {
        cfg.as_object_mut()
            .ok_or("配置文件顶层不是 JSON 对象")?
            .insert("llmProfiles".into(), serde_json::json!([]));
    }
    let profiles = cfg
        .get_mut("llmProfiles")
        .and_then(|v| v.as_array_mut())
        .unwrap();
    if let Some(idx) = profiles
        .iter()
        .position(|p| p.get("id").and_then(|v| v.as_str()) == Some(&profile_id))
    {
        profiles[idx] = profile;
    } else {
        profiles.push(profile);
    }

    cfg.as_object_mut()
        .ok_or("配置文件顶层不是 JSON 对象")?
        .insert(
            "activeLlmProfileId".into(),
            serde_json::Value::String(profile_id),
        );

    commands::strip_secrets_for_save(&mut cfg)?;
    let content = serde_json::to_string_pretty(&cfg).unwrap_or_default();
    crate::util::write_atomic(&path, &content).map_err(|e| e.to_string())?;

    commands::hydrate_secret_placeholders(&mut cfg);
    let defaults = commands::default_config();
    commands::merge_defaults(&mut cfg, &defaults);
    Ok(cfg)
}

/// 切换到指定 profile：把该 profile 的字段复制到 llm，apiKey 从 keychain 槽位拷到 llm.apiKey
#[tauri::command]
pub async fn llm_profile_switch(profile_id: String) -> Result<serde_json::Value, String> {
    let path = commands::config_path();
    let mut cfg: serde_json::Value = if path.exists() {
        let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        return Err("No config found".into());
    };

    let current_id = cfg
        .get("activeLlmProfileId")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if !current_id.is_empty() {
        if let Some(key) = crate::settings::secret_get("llm.apiKey") {
            let _ =
                crate::settings::secret_set(&format!("llm.profile.{}.apiKey", current_id), &key);
        }
    }

    let target = cfg
        .get("llmProfiles")
        .and_then(|v| v.as_array())
        .and_then(|arr| {
            arr.iter()
                .find(|p| p.get("id").and_then(|v| v.as_str()) == Some(&profile_id))
                .cloned()
        })
        .ok_or("Profile not found")?;

    if let Some(llm) = cfg.get_mut("llm").and_then(|v| v.as_object_mut()) {
        for key in &["provider", "baseUrl", "model", "wireApi"] {
            if let Some(val) = target.get(*key) {
                llm.insert(key.to_string(), val.clone());
            }
        }
    }

    let profile_key = format!("llm.profile.{}.apiKey", profile_id);
    if let Some(key) = crate::settings::secret_get(&profile_key) {
        let _ = crate::settings::secret_set("llm.apiKey", &key);
    } else {
        let _ = crate::settings::secret_clear("llm.apiKey");
    }

    cfg.as_object_mut()
        .ok_or("配置文件顶层不是 JSON 对象")?
        .insert(
            "activeLlmProfileId".into(),
            serde_json::Value::String(profile_id),
        );

    commands::strip_secrets_for_save(&mut cfg)?;
    let content = serde_json::to_string_pretty(&cfg).unwrap_or_default();
    crate::util::write_atomic(&path, &content).map_err(|e| e.to_string())?;

    commands::hydrate_secret_placeholders(&mut cfg);
    let defaults = commands::default_config();
    commands::merge_defaults(&mut cfg, &defaults);
    Ok(cfg)
}

/// 删除指定 profile
#[tauri::command]
pub async fn llm_profile_delete(profile_id: String) -> Result<serde_json::Value, String> {
    let path = commands::config_path();
    let mut cfg: serde_json::Value = if path.exists() {
        let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        return Err("No config found".into());
    };

    if let Some(profiles) = cfg.get_mut("llmProfiles").and_then(|v| v.as_array_mut()) {
        profiles.retain(|p| p.get("id").and_then(|v| v.as_str()) != Some(&profile_id));
    }
    let _ = crate::settings::secret_clear(&format!("llm.profile.{}.apiKey", profile_id));

    let active_id = cfg
        .get("activeLlmProfileId")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if active_id == profile_id {
        let next_profile = cfg
            .get("llmProfiles")
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .cloned();
        if let Some(next) = next_profile {
            let next_id = next
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if let Some(llm) = cfg.get_mut("llm").and_then(|v| v.as_object_mut()) {
                for key in &["provider", "baseUrl", "model", "wireApi"] {
                    if let Some(val) = next.get(*key) {
                        llm.insert(key.to_string(), val.clone());
                    }
                }
            }
            cfg.as_object_mut()
                .ok_or("配置文件顶层不是 JSON 对象")?
                .insert(
                    "activeLlmProfileId".into(),
                    serde_json::Value::String(next_id.clone()),
                );
            let next_key = format!("llm.profile.{}.apiKey", next_id);
            if let Some(key) = crate::settings::secret_get(&next_key) {
                let _ = crate::settings::secret_set("llm.apiKey", &key);
            } else {
                let _ = crate::settings::secret_clear("llm.apiKey");
            }
        } else {
            cfg.as_object_mut()
                .ok_or("配置文件顶层不是 JSON 对象")?
                .insert(
                    "activeLlmProfileId".into(),
                    serde_json::Value::String(String::new()),
                );
            if let Some(llm) = cfg.get_mut("llm").and_then(|v| v.as_object_mut()) {
                if let Some(default_llm) = commands::default_config().get("llm").and_then(|v| v.as_object()) {
                    for key in &["provider", "baseUrl", "model", "wireApi"] {
                        if let Some(val) = default_llm.get(*key) {
                            llm.insert(key.to_string(), val.clone());
                        }
                    }
                }
                llm.insert("apiKey".into(), serde_json::Value::String(String::new()));
            }
            let _ = crate::settings::secret_clear("llm.apiKey");
        }
    }

    commands::strip_secrets_for_save(&mut cfg)?;
    let content = serde_json::to_string_pretty(&cfg).unwrap_or_default();
    crate::util::write_atomic(&path, &content).map_err(|e| e.to_string())?;

    commands::hydrate_secret_placeholders(&mut cfg);
    let defaults = commands::default_config();
    commands::merge_defaults(&mut cfg, &defaults);
    Ok(cfg)
}

/// 从表单字段直接 upsert profile（新增或编辑）
#[tauri::command]
pub async fn llm_profile_upsert(
    profile_id: String,
    name: String,
    provider: String,
    base_url: String,
    model: String,
    api_key: String,
    wire_api: Option<String>,
) -> Result<serde_json::Value, String> {
    let path = commands::config_path();
    let mut cfg: serde_json::Value = if path.exists() {
        let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        serde_json::json!({})
    };
    let current_active_id = cfg
        .get("activeLlmProfileId")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let mut profile = serde_json::json!({
        "id": profile_id,
        "name": name,
        "provider": provider,
        "baseUrl": base_url,
        "model": model,
    });
    if let Some(w) = wire_api {
        profile
            .as_object_mut()
            .unwrap()
            .insert("wireApi".into(), serde_json::Value::String(w));
    }

    let keychain_key = format!("llm.profile.{}.apiKey", profile_id);
    if !api_key.is_empty() {
        let _ = crate::settings::secret_set(&keychain_key, &api_key);
    }

    if cfg.get("llmProfiles").is_none() {
        cfg.as_object_mut()
            .ok_or("配置文件顶层不是 JSON 对象")?
            .insert("llmProfiles".into(), serde_json::json!([]));
    }
    let profiles = cfg
        .get_mut("llmProfiles")
        .and_then(|v| v.as_array_mut())
        .unwrap();
    if let Some(idx) = profiles
        .iter()
        .position(|p| p.get("id").and_then(|v| v.as_str()) == Some(&profile_id))
    {
        profiles[idx] = profile;
    } else {
        profiles.push(profile);
    }

    let should_activate = current_active_id.is_empty() || current_active_id == profile_id;
    if should_activate {
        cfg.as_object_mut().unwrap().insert(
            "activeLlmProfileId".into(),
            serde_json::Value::String(profile_id.clone()),
        );
    }

    let target = cfg
        .get("llmProfiles")
        .and_then(|v| v.as_array())
        .and_then(|arr| {
            arr.iter()
                .find(|p| p.get("id").and_then(|v| v.as_str()) == Some(&profile_id))
                .cloned()
        })
        .ok_or("Profile not found after upsert")?;
    if should_activate {
        if let Some(llm) = cfg.get_mut("llm").and_then(|v| v.as_object_mut()) {
            for key in &["provider", "baseUrl", "model", "wireApi"] {
                if let Some(val) = target.get(*key) {
                    llm.insert(key.to_string(), val.clone());
                }
            }
        }
    }
    if should_activate {
        if !api_key.is_empty() {
            let _ = crate::settings::secret_set("llm.apiKey", &api_key);
        } else if let Some(existing) = crate::settings::secret_get(&keychain_key) {
            let _ = crate::settings::secret_set("llm.apiKey", &existing);
        }
    }

    commands::strip_secrets_for_save(&mut cfg)?;
    let content = serde_json::to_string_pretty(&cfg).unwrap_or_default();
    crate::util::write_atomic(&path, &content).map_err(|e| e.to_string())?;

    commands::hydrate_secret_placeholders(&mut cfg);
    let defaults = commands::default_config();
    commands::merge_defaults(&mut cfg, &defaults);
    Ok(cfg)
}

// ===== Profile 测试 =====

#[tauri::command]
pub async fn llm_profile_test(
    profile_id: Option<String>,
    provider: String,
    base_url: String,
    model: String,
    api_key: String,
    allow_saved_key_when_empty: Option<bool>,
    wire_api: Option<String>,
) -> Result<serde_json::Value, String> {
    let base_url = base_url.trim().to_string();
    let model = model.trim().to_string();
    if base_url.is_empty() {
        return Err("LLM baseUrl 未配置".into());
    }
    if model.is_empty() {
        return Err("LLM model 未配置".into());
    }

    let mut resolved_key = api_key.trim().to_string();
    if resolved_key.is_empty() && allow_saved_key_when_empty.unwrap_or(false) {
        if let Some(id) = profile_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            resolved_key = crate::settings::secret_get(&format!("llm.profile.{}.apiKey", id))
                .unwrap_or_default();
        }
    }
    if resolved_key.is_empty() {
        return Err("请先填写 API Key，或保存过该模型的 API Key 后再测试".into());
    }

    let messages = vec![
        crate::llm::ChatMessage {
            role: crate::llm::Role::System,
            content: "只回复一个字：好".to_string(),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        },
        crate::llm::ChatMessage {
            role: crate::llm::Role::User,
            content: "ping".to_string(),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        },
    ];
    let mut req = crate::llm::ChatRequest::new(messages);
    req.max_tokens = Some(8);
    req.timeout_ms = Some(30_000);

    let cred = crate::settings::LlmCredentials {
        provider,
        base_url,
        model,
        api_key: resolved_key,
        wire_api: match wire_api.as_deref() {
            Some("responses") => "responses".to_string(),
            Some("anthropic") => "anthropic".to_string(),
            _ => "chat".to_string(),
        },
    };
    let resp = crate::llm::chat_with_credentials(req, cred).await?;
    Ok(serde_json::json!({
        "text": resp.text,
        "tokensIn": resp.tokens_in,
        "tokensOut": resp.tokens_out,
        "model": resp.model,
    }))
}

// ===== CC Switch 全量导入 =====

/// 扫描 CC Switch 数据库中全部 providers，返回摘要列表（不含 apiKey）。
#[tauri::command]
pub async fn cc_switch_list_providers() -> Result<serde_json::Value, String> {
    let providers = crate::tools::cc_switch_import::list_cc_switch_providers()?;
    serde_json::to_value(&providers).map_err(|e| e.to_string())
}

/// 按 provider ID 导入单个 CC Switch provider 为 llmProfile。
#[tauri::command]
pub async fn cc_switch_import_provider(provider_id: String) -> Result<serde_json::Value, String> {
    let result = crate::tools::cc_switch_import::import_cc_switch_provider_by_id(&provider_id).await?;
    serde_json::to_value(&result).map_err(|e| e.to_string())
}
