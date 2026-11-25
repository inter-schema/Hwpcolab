//! Pure-Rust layout inspection and comparison math ported from `src/core/layoutInspector.ts`.

use std::collections::{HashMap, HashSet};

use crate::types::{
    CellBBox, CellLayoutInfo, CellProps, LayoutComparison, LayoutInspectorOptions, LayoutIssue,
    LayoutIssueCellRef, LayoutIssueCode, LayoutIssueMetrics, LayoutIssueSeverity,
    LayoutMetricsDelta, LayoutReport, PageDef, TableCoordinateMeta, TableLayoutInfo,
};

pub const HU_PER_PX: f64 = 75.0;
pub const HU_PER_MM: f64 = 283.465;
pub const PX_PER_MM: f64 = HU_PER_MM / HU_PER_PX;

/// Abstract layout data source decoupled from `rhwp` WASM / native bindings.
pub trait DocumentLayoutSource {
    fn tables_meta(&self) -> &[TableCoordinateMeta];
    fn page_def(&self, sec: usize) -> PageDef;
    fn cell_properties(&self, sec: usize, para: usize, ctrl: usize, cell_idx: usize) -> CellProps;
    fn cell_paragraphs(&self, sec: usize, para: usize, ctrl: usize, cell_idx: usize) -> Vec<String>;
}

/// Measure CJK + Latin text width in points, matching `measureKoreanTextPt` in `layoutInspector.ts`.
pub fn measure_korean_text_pt(text: &str, font_pt: f64) -> f64 {
    let mut pt_width = 0.0_f64;
    for ch in text.chars() {
        let code = ch as u32;
        if (0xac00..=0xd7a3).contains(&code) || (0x3000..=0x303f).contains(&code) {
            pt_width += font_pt * 1.0;
        } else if ch == ' ' || ch == '\t' {
            pt_width += font_pt * 0.4;
        } else if ch.is_ascii_uppercase() {
            pt_width += font_pt * 0.65;
        } else if ch.is_ascii_lowercase() || ch.is_ascii_digit() {
            pt_width += font_pt * 0.55;
        } else {
            pt_width += font_pt * 0.5;
        }
    }
    pt_width
}

#[inline]
fn round2(val: f64) -> f64 {
    (val * 100.0).round() / 100.0
}

fn sanitize_full_text(parts: &[String]) -> String {
    let joined = parts.join("\n");
    let mut out = String::with_capacity(joined.len());
    let mut prev_ctrl = false;
    for ch in joined.chars() {
        let c = ch as u32;
        if ch == '\r' || c <= 0x1f {
            if !prev_ctrl {
                out.push(' ');
                prev_ctrl = true;
            }
        } else {
            out.push(ch);
            prev_ctrl = false;
        }
    }
    out.trim().to_string()
}

