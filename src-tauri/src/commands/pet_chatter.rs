//! pet_chatter — 桌宠「主动搭话」的单发 LLM 命令（方案 B2）。
//!
//! 模板同 summarize_work_content：system 一句话约束 + user 上下文，
//! `crate::llm::chat` 非流式单发。差别在失败语义——搭话是锦上添花，
//! 任何失败（未配置 LLM / 网络 / 超时 / kind 不认识）一律返回 `Ok(None)`，
//! 前端拿固定文案兜底，绝不为此弹错误。
//!
//! 上下文由前端附带（触发点手上就有新鲜数据，后端不读快照、不打禅道），
//! 后端只负责拼 prompt + 注入当前时间与星期。

use std::time::Duration;

use serde_json::Value;

/// 整体超时：气泡文案 5s 内没等到就用固定文案顶到底，不再等。
/// ChatRequest 自身的 timeout_ms 只管 HTTP，这里是兜住取凭证等全部环节的总闸。
const CHATTER_TIMEOUT: Duration = Duration::from_secs(5);

/// 各 kind 的语气要求（拼进 user prompt）。未识别的 kind 走通用问候——
/// 一句搭话不值得显式报错。
/// kind=custom（方案 C′ 插件 llm_line）：用插件自带的 instruction 当生成指令，
/// 指令缺失/为空时退回通用问候（清单校验层会先拦，这里是运行时兜底）。
fn tone_for_kind(kind: &str, context: &Value) -> String {
    if kind == "custom" {
        let inst = context
            .get("instruction")
            .and_then(|v| v.as_str())
            .map(|s| s.trim())
            .filter(|s| !s.is_empty());
        if let Some(inst) = inst {
            // 护栏同 context 行：异常超长的指令截断，别把 prompt 撑爆
            let cut: String = inst.chars().take(CONTEXT_INSTRUCTION_MAX_CHARS).collect();
            return format!("生成要求：{cut}");
        }
        return "语气：日常问候".to_string();
    }
    match kind {
        "morning_plan" => "语气：催办，提醒对方赶紧定好今天的计划",
        "evening_summary" => "语气：收尾，提醒对方看一眼今天的复盘、准备下班",
        "greeting_morning" => "语气：清晨问候，元气满满",
        "greeting_coffee" => "语气：午后问候，劝对方喝一杯放松一下",
        "greeting_late" => "语气：深夜劝休，温柔地催对方去休息",
        "overdue" => "语气：着急，强调任务已逾期、越拖越麻烦",
        _ => "语气：日常问候",
    }
    .to_string()
}

fn weekday_cn(now: &chrono::DateTime<chrono::Local>) -> &'static str {
    use chrono::Datelike;
    match now.weekday() {
        chrono::Weekday::Mon => "周一",
        chrono::Weekday::Tue => "周二",
        chrono::Weekday::Wed => "周三",
        chrono::Weekday::Thu => "周四",
        chrono::Weekday::Fri => "周五",
        chrono::Weekday::Sat => "周六",
        chrono::Weekday::Sun => "周日",
    }
}

/// 未知字段透传的护栏：context 是前端自由 JSON，别让超长字段把 prompt 撑爆
/// （正常触发点只传几个短字段，护栏只对异常调用生效）。
const CONTEXT_LINE_MAX_BYTES: usize = 200;
/// 单次搭话最多透传的上下文行数
const CONTEXT_MAX_LINES: usize = 16;
/// kind=custom 时插件生成指令的最大字符数（正常指令就一句话，护栏只对异常生效）
const CONTEXT_INSTRUCTION_MAX_CHARS: usize = 200;

/// 把前端附带的 context JSON 渲染成人话行。已知字段翻译成中文，
/// 未识别字段原样透传（前端以后加新触发点不用改这里）；
/// 人设字段（assistantName/userTitle）与 instruction（kind=custom 时已被
/// tone_for_kind 单独消费）不重复罗列。
fn render_context(context: &Value) -> Vec<String> {
    let mut lines = Vec::new();
    let Some(obj) = context.as_object() else {
        return lines;
    };
    for (key, value) in obj {
        match key.as_str() {
            "assistantName" | "userTitle" | "instruction" => {}
            "overdue" => {
                if let Some(n) = value.as_u64() {
                    lines.push(format!("当前逾期任务 {n} 个"));
                }
            }
            "todayDue" => {
                if let Some(n) = value.as_u64() {
                    lines.push(format!("今天到期任务 {n} 个"));
                }
            }
            "overdueTitle" => {
                if let Some(s) = value.as_str().filter(|s| !s.is_empty()) {
                    lines.push(format!("逾期最久的任务：「{s}」"));
                }
            }
            "overdueDays" => {
                if let Some(n) = value.as_i64() {
                    lines.push(format!("已经逾期 {n} 天"));
                }
            }
            other => {
                let rendered = format!("{other}: {value}");
                if rendered.len() <= CONTEXT_LINE_MAX_BYTES {
                    lines.push(rendered);
                }
            }
        }
    }
    lines.truncate(CONTEXT_MAX_LINES);
    lines
}

