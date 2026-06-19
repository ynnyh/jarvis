use serde::{Deserialize, Serialize};

/// 帆软报表模板路径。曾在 example/effort-report.cpt，但该模板在 sxjbjt.com:19085
/// 服务器上不存在，open_report 会拿到 11300004 错误页，连锁导致 parameters_d 401。
/// 实际路径见浏览器地址栏 viewlet= 参数（zentao/chandaogongshitongji.cpt）。
pub(super) const DEFAULT_VIEWLET: &str = "zentao/chandaogongshitongji.cpt";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EffortRecord {
    pub date: String,
    pub department: String,
    pub employee: String,
    pub daily_total_hours: f32,
    pub item_hours: f32,
    pub project_name: String,
    pub system: String,
    pub task_name: String,
    pub work_content: String,
}

/// 解析 reportIndex=1「禅道工时统计明细」的 HTML。
///
/// FR x-table 4 区结构（frozen-corner / frozen-north / frozen-west / frozen-center）：
/// 我们只关心 frozen-center 里 row>=4 的 data row。
///
/// 每个 td 形如：
///   `<td ... col="X" ... row="Y" [rowSpan="N"] ...><div ...>TEXT</div></td>`
///
/// 列定义（9 列）：0 部门 / 1 员工 / 2 日期 / 3 当日总工时 / 4 单项工时 /
///                  5 项目名称 / 6 所属系统 / 7 任务名称 / 8 工作内容
///
/// 跨行合并：col 0-3（人员维度）会用 `rowSpan="N"` 合并多行，后续行直接缺这些 col 的 td，
/// 解析时需要把 rowSpan 覆盖区回填到 (row+1..row+N) 同 col。
///
/// 要跳过的：合计行（col=0 文本"合计："+ colSpan=3）、备注行（display:none）、隐藏行
/// （style 含 `display:none`）。
pub fn parse_detail_html(html: &str) -> Result<Vec<EffortRecord>, String> {
    use std::collections::HashMap;

    // 第一步：找 frozen-center div 范围。它在 HTML 最后一个 <td valign="top"> 里。
    // 简化处理：直接全文 regex，反正其他区只有表头，row>=4 的数据 td 只在 frozen-center 里。
    let td_re = regex::Regex::new(r#"<td\b([^>]*?)>\s*<div[^>]*>([\s\S]*?)</div>\s*</td>"#)
        .map_err(|e| format!("td regex 编译失败: {}", e))?;
    let attr_re = regex::Regex::new(r#"(col|row|rowSpan|colSpan|style)="([^"]*)""#)
        .map_err(|e| format!("attr regex 编译失败: {}", e))?;

    // (row, col) → cell text
    let mut cells: HashMap<(u32, u32), String> = HashMap::new();
    let mut max_row: u32 = 0;
    let mut skip_rows: std::collections::HashSet<u32> = std::collections::HashSet::new();

    for cap in td_re.captures_iter(html) {
        let attrs = &cap[1];
        let inner = cap[2].trim();

        let mut col: Option<u32> = None;
        let mut row: Option<u32> = None;
        let mut row_span: u32 = 1;
        let mut col_span: u32 = 1;
        let mut hidden = false;
        for a in attr_re.captures_iter(attrs) {
            match &a[1] {
                "col" => col = a[2].parse().ok(),
                "row" => row = a[2].parse().ok(),
                "rowSpan" => row_span = a[2].parse().unwrap_or(1),
                "colSpan" => col_span = a[2].parse().unwrap_or(1),
                "style"
                    if a[2].contains("display:none") => {
                        hidden = true;
                    }
                _ => {}
            }
        }
        // tr 级 display:none（"备注：" 那行）：td 自身 style 通常不写 display:none，
        // 由父 tr 控制。但实测备注行的 td 也带 max-height:0px 的 div，文本是"备注："，
        // 直接按文本 = "备注：" 跳即可。
        let (col, row) = match (col, row) {
            (Some(c), Some(r)) => (c, r),
            _ => continue,
        };
        // 跳标题/表头/合计/备注：
        // - row < 4: 标题(1) 表头(2) 合计(3)
        // - 文本=备注：（row=12）
        if row < 4 {
            continue;
        }
        if inner == "备注：" || hidden {
            skip_rows.insert(row);
            continue;
        }

        let text = html_to_plain_text(&decode_html_entities(inner));

        // 主单元格写入，rowSpan>1 时覆盖到下面行的同列
        for rr in row..row.saturating_add(row_span) {
            for cc in col..col.saturating_add(col_span) {
                cells.entry((rr, cc)).or_insert_with(|| text.clone());
            }
        }
        if row + row_span - 1 > max_row {
            max_row = row + row_span - 1;
        }
    }

    let mut records: Vec<EffortRecord> = Vec::new();
    for r in 4..=max_row {
        if skip_rows.contains(&r) {
            continue;
        }
        // 至少要有 col 4（单项工时）才算一条明细记录；col 0-3 可能从 rowSpan 回填
        let item_hours_raw = match cells.get(&(r, 4)) {
            Some(v) if !v.is_empty() => v.clone(),
            _ => continue,
        };
        let department = cells.get(&(r, 0)).cloned().unwrap_or_default();
        let employee = cells.get(&(r, 1)).cloned().unwrap_or_default();
        let date = cells.get(&(r, 2)).cloned().unwrap_or_default();
        let daily_total_raw = cells.get(&(r, 3)).cloned().unwrap_or_default();
        let project_name = cells.get(&(r, 5)).cloned().unwrap_or_default();
        let system = cells.get(&(r, 6)).cloned().unwrap_or_default();
        let task_name = cells.get(&(r, 7)).cloned().unwrap_or_default();
        let work_content = cells.get(&(r, 8)).cloned().unwrap_or_default();

        // 跳合计行：col=0 文本"合计："
        if department == "合计：" {
            continue;
        }

        records.push(EffortRecord {
            date,
            department,
            employee,
            daily_total_hours: daily_total_raw.parse::<f32>().unwrap_or(0.0),
            item_hours: item_hours_raw.parse::<f32>().unwrap_or(0.0),
            project_name,
            system,
            task_name,
            work_content,
        });
    }
    Ok(records)
}

/// 解 HTML 实体：FR div 文本里偶尔会出现 `&amp;` `&lt;` `&#45;` 等。
/// 但实测 div 里大多已经是明文，这里做兜底。
fn decode_html_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'&' {
            if let Some(end) = s[i..].find(';') {
                let entity = &s[i + 1..i + end];
                let decoded: Option<char> = if let Some(rest) = entity.strip_prefix('#') {
                    if let Some(hex) = rest.strip_prefix('x').or_else(|| rest.strip_prefix('X')) {
                        u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)
                    } else {
                        rest.parse::<u32>().ok().and_then(char::from_u32)
                    }
                } else {
                    match entity {
                        "amp" => Some('&'),
                        "lt" => Some('<'),
                        "gt" => Some('>'),
                        "quot" => Some('"'),
                        "apos" => Some('\''),
                        "nbsp" => Some(' '),
                        _ => None,
                    }
                };
                if let Some(ch) = decoded {
                    out.push(ch);
                    i += end + 1;
                    continue;
                }
            }
        }
        // safe utf-8 推进：找下一个字符边界
        let mut step = 1;
        while i + step < bytes.len() && !s.is_char_boundary(i + step) {
            step += 1;
        }
        out.push_str(&s[i..i + step]);
        i += step;
    }
    out
}

