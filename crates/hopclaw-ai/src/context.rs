use hopclaw_types::{LayoutReport, SelectionTarget, TableCoordinateMeta};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};
use std::sync::OnceLock;

use crate::traits::default_format_layout_report_for_prompt;
use crate::types::NeutralMessage;

pub const DOC_MARKDOWN_THRESHOLD: usize = 12_000;
pub const TOOL_RESULT_MAX_LENGTH: usize = 4_000;
pub const COMPACT_KEEP_RECENT_TURNS: usize = 3;

/// Truncate a tool result string if it exceeds `max_length` (default 4000 chars).
/// Appends `\n...(생략 ${omitted}자)...` when truncated.
pub fn truncate_tool_result(text: &str, max_length: usize) -> String {
    let char_len = text.chars().count();
    if char_len <= max_length {
        return text.to_string();
    }
    let omitted = char_len - max_length;
    let prefix: String = text.chars().take(max_length).collect();
    format!("{prefix}\n...(생략 {omitted}자)...")
}

/// Summarize a single tool result into a 1-line string.
pub fn summarize_tool_content(content: &str) -> String {
    if content.starts_with("[이전 도구 실행 결과 축약]") {
        return content.to_string();
    }
    if let Ok(parsed) = serde_json::from_str::<Value>(content) {
        if let Some(obj) = parsed.as_object() {
            if let Some(summary) = obj.get("summary").and_then(|v| v.as_str()) {
                return format!("[이전 도구 실행 결과 축약] 요약: {summary}");
            }
            if let Some(err) = obj.get("error").and_then(|v| v.as_str()) {
                return format!("[이전 도구 실행 결과 축약] 오류: {err}");
            }
            if let Some(ok) = obj.get("ok").and_then(|v| v.as_bool()) {
                let detail = serde_json::to_string(&parsed).unwrap_or_default();
                let snippet: String = detail.chars().take(60).collect();
                return format!("[이전 도구 실행 결과 축약] ok: {ok}, {snippet}");
            }
        }
    }

    let first_line = content
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    let snippet = if first_line.chars().count() > 80 {
        let head: String = first_line.chars().take(80).collect();
        format!("{head}...")
    } else {
        first_line.to_string()
    };
    format!("[이전 도구 실행 결과 축약] {snippet}")
}

/// Compact old tool results to a 1-line summary while preserving message structure
/// and `tool_call_id` for protocol compliance. Keeps the most recent `keep_recent_turns`
/// tool interaction turns uncompacted (default 3).
pub fn compact_old_tool_results(
    messages: &[NeutralMessage],
    keep_recent_turns: usize,
) -> Vec<NeutralMessage> {
    let mut assistant_tool_indices = Vec::new();
    for (i, msg) in messages.iter().enumerate() {
        if let NeutralMessage::Assistant {
            tool_calls: Some(calls),
            ..
        } = msg
        {
            if !calls.is_empty() {
                assistant_tool_indices.push(i);
            }
        }
    }

    if assistant_tool_indices.len() <= keep_recent_turns {
        return messages.to_vec();
    }

    let cut_off_assistant_index =
        assistant_tool_indices[assistant_tool_indices.len() - keep_recent_turns];

    let mut recent_tool_call_ids = HashSet::new();
    for msg in &messages[cut_off_assistant_index..] {
        if let NeutralMessage::Assistant {
            tool_calls: Some(calls),
            ..
        } = msg
        {
            for tc in calls {
                recent_tool_call_ids.insert(tc.id.as_str());
            }
        }
    }

    messages
        .iter()
        .map(|msg| match msg {
            NeutralMessage::Tool {
                tool_call_id,
                name,
                content,
            } if !recent_tool_call_ids.contains(tool_call_id.as_str()) => NeutralMessage::Tool {
                tool_call_id: tool_call_id.clone(),
                name: name.clone(),
                content: summarize_tool_content(content),
            },
            other => other.clone(),
        })
        .collect()
}