/// 单发 LLM 生成搭话文案。任何失败都向上返回 Err，由调用方统一降级 None。
async fn generate_line(req: crate::llm::ChatRequest) -> Result<Option<String>, String> {
    let resp = crate::llm::chat(req).await?;
    let text = resp.text.trim().to_string();
    // 空回复同 None：一句话都没拿到，让前端固定文案顶到底
    if text.is_empty() {
        Ok(None)
    } else {
        Ok(Some(text))
    }
}

#[tauri::command]
pub async fn pet_chatter(kind: String, context: Option<Value>) -> Result<Option<String>, String> {
    let context = context.unwrap_or(Value::Null);
    let user_title = context
        .get("userTitle")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("主人")
        .to_string();
    let assistant_name = context
        .get("assistantName")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("Jarvis")
        .to_string();

    // system 只管人设和格式硬约束；场景、数据、时间全放 user 段
    // （与 summarize_work_content 的两段式分法一致）。
    let system = format!(
        "你是桌宠助手{assistant_name}，正在向用户主动搭话。\
         只输出一句不超过 30 字的口语中文：不用列表、不用 markdown、不用引号，\
         最多带一个 emoji。直接输出这句话本身，不要任何前缀或解释。"
    );

    let mut user_parts = vec![
        format!("称呼用户为「{user_title}」。"),
        tone_for_kind(&kind, &context),
    ];
    user_parts.extend(render_context(&context));
    let now = chrono::Local::now();
    user_parts.push(format!(
        "当前时间：{}（{}）",
        now.format("%Y-%m-%d %H:%M"),
        weekday_cn(&now)
    ));
    user_parts.push("请现在开口，说这一句话。".to_string());

    let req = crate::llm::ChatRequest {
        timeout_ms: Some(CHATTER_TIMEOUT.as_millis() as u64),
        ..crate::llm::ChatRequest::new(vec![
            crate::llm::ChatMessage {
                role: crate::llm::Role::System,
                content: system,
                tool_calls: None,
                tool_call_id: None,
                name: None,
            },
            crate::llm::ChatMessage {
                role: crate::llm::Role::User,
                content: user_parts.join("\n"),
                tool_calls: None,
                tool_call_id: None,
                name: None,
            },
        ])
    };

    let fut = generate_line(req);
    // 双保险：HTTP 层 timeout_ms + 这里包住整体（含凭证读取）的硬超时
    match tokio::time::timeout(CHATTER_TIMEOUT, fut).await {
        Ok(result) => result.or_else(|e| {
            // LLM 未配置/网络失败等一切错误都降级为 None，前端用固定文案
            tracing::debug!(target: "pet_chatter", "搭话生成失败，走固定文案: {e}");
            Ok(None)
        }),
        Err(_) => {
            tracing::debug!(target: "pet_chatter", "搭话生成超时（{}s），走固定文案", CHATTER_TIMEOUT.as_secs());
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn custom_kind_uses_plugin_instruction() {
        let ctx = json!({ "instruction": "提醒用户喝水，语气关心" });
        let tone = tone_for_kind("custom", &ctx);
        assert!(tone.contains("提醒用户喝水"), "生成指令应进 prompt: {tone}");
    }

    #[test]
    fn custom_kind_without_instruction_falls_back_to_generic() {
        assert_eq!(tone_for_kind("custom", &json!({})), "语气：日常问候");
        assert_eq!(tone_for_kind("custom", &json!({ "instruction": "   " })), "语气：日常问候");
    }

    #[test]
    fn custom_instruction_is_length_capped() {
        let long = "长".repeat(500);
        let tone = tone_for_kind("custom", &json!({ "instruction": long }));
        assert!(tone.chars().count() <= CONTEXT_INSTRUCTION_MAX_CHARS + "生成要求：".len());
    }

    #[test]
    fn render_context_skips_instruction_and_persona() {
        let lines = render_context(&json!({
            "assistantName": "Jarvis",
            "userTitle": "主人",
            "instruction": "提醒喝水",
            "cards": 3
        }));
        assert_eq!(lines, vec!["cards: 3".to_string()]);
    }

    #[test]
    fn known_kinds_still_resolve_to_tones() {
        assert_eq!(tone_for_kind("greeting_morning", &json!({})), "语气：清晨问候，元气满满");
        assert_eq!(tone_for_kind("overdue", &json!({})), "语气：着急，强调任务已逾期、越拖越麻烦");
    }
}
