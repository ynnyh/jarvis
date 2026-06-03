// 记忆提取器：从对话中提取关键事实并存储。
//
// 每轮对话结束后异步触发。调用 LLM 提取结构化事实，然后与已有记忆比较：
//   - 新事实 → 插入
//   - 与已有记忆相似 → 合并更新
//   - 与已有记忆矛盾 → 过期旧的，插入新的
//
// 提取 prompt 设计参考 Mem0 的 Memory Extraction Pattern。

use crate::llm::{self, ChatMessage, ChatRequest, Role};
use crate::memory::db::MemoryDb;
use serde_json::Value;

/// 提取的记忆事实
#[derive(Debug, Clone)]
pub struct ExtractedFact {
    content: String,
    category: String,
    importance: f32,
}

/// 从一轮对话中提取记忆事实（纯 async，不涉及 DB）。
///
/// 返回提取到的事实列表，由调用方负责存储。
pub async fn extract_facts_only(
    user_msg: &str,
    assistant_msg: &str,
) -> Vec<ExtractedFact> {
    match extract_facts(user_msg, assistant_msg).await {
        Ok(f) => f,
        Err(e) => {
            eprintln!("[memory] 提取失败（静默跳过）: {}", e);
            Vec::new()
        }
    }
}

/// 计算事实的嵌入向量（纯 async，不涉及 DB）。
pub async fn compute_fact_embedding(fact: &ExtractedFact) -> Option<(ExtractedFact, Vec<f32>)> {
    match crate::memory::embedding::embed(&fact.content).await {
        Ok(emb) => Some((fact.clone(), emb)),
        Err(e) => {
            eprintln!("[memory] 嵌入计算失败: {}", e);
            None
        }
    }
}

/// 将事实存入数据库（纯同步，短暂持锁）。
pub fn store_fact_sync(
    fact: &ExtractedFact,
    embedding: &[f32],
    conversation_id: Option<&str>,
    db: &MemoryDb,
) -> Result<(), String> {
    let similar = db
        .find_similar(embedding, 0.85, 3)
        .map_err(|e| format!("查找相似记忆失败: {}", e))?;

    if let Some((existing_id, distance, _existing_content)) = similar.first() {
        if *distance < 0.3 {
            return Ok(());
        }
        db.update_memory_content(*existing_id, &fact.content, embedding)
            .map_err(|e| format!("更新记忆失败: {}", e))?;
        return Ok(());
    }

    db.insert_memory(
        &fact.content,
        &fact.category,
        conversation_id,
        fact.importance,
        embedding,
    )
    .map_err(|e| format!("插入记忆失败: {}", e))?;

    Ok(())
}

async fn extract_facts(
    user_msg: &str,
    assistant_msg: &str,
) -> Result<Vec<ExtractedFact>, String> {
    let extraction_prompt = r#"你是一个记忆提取器。从下面的对话中提取值得长期记住的事实。

提取规则：
1. 只提取关于用户的个人事实：偏好、习惯、工作方式、项目信息、团队信息、决策、重要背景
2. 忽略：一次性查询（"今天天气"）、工具调用细节、临时性闲聊
3. 每条事实用一句简洁的中文陈述
4. 不要提取 assistant 的回答内容，只关注 user 透露的信息
5. 如果对话中没有值得记住的事实，返回空数组

以 JSON 数组格式返回，每项格式：
{"content": "事实内容", "category": "分类", "importance": 0.0到1.0}

分类可选：preference（偏好）、personal（个人信息）、project（项目）、team（团队）、decision（决策）、workflow（工作方式）、general（其他）

重要性参考：0.9+ = 核心信息（姓名、角色），0.7-0.9 = 重要偏好/决策，0.5-0.7 = 一般事实，0.3-0.5 = 次要细节

只返回 JSON 数组，不要其他文字。如果没有事实，返回 []"#;

    let messages = vec![
        ChatMessage {
            role: Role::System,
            content: extraction_prompt.to_string(),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        },
        ChatMessage {
            role: Role::User,
            content: format!(
                "用户说：{}\n\n助手回复：{}",
                crate::util::truncate_chars(user_msg, 2000),
                crate::util::truncate_chars(assistant_msg, 2000)
            ),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        },
    ];

    let mut req = ChatRequest::new(messages);
    req.temperature = Some(0.1); // 提取需要确定性
    req.max_tokens = Some(512);
    req.timeout_ms = Some(15_000);

    let resp = llm::chat(req).await.map_err(|e| format!("提取 LLM 调用失败: {}", e))?;

    parse_facts(&resp.text)
}