/// Inspect table/cell layout from any `DocumentLayoutSource` and produce a `LayoutReport`.
pub fn inspect_layout_from_source(
    source: &dyn DocumentLayoutSource,
    options: LayoutInspectorOptions,
    timestamp: u64,
) -> LayoutReport {
    let safety_h = options.safety_margin_horizontal.unwrap_or(0.05);
    let safety_v = options.safety_margin_vertical.unwrap_or(0.08);
    let font_pt = options.default_font_pt.unwrap_or(10.0);
    let line_spacing_ratio = options.line_spacing_ratio.unwrap_or(1.10);

    let font_px = font_pt * (96.0 / 72.0);
    let line_advance_px = font_px * line_spacing_ratio;

    let tables_meta = source.tables_meta();
    let mut tables_report: Vec<TableLayoutInfo> = Vec::with_capacity(tables_meta.len());
    let mut issues: Vec<LayoutIssue> = Vec::new();

    for table_meta in tables_meta {
        let sec = table_meta.sec;
        let para = table_meta.para;
        let ctrl = table_meta.ctrl;
        let t_idx = table_meta.table_index;

        let page_def = source.page_def(sec);
        let printable_width_mm = round2(
            (page_def.width - page_def.margin_left - page_def.margin_right) as f64 / HU_PER_MM,
        );

        let mut col_max_x: HashMap<usize, f64> = HashMap::new();
        for c in &table_meta.cells {
            if c.col_span == 1 {
                if let Some(bb) = &c.bbox {
                    let entry = col_max_x.entry(c.col).or_insert(0.0);
                    if bb.w > *entry {
                        *entry = bb.w;
                    }
                }
            }
        }
        let table_width_px: f64 = col_max_x.values().sum();
        let table_width_mm = round2(table_width_px / PX_PER_MM);

        let is_nested = table_meta.depth.unwrap_or(0) > 0;
        let mut allowed_width_mm: Option<f64> = None;
        let mut is_parent_cell_limit = false;

        if is_nested {
            if let (Some(parent_idx), Some(parent_coord)) =
                (table_meta.parent_table_index, table_meta.parent_cell)
            {
                if let Some(parent_table) =
                    tables_meta.iter().find(|t| t.table_index == parent_idx)
                {
                    if let Some(parent_cell) = parent_table
                        .cells
                        .iter()
                        .find(|c| c.row == parent_coord.row && c.col == parent_coord.col)
                    {
                        if let Some(bb) = &parent_cell.bbox {
                            if bb.w > 0.0 {
                                let p_props = source.cell_properties(
                                    parent_table.sec,
                                    parent_table.para,
                                    parent_table.ctrl,
                                    parent_cell.idx,
                                );
                                let pad_l_px = p_props.padding_left as f64 / HU_PER_PX;
                                let pad_r_px = p_props.padding_right as f64 / HU_PER_PX;
                                let avail_w_px = (bb.w - pad_l_px - pad_r_px).max(0.0);
                                allowed_width_mm = Some(round2(avail_w_px / PX_PER_MM));
                                is_parent_cell_limit = true;
                            }
                        }
                    }
                }
            }
        } else {
            allowed_width_mm = Some(printable_width_mm);
        }

        if let Some(allowed) = allowed_width_mm {
            if table_width_mm > allowed + 1.0 {
                let overflow_mm = round2(table_width_mm - allowed);
                let target_desc = if is_parent_cell_limit {
                    format!("부모 셀 내부 폭({allowed}mm)")
                } else {
                    format!("인쇄 가능 폭({allowed}mm)")
                };
                issues.push(LayoutIssue {
                    id: format!("table:{t_idx}:exceeds_page"),
                    code: LayoutIssueCode::TABLE_EXCEEDS_PAGE,
                    severity: LayoutIssueSeverity::Warning,
                    message: format!(
                        "표 {t_idx}의 너비({table_width_mm}mm)가 {target_desc}을 {overflow_mm}mm 초과했습니다."
                    ),
                    cell: None,
                    table_index: Some(t_idx),
                    metrics: LayoutIssueMetrics {
                        table_width_mm: Some(table_width_mm),
                        printable_width_mm: Some(allowed),
                        overflow_width_mm: Some(overflow_mm),
                        ..Default::default()
                    },
                    text: None,
                });
            }
        }

        let mut cells_info: Vec<CellLayoutInfo> = Vec::with_capacity(table_meta.cells.len());

        for cell in &table_meta.cells {
            let cell_idx = cell.idx;
            let row = cell.row;
            let col = cell.col;
            let row_span = cell.row_span;
            let col_span = cell.col_span;

            let props = source.cell_properties(sec, para, ctrl, cell_idx);
            let pad_l_px = props.padding_left as f64 / HU_PER_PX;
            let pad_r_px = props.padding_right as f64 / HU_PER_PX;
            let pad_t_px = props.padding_top as f64 / HU_PER_PX;
            let pad_b_px = props.padding_bottom as f64 / HU_PER_PX;

            let bb = cell.bbox.clone().unwrap_or(CellBBox {
                cell_idx,
                row,
                col,
                row_span,
                col_span,
                page_index: 0,
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 24.0,
            });
            let width_px = bb.w;
            let height_px = bb.h;
            let width_mm = round2(width_px / PX_PER_MM);
            let height_mm = round2(height_px / PX_PER_MM);

            let avail_width_px = (width_px - pad_l_px - pad_r_px).max(0.0);
            let avail_height_px = (height_px - pad_t_px - pad_b_px).max(0.0);

            let mut para_texts = source.cell_paragraphs(sec, para, ctrl, cell_idx);
            if para_texts.is_empty() && !cell.text.is_empty() {
                para_texts = cell.text.split('\n').map(|s| s.to_string()).collect();
            }
            let full_text = sanitize_full_text(&para_texts);

            let effective_width_px = avail_width_px * (1.0 - safety_h);
            let mut total_estimated_lines = 0usize;
            let mut max_line_width_px = 0.0_f64;

            for p_text in &para_texts {
                if p_text.is_empty() {
                    total_estimated_lines += 1;
                    continue;
                }
                let text_pt = measure_korean_text_pt(p_text, font_pt);
                let text_px = text_pt * (96.0 / 72.0);
                if text_px > max_line_width_px {
                    max_line_width_px = text_px;
                }
                let lines = if effective_width_px > 0.0 {
                    (text_px / effective_width_px).ceil().max(1.0) as usize
                } else {
                    1
                };
                total_estimated_lines += lines;
            }

            let pad_y_px = pad_t_px + pad_b_px;
            let content_height_px = if total_estimated_lines == 0 {
                0.0
            } else {
                font_px + (total_estimated_lines as f64 - 1.0) * line_advance_px + pad_y_px
            };
            let content_height_mm = round2(content_height_px / PX_PER_MM);

            let threshold_px = (height_px * safety_v).max(6.0);
            let overflow_diff_px = content_height_px - height_px;
            let is_vertical_overflow = overflow_diff_px > threshold_px;

            let is_horizontal_overflow =
                (max_line_width_px - avail_width_px) > (avail_width_px * safety_h + 4.0);
            let overflow_amount_mm = round2((content_height_mm - height_mm).max(0.0));

            cells_info.push(CellLayoutInfo {
                cell_idx,
                row,
                col,
                row_span,
                col_span,
                width_mm,
                height_mm,
                avail_width_mm: round2(avail_width_px / PX_PER_MM),
                avail_height_mm: round2(avail_height_px / PX_PER_MM),
                estimated_lines: total_estimated_lines,
                content_height_mm,
                is_vertical_overflow,
                is_horizontal_overflow,
                overflow_amount_mm,
                text: full_text.clone(),
            });

            if is_vertical_overflow {
                let preview: String = full_text.chars().take(40).collect();
                issues.push(LayoutIssue {
                    id: format!("cell:{sec}:{para}:{ctrl}:{cell_idx}:v_overflow"),
                    code: LayoutIssueCode::CELL_OVERFLOW_V,
                    severity: LayoutIssueSeverity::Error,
                    message: format!(
                        "셀 [행 {row}, 열 {col}]: 내용물 높이({content_height_mm}mm)가 셀 높이({height_mm}mm)를 +{overflow_amount_mm}mm 초과했습니다. ({total_estimated_lines}줄 예상)"
                    ),
                    cell: Some(LayoutIssueCellRef {
                        table_index: t_idx,
                        sec: Some(sec),
                        para: Some(para),
                        ctrl: Some(ctrl),
                        row,
                        col,
                        cell_idx,
                    }),
                    table_index: Some(t_idx),
                    metrics: LayoutIssueMetrics {
                        cell_height_mm: Some(height_mm),
                        content_height_mm: Some(content_height_mm),
                        overflow_mm: Some(overflow_amount_mm),
                        lines: Some(total_estimated_lines),
                        ..Default::default()
                    },
                    text: Some(preview),
                });
            }
        }

        tables_report.push(TableLayoutInfo {
            table_index: t_idx,
            section: sec,
            para,
            ctrl,
            cell_count: cells_info.len(),
            table_width_mm: Some(table_width_mm),
            printable_width_mm: Some(allowed_width_mm.unwrap_or(printable_width_mm)),
            cells: cells_info,
        });
    }

    LayoutReport {
        timestamp,
        table_count: tables_report.len(),
        issues_count: issues.len(),
        issues,
        tables: tables_report,
    }
}