/// Strip HTML tags and clean up rich-text content from FineReport work content cells.
/// FR sometimes stores `<p><span style="...">text</span></p>` which ends up as raw tags
/// after decode_html_entities. This extracts the visible text only.
fn html_to_plain_text(s: &str) -> String {
    // Remove all HTML tags
    let re = regex::Regex::new(r"<[^>]*>").unwrap();
    let stripped = re.replace_all(s, "").to_string();
    // Collapse whitespace
    let collapsed = stripped
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    // Replace literal \r\n / \n (FR stores these as visible text)
    let cleaned = collapsed
        .replace("\\r\\n", "")
        .replace("\\n", "")
        .replace("  ", " ")
        .trim()
        .to_string();
    cleaned
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 极简 fixture：还原明细 HTML 的关键结构——2 名员工，各 1-2 条记录。
    /// 覆盖 rowSpan 回填 / 合计行跳过 / "备注：" 隐藏行跳过。
    #[test]
    fn parse_detail_minimal() {
        let html = r#"
<table><tbody>
<tr><td col="0" row="1" colSpan="9"><div>软件部禅道工时统计明细</div></td></tr>
<tr><td col="0" row="2"><div>部门</div></td></tr>
<tr><td col="0" row="3" colSpan="3"><div>合计：</div></td><td col="3" row="3"><div>12</div></td><td col="4" row="3"><div>12</div></td></tr>
<tr>
  <td rowSpan="2" col="0" row="4"><div>开发组</div></td>
  <td rowSpan="2" col="1" row="4"><div>丁佳斌</div></td>
  <td rowSpan="2" col="2" row="4"><div>2026-05-28</div></td>
  <td rowSpan="2" col="3" row="4"><div>8</div></td>
  <td col="4" row="4"><div>4</div></td>
  <td col="5" row="4"><div>项目A</div></td>
  <td col="6" row="4"><div>系统A</div></td>
  <td col="7" row="4"><div>任务A</div></td>
  <td col="8" row="4"><div>内容A</div></td>
</tr>
<tr>
  <td col="4" row="5"><div>4</div></td>
  <td col="5" row="5"><div>项目A</div></td>
  <td col="6" row="5"><div>系统A</div></td>
  <td col="7" row="5"><div>任务B</div></td>
  <td col="8" row="5"><div>内容B</div></td>
</tr>
<tr>
  <td col="0" row="6"><div>运维组</div></td>
  <td col="1" row="6"><div>吕巧艳</div></td>
  <td col="2" row="6"><div>2026-05-28</div></td>
  <td col="3" row="6"><div>4</div></td>
  <td col="4" row="6"><div>4</div></td>
  <td col="5" row="6"><div>项目B</div></td>
  <td col="6" row="6"><div>系统B</div></td>
  <td col="7" row="6"><div>任务C</div></td>
  <td col="8" row="6"><div>跟踪进度&amp;问题</div></td>
</tr>
<tr><td col="0" row="7"><div>备注：</div></td></tr>
</tbody></table>
"#;
        let records = parse_detail_html(html).expect("parse ok");
        assert_eq!(records.len(), 3, "expect 3 records");

        assert_eq!(records[0].employee, "丁佳斌");
        assert_eq!(records[0].department, "开发组");
        assert_eq!(records[0].daily_total_hours, 8.0);
        assert_eq!(records[0].item_hours, 4.0);
        assert_eq!(records[0].task_name, "任务A");

        // rowSpan 回填：第二条应仍带丁佳斌/开发组/2026-05-28/8h
        assert_eq!(records[1].employee, "丁佳斌");
        assert_eq!(records[1].department, "开发组");
        assert_eq!(records[1].daily_total_hours, 8.0);
        assert_eq!(records[1].task_name, "任务B");

        assert_eq!(records[2].employee, "吕巧艳");
        assert_eq!(records[2].department, "运维组");
        // HTML 实体解码
        assert_eq!(records[2].work_content, "跟踪进度&问题");
    }

    #[test]
    fn decode_entities_chinese_and_ascii() {
        assert_eq!(decode_html_entities("&#37096;&#38376;"), "部门");
        assert_eq!(decode_html_entities("a&amp;b"), "a&b");
        assert_eq!(decode_html_entities("&#x8F6F;&#x4EF6;"), "软件");
        assert_eq!(decode_html_entities("plain"), "plain");
    }

    #[test]
    fn html_to_plain_strips_rich() {
        assert_eq!(
            html_to_plain_text(r#"<p><span style="font-size: 13px;">完成页面改造</span></p>"#),
            "完成页面改造"
        );
        assert_eq!(
            html_to_plain_text(r#"新增自提运价卡控标准\r\n，甩挂记录"#),
            "新增自提运价卡控标准，甩挂记录"
        );
        assert_eq!(html_to_plain_text("纯文本内容"), "纯文本内容");
        assert_eq!(html_to_plain_text(""), "");
    }
}