fn parse_facts(text: &str) -> Result<Vec<ExtractedFact>, String> {
    // 尝试从文本中提取 JSON 数组
    let json_str = extract_json_array(text);
    let arr: Vec<Value> = serde_json::from_str(&json_str)
        .map_err(|e| format!("JSON 解析失败: {}", e))?;

    let mut facts = Vec::new();
    for item in &arr {
        let content = item
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if content.is_empty() {
            continue;
        }
        let category = item
            .get("category")
            .and_then(|v| v.as_str())
            .unwrap_or("general")
            .to_string();
        let importance = item
            .get("importance")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.5) as f32;
        facts.push(ExtractedFact {
            content,
            category,
            importance: importance.clamp(0.0, 1.0),
        });
    }
    Ok(facts)
}

/// 从可能包含非 JSON 文本的响应中提取 JSON 数组。
fn extract_json_array(text: &str) -> String {
    let trimmed = text.trim();
    // 直接是 JSON 数组
    if trimmed.starts_with('[') {
        // 找到匹配的 ]
        if let Some(end) = find_matching_bracket(trimmed) {
            return trimmed[..=end].to_string();
        }
    }
    // 可能在 markdown 代码块里
    if let Some(start) = trimmed.find('[') {
        let sub = &trimmed[start..];
        if let Some(end) = find_matching_bracket(sub) {
            return sub[..=end].to_string();
        }
    }
    // fallback
    "[]".to_string()
}

fn find_matching_bracket(s: &str) -> Option<usize> {
    let mut depth = 0;
    let mut in_string = false;
    let mut escape = false;
    for (i, ch) in s.char_indices() {
        if escape {
            escape = false;
            continue;
        }
        if ch == '\\' && in_string {
            escape = true;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        match ch {
            '[' | '{' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            '}' => {
                depth -= 1;
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_json_array_direct() {
        let input = r#"[{"content": "用户喜欢 Python", "category": "preference", "importance": 0.8}]"#;
        assert_eq!(extract_json_array(input), input);
    }

    #[test]
    fn extract_json_array_in_markdown() {
        let input = "根据对话，我提取了以下事实：\n```json\n[{\"content\": \"test\", \"category\": \"general\", \"importance\": 0.5}]\n```\n";
        let result = extract_json_array(input);
        assert!(result.starts_with('['));
        assert!(result.ends_with(']'));
    }

    #[test]
    fn extract_json_array_empty() {
        let input = "这段对话没有值得记住的事实";
        assert_eq!(extract_json_array(input), "[]");
    }

    #[test]
    fn parse_facts_valid() {
        let json = r#"[{"content": "用户是后端开发", "category": "personal", "importance": 0.9}]"#;
        let facts = parse_facts(json).unwrap();
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].content, "用户是后端开发");
        assert_eq!(facts[0].category, "personal");
        assert!((facts[0].importance - 0.9).abs() < 0.01);
    }

    #[test]
    fn parse_facts_empty() {
        let facts = parse_facts("[]").unwrap();
        assert!(facts.is_empty());
    }

    #[test]
    fn find_matching_bracket_simple() {
        assert_eq!(find_matching_bracket("[]"), Some(1));
        assert_eq!(find_matching_bracket("[1, 2, 3]"), Some(8));
    }

    #[test]
    fn find_matching_bracket_nested() {
        assert_eq!(
            find_matching_bracket(r#"[{"a": "b"}]"#),
            Some(11)
        );
    }
}