/// Format a `LayoutReport` for LLM / review prompt consumption, matching `formatLayoutReportForPrompt`.
pub fn format_layout_report_for_prompt(report: &LayoutReport) -> String {
    if report.issues_count == 0 {
        return format!(
            "[레이아웃 검사 결과: 정상] 표 {}개에 대해 시각적 넘침이나 여백 침범 이슈가 검출되지 않았습니다.",
            report.table_count
        );
    }

    let mut lines = vec![
        format!("[레이아웃 검사 결과: 이슈 {}건 검출]", report.issues_count),
        format!("총 {}개의 표에서 발견된 문제 목록:", report.table_count),
    ];

    for (i, iss) in report.issues.iter().enumerate() {
        let sev = match iss.severity {
            LayoutIssueSeverity::Error => "error",
            LayoutIssueSeverity::Warning => "warning",
        };
        match (&iss.code, &iss.cell) {
            (LayoutIssueCode::CELL_OVERFLOW_V, Some(cell)) => {
                lines.push(format!(
                    "{}. [세로 넘침 / {}] 표 {} [행 {}, 열 {}]: 내용({}mm) > 셀({}mm) (+{}mm 초과, {}줄) | \"{}\"",
                    i + 1,
                    sev,
                    cell.table_index,
                    cell.row,
                    cell.col,
                    iss.metrics.content_height_mm.unwrap_or(0.0),
                    iss.metrics.cell_height_mm.unwrap_or(0.0),
                    iss.metrics.overflow_mm.unwrap_or(0.0),
                    iss.metrics.lines.unwrap_or(0),
                    iss.text.as_deref().unwrap_or("")
                ));
            }
            (LayoutIssueCode::TABLE_EXCEEDS_PAGE, _) => {
                lines.push(format!(
                    "{}. [표 폭 초과 / {}] 표 {}: 표 너비 {}mm > 인쇄 영역 {}mm (+{}mm 여백 침범)",
                    i + 1,
                    sev,
                    iss.table_index.unwrap_or(0),
                    iss.metrics.table_width_mm.unwrap_or(0.0),
                    iss.metrics.printable_width_mm.unwrap_or(0.0),
                    iss.metrics.overflow_width_mm.unwrap_or(0.0),
                ));
            }
            _ => {
                let code_str = match iss.code {
                    LayoutIssueCode::CELL_OVERFLOW_V => "CELL_OVERFLOW_V",
                    LayoutIssueCode::CELL_OVERFLOW_H => "CELL_OVERFLOW_H",
                    LayoutIssueCode::TABLE_EXCEEDS_PAGE => "TABLE_EXCEEDS_PAGE",
                };
                lines.push(format!("{}. [{code_str} / {sev}] {}", i + 1, iss.message));
            }
        }
    }

    lines.join("\n")
}

