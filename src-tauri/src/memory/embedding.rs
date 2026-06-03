// 嵌入服务：调用 LLM 提供商的 /embeddings API 生成向量。
//
// 复用现有 LLM 凭证（settings::get_llm_credentials），零额外依赖。
// 大部分 OpenAI 兼容服务商（DeepSeek、OpenAI、Moonshot 等）都支持 /embeddings 端点。
//
// 嵌入 base URL 优先级：config.embeddingBaseUrl > LLM 提供商 URL。
// 高级用户可配置 embeddingBaseUrl 指向本地 Ollama 等服务实现离线嵌入。

use crate::settings::get_llm_credentials;
use serde::{Deserialize, Serialize};

const EMBEDDING_DIM: usize = 384;

#[derive(Debug, Serialize)]
struct EmbedRequest {
    model: String,
    input: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    dimensions: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct EmbedResponse {
    data: Vec<EmbedData>,
}

#[derive(Debug, Deserialize)]
struct EmbedData {
    embedding: Vec<f32>,
}

/// 从 config.json 读取 embeddingBaseUrl 配置项。
/// 未配置时返回 None，由调用方回退到 LLM 提供商 URL。
fn get_embedding_base_url() -> Option<String> {
    crate::settings::load_raw_config()
        .and_then(|v| v.get("embeddingBaseUrl")?.as_str().map(|s| s.trim().to_string()))
        .filter(|s| !s.is_empty())
}

/// 对文本生成嵌入向量。
///
/// 使用当前配置的 LLM 服务商的 /embeddings 端点。
/// 若配置了 embeddingBaseUrl，则优先使用该地址（可指向本地 Ollama 等）。
/// 若服务商不支持 embeddings，返回错误，由调用方跳过向量检索。
pub async fn embed(text: &str) -> Result<Vec<f32>, String> {
    let cred = get_llm_credentials();
    if cred.api_key.is_empty() || cred.base_url.is_empty() {
        return Err("LLM 凭证未配置".into());
    }

    // embeddingBaseUrl 优先级高于 LLM 提供商 URL
    let base_url = get_embedding_base_url().unwrap_or_else(|| cred.base_url.clone());
    let url = build_embeddings_url(&base_url);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("embed client 构造失败: {}", e))?;

    // text-embedding-3-small 或 text-embedding-v3 等，取决于提供商。
    // 这里用通用的 text-embedding-3-small，大多数服务商兼容。
    // 传 dimensions 参数让模型直接返回目标维度，避免事后截断丢信息。
    let body = EmbedRequest {
        model: "text-embedding-3-small".into(),
        input: vec![text.to_string()],
        dimensions: Some(EMBEDDING_DIM),
    };

    let resp = client
        .post(&url)
        .bearer_auth(&cred.api_key)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("embed 请求失败: {}", e))?;

    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        // 模型不存在时返回错误，让调用方跳过向量检索
        if status.as_u16() == 404 || text.contains("model_not_found") {
            return Err("embed 模型不可用".into());
        }
        return Err(format!("embed HTTP {}: {}", status.as_u16(), crate::util::truncate_chars(&text, 300)));
    }

    let data: EmbedResponse = resp
        .json()
        .await
        .map_err(|e| format!("embed 响应解析失败: {}", e))?;

    let embedding = data
        .data
        .into_iter()
        .next()
        .map(|d| d.embedding)
        .ok_or("embed 响应中无向量数据")?;

    // 如果返回的维度和期望不一致，填充或截断到 EMBEDDING_DIM
    Ok(normalize_dim(embedding, EMBEDDING_DIM))
}

#[allow(dead_code)]
pub fn embedding_dim() -> usize {
    EMBEDDING_DIM
}

fn build_embeddings_url(raw_base: &str) -> String {
    let trimmed = raw_base.trim_end_matches('/');
    let has_custom_prefix = match reqwest::Url::parse(trimmed) {
        Ok(u) => {
            let p = u.path();
            !(p.is_empty() || p == "/")
        }
        Err(_) => false,
    };
    if has_custom_prefix {
        format!("{}/embeddings", trimmed)
    } else {
        format!("{}/v1/embeddings", trimmed)
    }
}

fn normalize_dim(mut v: Vec<f32>, target: usize) -> Vec<f32> {
    if v.len() == target {
        return v;
    }
    if v.len() > target {
        v.truncate(target);
        return v;
    }
    v.resize(target, 0.0);
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_embeddings_url_bare_host() {
        assert_eq!(
            build_embeddings_url("https://api.deepseek.com"),
            "https://api.deepseek.com/v1/embeddings"
        );
    }

    #[test]
    fn build_embeddings_url_with_prefix() {
        assert_eq!(
            build_embeddings_url("https://api.openai.com/v1"),
            "https://api.openai.com/v1/embeddings"
        );
    }

    #[test]
    fn normalize_dim_pads() {
        let v = vec![1.0f32; 10];
        let out = normalize_dim(v, 384);
        assert_eq!(out.len(), 384);
    }

    #[test]
    fn normalize_dim_truncates() {
        let v = vec![1.0f32; 512];
        let out = normalize_dim(v, 384);
        assert_eq!(out.len(), 384);
    }
}