fn format_number_with_commas(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (idx, ch) in s.chars().rev().enumerate() {
        if idx > 0 && idx % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out.chars().rev().collect()
}

/// Generate a document outline (headings and table summaries) when `doc_markdown`
/// exceeds `DOC_MARKDOWN_THRESHOLD` (12,000 chars).
pub fn generate_document_outline(
    doc_markdown: &str,
    tables_meta: &[TableCoordinateMeta],
) -> String {
    static HEADING_RE: OnceLock<Regex> = OnceLock::new();
    static PARA_SPLIT_RE: OnceLock<Regex> = OnceLock::new();
    let heading_re = HEADING_RE.get_or_init(|| Regex::new(r"^#{1,6}\s+").unwrap());
    let para_split_re = PARA_SPLIT_RE.get_or_init(|| Regex::new(r"\n{2,}").unwrap());

    let mut heading_lines = Vec::new();
    for (idx, raw_line) in doc_markdown.lines().enumerate() {
        let line = raw_line.trim();
        if heading_re.is_match(line) {
            heading_lines.push(format!("(문단 {}) {line}", idx + 1));
        }
    }

    let mut sections = Vec::new();
    sections.push("### [문서 개요 - 주요 제목 구조]".to_string());
    if !heading_lines.is_empty() {
        sections.push(
            heading_lines
                .into_iter()
                .take(50)
                .collect::<Vec<_>>()
                .join("\n"),
        );
    } else {
        sections.push("(제목 마크다운 서식이 없습니다. 본문 첫 3문단 참고)".to_string());
        let paras: Vec<&str> = para_split_re
            .split(doc_markdown)
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .collect();
        let preview = paras
            .into_iter()
            .take(3)
            .enumerate()
            .map(|(i, p)| {
                let head: String = p.chars().take(100).collect();
                format!("[문단 {}]: {head}...", i + 1)
            })
            .collect::<Vec<_>>()
            .join("\n");
        sections.push(preview);
    }

    if !tables_meta.is_empty() {
        sections.push("### [문서 내 표 구조 요약]".to_string());
        let tbl_summaries: Vec<String> = tables_meta
            .iter()
            .map(|t| {
                let header_row: Vec<String> = t
                    .cells
                    .iter()
                    .filter(|c| c.row == 0)
                    .map(|c| c.text.replace('\n', " ").trim().to_string())
                    .take(8)
                    .collect();
                let header_preview = header_row.join(" | ");
                let suffix = if header_preview.is_empty() {
                    String::new()
                } else {
                    format!(" (헤더: {header_preview})")
                };
                format!(
                    "- 표 {}: {}행 x {}열{suffix}",
                    t.table_index, t.row_count, t.col_count
                )
            })
            .collect();
        sections.push(tbl_summaries.join("\n"));
    }

    let len_str = format_number_with_commas(doc_markdown.chars().count());
    sections.push(format!(
        "\n[안내: 문서 본문이 12,000자를 초과(총 {len_str}자)하여 제목/표 개요 및 선택영역만 표시되었습니다.\n상세 문단이나 표 내용은 `read_document` 도구를 사용하여 필요한 범위를 조회하십시오.]"
    ));

    sections.join("\n\n")
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadDocumentArgs {
    pub kind: String,
    pub start: Option<usize>,
    pub count: Option<usize>,
    pub table_index: Option<usize>,
}

/// Read parts of the document on-demand (paragraphs or table cells).
pub fn execute_read_document(
    args: &ReadDocumentArgs,
    doc_markdown: &str,
    tables_meta: &[TableCoordinateMeta],
) -> String {
    static PARA_SPLIT_RE: OnceLock<Regex> = OnceLock::new();
    let para_split_re = PARA_SPLIT_RE.get_or_init(|| Regex::new(r"\n{2,}").unwrap());

    if args.kind == "paragraphs" {
        let paras: Vec<&str> = para_split_re
            .split(doc_markdown)
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .collect();
        let total_paragraphs = paras.len();
        let start = args.start.unwrap_or(0);
        let count = args.count.unwrap_or(5).clamp(1, 30);

        if start >= total_paragraphs {
            return json!({
                "ok": false,
                "error": format!("문단 범위 초과: start={start}, 전체 문단 수={total_paragraphs}"),
                "totalParagraphs": total_paragraphs,
            })
            .to_string();
        }

        let end = (start + count).min(total_paragraphs);
        let sliced: Vec<Value> = paras[start..end]
            .iter()
            .enumerate()
            .map(|(idx, text)| json!({ "index": start + idx, "text": text }))
            .collect();

        return json!({
            "ok": true,
            "kind": "paragraphs",
            "start": start,
            "count": sliced.len(),
            "totalParagraphs": total_paragraphs,
            "paragraphs": sliced,
        })
        .to_string();
    }

    if args.kind == "table" {
        let table_index = args.table_index.unwrap_or(0);
        let Some(table) = tables_meta.iter().find(|t| t.table_index == table_index) else {
            let available: Vec<Value> = tables_meta
                .iter()
                .map(|t| {
                    json!({
                        "tableIndex": t.table_index,
                        "rowCount": t.row_count,
                        "colCount": t.col_count,
                    })
                })
                .collect();
            return json!({
                "ok": false,
                "error": format!("표 {table_index}를 찾을 수 없습니다. (전체 표 개수: {})", tables_meta.len()),
                "availableTables": available,
            })
            .to_string();
        };

        let start_row = args.start.unwrap_or(0);
        let count = args.count.unwrap_or(5).clamp(1, 20);
        let end_row = table
            .row_count
            .saturating_sub(1)
            .min(start_row + count - 1);

        let mut rows_map: BTreeMap<usize, Vec<(usize, Value)>> = BTreeMap::new();
        for cell in &table.cells {
            if cell.row >= start_row && cell.row <= end_row {
                rows_map.entry(cell.row).or_default().push((
                    cell.col,
                    json!({
                        "col": cell.col,
                        "text": cell.text,
                        "rowSpan": cell.row_span,
                        "colSpan": cell.col_span,
                    }),
                ));
            }
        }

        let rows_data: Vec<Value> = rows_map
            .into_iter()
            .map(|(row, mut cells)| {
                cells.sort_by_key(|(col, _)| *col);
                let cell_vals: Vec<Value> = cells.into_iter().map(|(_, v)| v).collect();
                json!({ "row": row, "cells": cell_vals })
            })
            .collect();

        return json!({
            "ok": true,
            "kind": "table",
            "tableIndex": table_index,
            "rowCount": table.row_count,
            "colCount": table.col_count,
            "startRow": start_row,
            "endRow": end_row,
            "rows": rows_data,
        })
        .to_string();
    }

    json!({
        "ok": false,
        "error": format!("지원하지 않는 kind: {}", args.kind),
    })
    .to_string()
}

pub struct BuildDocumentContextOptions<'a> {
    pub doc_markdown: Option<&'a str>,
    pub tables_meta: &'a [TableCoordinateMeta],
    pub selection: Option<&'a SelectionTarget>,
    pub initial_report: Option<&'a LayoutReport>,
    pub attachments_context: Option<&'a str>,
    pub memory_context: Option<&'a str>,
    pub user_prompt: Option<&'a str>,
}

/// Build the full context prompt for multi-turn agent loop.
pub fn build_document_context(opts: &BuildDocumentContextOptions<'_>) -> String {
    let mut parts = Vec::new();
    let tables_meta = opts.tables_meta;

    let tables_summary = tables_meta
        .iter()
        .map(|t| {
            let cells_preview = t
                .cells
                .iter()
                .take(30)
                .map(|c| format!("[{},{}]=\"{}\"", c.row, c.col, c.text.replace('\n', " ")))
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "- 표 {} ({}행 x {}열): {cells_preview}",
                t.table_index, t.row_count, t.col_count
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let summary_suffix = if tables_summary.is_empty() {
        String::new()
    } else {
        format!("\n{tables_summary}")
    };
    parts.push(format!(
        "### [현재 문서 구조 요약]\n표 개수: {}{summary_suffix}",
        tables_meta.len()
    ));

    if let Some(report) = opts.initial_report {
        parts.push(format!(
            "### [문서 레이아웃 실측 진단 보고서 (WASM Inspector)]\n{}",
            default_format_layout_report_for_prompt(report)
        ));
    }

    if let Some(sel) = opts.selection {
        match sel {
            SelectionTarget::TableCells {
                table_index,
                start_row,
                start_col,
                end_row,
                end_col,
                selected_cells,
                ..
            } => {
                let cell_lines = selected_cells
                    .iter()
                    .map(|c| format!("- [{}행 {}열]: \"{}\"", c.row, c.col, c.text))
                    .collect::<Vec<_>>()
                    .join("\n");
                parts.push(format!(
                    "### [사용자가 마우스로 블록 지정한 표 셀 영역]\n표 인덱스: {table_index} ({}~{}행, {}~{}열)\n선택된 셀 데이터:\n{cell_lines}",
                    start_row + 1,
                    end_row + 1,
                    start_col + 1,
                    end_col + 1
                ));
            }
            SelectionTarget::BodyRange {
                start_para,
                end_para,
                selected_text,
                ..
            } => {
                parts.push(format!(
                    "### [사용자가 마우스로 드래그 선택한 본문 텍스트]\n문단 범위: {} ~ {}\n내용:\n{selected_text}",
                    start_para + 1,
                    end_para + 1
                ));
            }
        }
    }

    if let Some(md) = opts.doc_markdown.filter(|s| !s.trim().is_empty()) {
        if md.chars().count() > DOC_MARKDOWN_THRESHOLD {
            parts.push(generate_document_outline(md, tables_meta));
        } else {
            parts.push(format!("### [문서 전체 마크다운 내용]\n{md}"));
        }
    } else {
        parts.push("### [문서 상태 안내]\n현재 문서는 빈 문서(또는 새로 생성된 문서)입니다. 본문과 표가 비어 있으므로 사용자의 요청에 따라 append_paragraph, append_heading, insert_table 등을 사용하여 문서를 작성하십시오.".to_string());
    }

    if let Some(att) = opts.attachments_context.filter(|s| !s.is_empty()) {
        parts.push(format!(
            "### [사용자가 첨부한 로컬 참고자료 (Excel / PDF / HWP 등)]\n{att}"
        ));
    }

    if let Some(mem) = opts.memory_context.map(str::trim).filter(|s| !s.is_empty()) {
        parts.push(mem.to_string());
    }

    if let Some(prompt) = opts.user_prompt.filter(|s| !s.is_empty()) {
        parts.push(format!("### [사용자 지시사항]\n{prompt}"));
    }

    parts.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::NeutralToolCall;
    use hopclaw_types::TableCellMeta;

    #[test]
    fn truncate_tool_result_preserves_short_and_truncates_long() {
        assert_eq!(truncate_tool_result("짧은 도구 결과", 100), "짧은 도구 결과");

        let long_text = "A".repeat(5000);
        let truncated = truncate_tool_result(&long_text, 4000);
        assert!(truncated.chars().count() < long_text.chars().count());
        assert!(truncated.starts_with(&"A".repeat(4000)));
        assert!(truncated.contains("(생략 1000자)"));
    }

    #[test]
    fn summarize_tool_content_formats_json_and_text() {
        let summary_json =
            json!({ "summary": "표 1 레이아웃 최적화 완료", "resolvedCount": 3 }).to_string();
        assert_eq!(
            summarize_tool_content(&summary_json),
            "[이전 도구 실행 결과 축약] 요약: 표 1 레이아웃 최적화 완료"
        );

        let err_json = json!({ "error": "Table 99 not found" }).to_string();
        assert_eq!(
            summarize_tool_content(&err_json),
            "[이전 도구 실행 결과 축약] 오류: Table 99 not found"
        );

        let text = "첫 번째 줄의 중요한 내용입니다.\n두 번째 줄 세부사항\n세 번째 줄";
        let summarized = summarize_tool_content(text);
        assert!(summarized.contains("[이전 도구 실행 결과 축약]"));
        assert!(summarized.contains("첫 번째 줄의 중요한 내용입니다."));
        assert!(!summarized.contains('\n'));
    }

    #[test]
    fn compact_old_tool_results_keeps_recent_turns_intact() {
        let short = vec![
            NeutralMessage::User {
                content: "질문".into(),
            },
            NeutralMessage::Assistant {
                content: None,
                tool_calls: Some(vec![NeutralToolCall {
                    id: "call_1".into(),
                    name: "inspect_layout".into(),
                    arguments_json: "{}".into(),
                }]),
            },
            NeutralMessage::Tool {
                tool_call_id: "call_1".into(),
                name: None,
                content: "상세 결과 1".into(),
            },
            NeutralMessage::Assistant {
                content: None,
                tool_calls: Some(vec![NeutralToolCall {
                    id: "call_2".into(),
                    name: "inspect_layout".into(),
                    arguments_json: "{}".into(),
                }]),
            },
            NeutralMessage::Tool {
                tool_call_id: "call_2".into(),
                name: None,
                content: "상세 결과 2".into(),
            },
        ];
        assert_eq!(compact_old_tool_results(&short, 3), short);

        let four_turns = vec![
            NeutralMessage::Assistant {
                content: None,
                tool_calls: Some(vec![NeutralToolCall {
                    id: "call_1".into(),
                    name: "inspect_layout".into(),
                    arguments_json: "{}".into(),
                }]),
            },
            NeutralMessage::Tool {
                tool_call_id: "call_1".into(),
                name: None,
                content: "오래된 긴 상세 결과 1\n여러 줄의 내용\n...".into(),
            },
            NeutralMessage::Assistant {
                content: None,
                tool_calls: Some(vec![NeutralToolCall {
                    id: "call_2".into(),
                    name: "inspect_layout".into(),
                    arguments_json: "{}".into(),
                }]),
            },
            NeutralMessage::Tool {
                tool_call_id: "call_2".into(),
                name: None,
                content: "오래된 긴 상세 결과 2\n여러 줄의 내용\n...".into(),
            },
            NeutralMessage::Assistant {
                content: None,
                tool_calls: Some(vec![NeutralToolCall {
                    id: "call_3".into(),
                    name: "inspect_layout".into(),
                    arguments_json: "{}".into(),
                }]),
            },
            NeutralMessage::Tool {
                tool_call_id: "call_3".into(),
                name: None,
                content: "최근 결과 3".into(),
            },
            NeutralMessage::Assistant {
                content: None,
                tool_calls: Some(vec![NeutralToolCall {
                    id: "call_4".into(),
                    name: "inspect_layout".into(),
                    arguments_json: "{}".into(),
                }]),
            },
            NeutralMessage::Tool {
                tool_call_id: "call_4".into(),
                name: None,
                content: "최근 결과 4".into(),
            },
        ];

        let compacted = compact_old_tool_results(&four_turns, 2);
        match &compacted[1] {
            NeutralMessage::Tool {
                tool_call_id,
                content,
                ..
            } => {
                assert_eq!(tool_call_id, "call_1");
                assert!(content.contains("[이전 도구 실행 결과 축약]"));
            }
            _ => panic!("expected tool message"),
        }
        match &compacted[3] {
            NeutralMessage::Tool {
                tool_call_id,
                content,
                ..
            } => {
                assert_eq!(tool_call_id, "call_2");
                assert!(content.contains("[이전 도구 실행 결과 축약]"));
            }
            _ => panic!("expected tool message"),
        }
        match &compacted[5] {
            NeutralMessage::Tool {
                tool_call_id,
                content,
                ..
            } => {
                assert_eq!(tool_call_id, "call_3");
                assert_eq!(content, "최근 결과 3");
            }
            _ => panic!("expected tool message"),
        }
        match &compacted[7] {
            NeutralMessage::Tool {
                tool_call_id,
                content,
                ..
            } => {
                assert_eq!(tool_call_id, "call_4");
                assert_eq!(content, "최근 결과 4");
            }
            _ => panic!("expected tool message"),
        }
    }

    #[test]
    fn execute_read_document_paragraphs_and_tables() {
        let md = [
            "# 첫 번째 섹션",
            "첫 번째 문단입니다. 내용이 들어갑니다.",
            "두 번째 문단입니다. 자세한 설명이 있습니다.",
            "세 번째 문단입니다. 결론 부분입니다.",
        ]
        .join("\n\n");

        let tables = vec![TableCoordinateMeta {
            table_index: 0,
            sec: 0,
            para: 1,
            ctrl: 0,
            row_count: 2,
            col_count: 2,
            parent_table_index: None,
            parent_cell: None,
            depth: None,
            cells: vec![
                TableCellMeta {
                    idx: 0,
                    row: 0,
                    col: 0,
                    row_span: 1,
                    col_span: 1,
                    text: "항목".into(),
                    bbox: None,
                },
                TableCellMeta {
                    idx: 1,
                    row: 0,
                    col: 1,
                    row_span: 1,
                    col_span: 1,
                    text: "금액".into(),
                    bbox: None,
                },
                TableCellMeta {
                    idx: 2,
                    row: 1,
                    col: 0,
                    row_span: 1,
                    col_span: 1,
                    text: "인건비".into(),
                    bbox: None,
                },
                TableCellMeta {
                    idx: 3,
                    row: 1,
                    col: 1,
                    row_span: 1,
                    col_span: 1,
                    text: "10,000".into(),
                    bbox: None,
                },
            ],
        }];

        let p_res: Value = serde_json::from_str(&execute_read_document(
            &ReadDocumentArgs {
                kind: "paragraphs".into(),
                start: Some(1),
                count: Some(2),
                table_index: None,
            },
            &md,
            &[],
        ))
        .unwrap();
        assert_eq!(p_res["ok"], true);
        assert_eq!(p_res["paragraphs"].as_array().unwrap().len(), 2);
        assert!(p_res["paragraphs"][0]["text"]
            .as_str()
            .unwrap()
            .contains("첫 번째 문단"));

        let oob_res: Value = serde_json::from_str(&execute_read_document(
            &ReadDocumentArgs {
                kind: "paragraphs".into(),
                start: Some(10),
                count: Some(2),
                table_index: None,
            },
            &md,
            &[],
        ))
        .unwrap();
        assert_eq!(oob_res["ok"], false);
        assert!(oob_res["error"].as_str().unwrap().contains("문단 범위 초과"));

        let t_res: Value = serde_json::from_str(&execute_read_document(
            &ReadDocumentArgs {
                kind: "table".into(),
                start: Some(0),
                count: Some(2),
                table_index: Some(0),
            },
            &md,
            &tables,
        ))
        .unwrap();
        assert_eq!(t_res["ok"], true);
        assert_eq!(t_res["rows"][0]["cells"][0]["text"], "항목");
        assert_eq!(t_res["rows"][1]["cells"][1]["text"], "10,000");

        let missing_t: Value = serde_json::from_str(&execute_read_document(
            &ReadDocumentArgs {
                kind: "table".into(),
                start: None,
                count: None,
                table_index: Some(99),
            },
            &md,
            &tables,
        ))
        .unwrap();
        assert_eq!(missing_t["ok"], false);
        assert!(missing_t["error"]
            .as_str()
            .unwrap()
            .contains("표 99를 찾을 수 없습니다"));
    }

    #[test]
    fn build_document_context_handles_12000_char_threshold() {
        let short_md = "# 보고서\n\n짧은 내용입니다.";
        let ctx_short = build_document_context(&BuildDocumentContextOptions {
            doc_markdown: Some(short_md),
            tables_meta: &[],
            selection: None,
            initial_report: None,
            attachments_context: None,
            memory_context: None,
            user_prompt: Some("수정해줘"),
        });
        assert!(ctx_short.contains("### [문서 전체 마크다운 내용]"));
        assert!(ctx_short.contains(short_md));
        assert!(!ctx_short.contains("12,000자를 초과"));

        let long_md = format!("# 대형 보고서\n\n{}", "내용 ".repeat(5000));
        assert!(long_md.chars().count() > DOC_MARKDOWN_THRESHOLD);
        let ctx_long = build_document_context(&BuildDocumentContextOptions {
            doc_markdown: Some(&long_md),
            tables_meta: &[],
            selection: None,
            initial_report: None,
            attachments_context: None,
            memory_context: None,
            user_prompt: Some("수정해줘"),
        });
        assert!(!ctx_long.contains("### [문서 전체 마크다운 내용]"));
        assert!(ctx_long.contains("### [문서 개요 - 주요 제목 구조]"));
        assert!(ctx_long.contains("read_document"));
        assert!(ctx_long.contains("12,000자를 초과"));
    }
}