fn max_overflow_mm(report: &LayoutReport) -> Option<f64> {
    report
        .issues
        .iter()
        .filter_map(|iss| iss.metrics.overflow_mm.or(iss.metrics.overflow_width_mm))
        .reduce(f64::max)
}

/// Compare `before` and `after` layout reports, matching `compareLayoutReports` in `layoutInspector.ts`.
pub fn compare_layout_reports(before: &LayoutReport, after: &LayoutReport) -> LayoutComparison {
    let before_ids: HashSet<&str> = before.issues.iter().map(|i| i.id.as_str()).collect();
    let after_ids: HashSet<&str> = after.issues.iter().map(|i| i.id.as_str()).collect();

    let resolved_issues: Vec<LayoutIssue> = before
        .issues
        .iter()
        .filter(|i| !after_ids.contains(i.id.as_str()))
        .cloned()
        .collect();
    let remaining_issues: Vec<LayoutIssue> = after
        .issues
        .iter()
        .filter(|i| before_ids.contains(i.id.as_str()))
        .cloned()
        .collect();
    let new_issues: Vec<LayoutIssue> = after
        .issues
        .iter()
        .filter(|i| !before_ids.contains(i.id.as_str()))
        .cloned()
        .collect();

    let ok = new_issues.is_empty() && (after.issues_count == 0 || !resolved_issues.is_empty());

    let mut parts: Vec<String> = Vec::new();
    if !resolved_issues.is_empty() {
        parts.push(format!("해결된 이슈: {}건", resolved_issues.len()));
    }
    if !remaining_issues.is_empty() {
        parts.push(format!("잔여 이슈: {}건", remaining_issues.len()));
    }
    if !new_issues.is_empty() {
        parts.push(format!("새로 발생한 이슈: {}건", new_issues.len()));
    }
    if parts.is_empty() {
        parts.push("이슈 없음 (정상)".to_string());
    }

    let metrics_delta = LayoutMetricsDelta {
        before_issues_count: before.issues_count,
        after_issues_count: after.issues_count,
        resolved_count: resolved_issues.len(),
        remaining_count: remaining_issues.len(),
        new_count: new_issues.len(),
        max_overflow_before_mm: max_overflow_mm(before),
        max_overflow_after_mm: max_overflow_mm(after),
    };

    LayoutComparison {
        ok,
        resolved_issues,
        remaining_issues,
        new_issues,
        summary_text: parts.join(", "),
        metrics_delta: Some(metrics_delta),
    }
}
