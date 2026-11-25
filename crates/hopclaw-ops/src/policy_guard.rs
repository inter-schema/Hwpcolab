use std::collections::HashSet;
use std::sync::LazyLock;

use hopclaw_types::{
    ExcludedRegion, ExcludedRegionTargetType, HwpUnit, LayoutReport, ParaIndexTarget,
    ParagraphAlign, TableCoordinateMeta, UnifiedOp,
};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::sanitize::{
    json_number, sanitize_markdown_formatting, sanitize_matrix_string, sanitize_opt_string,
    sanitize_vec_string,
};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicyGuardContext {
    #[serde(default)]
    pub user_prompt: String,
    #[serde(default)]
    pub is_new_document: Option<bool>,
    #[serde(default)]
    pub is_hwpx: Option<bool>,
    #[serde(default)]
    pub excluded_regions: Option<Vec<ExcludedRegion>>,
    #[serde(default)]
    pub layout_report: Option<LayoutReport>,
    #[serde(default)]
    pub tables_meta: Option<Vec<TableCoordinateMeta>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RejectedOp {
    pub op: UnifiedOp,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicyGuardResult {
    pub passed: bool,
    pub rejected_ops: Vec<RejectedOp>,
    pub sanitized_ops: Vec<UnifiedOp>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawRejectedOp {
    pub op: Value,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawPolicyGuardResult {
    pub passed: bool,
    pub rejected_ops: Vec<RawRejectedOp>,
    pub sanitized_ops: Vec<Value>,
    pub warnings: Vec<String>,
}

static RE_NEG_1: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"([가-힣a-zA-Z0-9\s]{1,15}?)(?:\s*셀|\s*칸|\s*행|\s*열)?(?:은|는|도)?\s*(?:변경하지|바꾸지|건드리지|수정하지)\s*마",
    )
    .unwrap()
});

static RE_NEG_2: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"([가-힣a-zA-Z0-9\s]{1,15}?)(?:\s*셀|\s*칸|\s*행|\s*열)?(?:은|는|도)?\s*(?:그대로\s*(?:둬|두라|유지)|제외)",
    )
    .unwrap()
});

static RE_CLIPPING: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)잘려|잘림|안보여|안\s*보여|겹쳐|겹침|넘침|오버플로").unwrap());

static RE_STYLE_REQ: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)스타일|서식|굵게|진하게|크게|크기|폰트|글꼴|볼드|bold|포인트|pt|제목")
        .unwrap()
});

const IGNORED_PRONOUNS: [&str; 5] = ["이", "그", "저", "해당", "모든"];

/// Extracts explicit negative constraints (e.g. `"날짜 셀은 바꾸지 마"`) from the user prompt.
pub fn extract_negative_constraints(prompt: &str) -> Vec<String> {
    if prompt.is_empty() {
        return Vec::new();
    }
    let mut constraints = Vec::new();
    for reg in [&*RE_NEG_1, &*RE_NEG_2] {
        for caps in reg.captures_iter(prompt) {
            let keyword = caps.get(1).map(|m| m.as_str().trim()).unwrap_or("");
            if !keyword.is_empty()
                && !IGNORED_PRONOUNS.contains(&keyword)
                && !constraints.iter().any(|c| c == keyword)
            {
                constraints.push(keyword.to_string());
            }
        }
    }
    constraints
}

fn build_excluded_cell_keys(context: &PolicyGuardContext) -> HashSet<(usize, usize, usize)> {
    context
        .excluded_regions
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .filter(|r| r.target_type == ExcludedRegionTargetType::Cell)
        .filter_map(|r| Some((r.table_index?, r.row?, r.col?)))
        .collect()
}

fn check_g2_excluded_typed(
    op: &UnifiedOp,
    excluded_cells: &HashSet<(usize, usize, usize)>,
) -> Option<String> {
    match op {
        UnifiedOp::SetCellAlign {
            table_index,
            row,
            col,
            end_row,
            end_col,
            ..
        } => {
            let sr = *row;
            let er = end_row.unwrap_or(sr);
            let sc = *col;
            let ec = end_col.unwrap_or(sc);
            for r in sr.min(er)..=sr.max(er) {
                for c in sc.min(ec)..=sc.max(ec) {
                    if excluded_cells.contains(&(*table_index, r, c)) {
                        return Some(format!(
                            "G2 위반: 셀 ({table_index}번 표 [{r}, {c}])은 사용자가 AI 수정 제외로 지정한 영역입니다."
                        ));
                    }
                }
            }
            None
        }
        UnifiedOp::SetCellText {
            table_index,
            row,
            col,
            ..
        }
        | UnifiedOp::InsertNestedTable {
            table_index,
            row,
            col,
            ..
        }
        | UnifiedOp::UnmergeCells {
            table_index,
            row,
            col,
        }
        | UnifiedOp::SplitCell {
            table_index,
            row,
            col,
            ..
        }
        | UnifiedOp::SetCellBackground {
            table_index,
            row,
            col,
            ..
        }
        | UnifiedOp::SetCellBorder {
            table_index,
            row,
            col,
            ..
        }
        | UnifiedOp::SetCellPadding {
            table_index,
            row,
            col,
            ..
        }
        | UnifiedOp::ShrinkCellFont {
            table_index,
            row,
            col,
            ..
        } => {
            if excluded_cells.contains(&(*table_index, *row, *col)) {
                Some(format!(
                    "G2 위반: 셀 ({table_index}번 표 [{row}, {col}])은 사용자가 AI 수정 제외로 지정한 영역입니다."
                ))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn check_g2_negative_constraints_typed(
    op: &UnifiedOp,
    negative_constraints: &[String],
    tables_meta: Option<&[TableCoordinateMeta]>,
) -> Option<String> {
    if negative_constraints.is_empty() {
        return None;
    }

    let target_text = match op {
        UnifiedOp::ApplyTextStyle { target_text, .. } => Some(target_text.as_str()),
        UnifiedOp::ApplyParagraphStyle { target_text, .. }
        | UnifiedOp::SetListStyle { target_text, .. } => target_text.as_deref(),
        _ => None,
    };
    let find_text = match op {
        UnifiedOp::ReplaceBodyText { find, .. } => Some(find.as_str()),
        _ => None,
    };
    let text_field = match op {
        UnifiedOp::SetParagraphText { text, .. }
        | UnifiedOp::AppendParagraph { text, .. }
        | UnifiedOp::AppendHeading { text, .. }
        | UnifiedOp::SetCellText { text, .. }
        | UnifiedOp::SetHeaderFooter { text, .. }
        | UnifiedOp::InsertFootnote { text, .. } => Some(text.as_str()),
        _ => None,
    };
    let cell_coord = match op {
        UnifiedOp::SetCellText {
            table_index,
            row,
            col,
            ..
        }
        | UnifiedOp::InsertNestedTable {
            table_index,
            row,
            col,
            ..
        }
        | UnifiedOp::UnmergeCells {
            table_index,
            row,
            col,
        }
        | UnifiedOp::SplitCell {
            table_index,
            row,
            col,
            ..
        }
        | UnifiedOp::SetCellBackground {
            table_index,
            row,
            col,
            ..
        }
        | UnifiedOp::SetCellAlign {
            table_index,
            row,
            col,
            ..
        }
        | UnifiedOp::SetCellBorder {
            table_index,
            row,
            col,
            ..
        }
        | UnifiedOp::SetCellPadding {
            table_index,
            row,
            col,
            ..
        }
        | UnifiedOp::ShrinkCellFont {
            table_index,
            row,
            col,
            ..
        } => Some((*table_index, *row, *col)),
        _ => None,
    };

    for keyword in negative_constraints {
        let mut matched = false;
        if target_text.is_some_and(|t| t.contains(keyword))
            || find_text.is_some_and(|f| f.contains(keyword))
            || text_field.is_some_and(|t| t.contains(keyword))
        {
            matched = true;
        } else if let (Some((ti, r, c)), Some(metas)) = (cell_coord, tables_meta) {
            if let Some(t_meta) = metas.get(ti) {
                if let Some(cell) = t_meta.cells.iter().find(|cell| cell.row == r && cell.col == c)
                {
                    if cell.text.contains(keyword) {
                        matched = true;
                    }
                }
            }
        }
        if matched {
            return Some(format!(
                "G2 위반: 사용자가 수정을 금지한 대상(\"{keyword}\")이 포함된 연산입니다."
            ));
        }
    }
    None
}

fn check_nested_table_policy(
    table_index: usize,
    row: usize,
    col: usize,
    rows: isize,
    cols: isize,
    width_mode: Option<&str>,
    context: &PolicyGuardContext,
) -> Option<String> {
    if context.is_hwpx == Some(false) {
        return Some("HWP 바이너리는 중첩 표 미지원 — HWPX로 저장 후 시도".to_string());
    }
    if !(1..=8).contains(&rows) || !(1..=8).contains(&cols) {
        return Some(format!(
            "중첩 표 크기 제한 위반: 행과 열은 각각 1 이상 8 이하이어야 합니다 (요청: {rows}×{cols})."
        ));
    }
    if let Some(wm) = width_mode {
        if wm != "fit_cell" {
            return Some(format!(
                "지원되지 않는 widthMode입니다 ('fit_cell'만 허용됨, 요청: {wm})."
            ));
        }
    }
    if let Some(metas) = context.tables_meta.as_deref() {
        if !metas.is_empty() {
            let Some(target) = metas.iter().find(|t| t.table_index == table_index) else {
                return Some(format!(
                    "대상 표를 찾을 수 없습니다 (tableIndex: {table_index})."
                ));
            };
            let current_depth = target.depth.unwrap_or(0);
            if current_depth >= 2 {
                return Some(format!(
                    "중첩 깊이 초과: 최대 중첩 깊이는 2단계까지만 허용됩니다 (현재 표 depth={current_depth})."
                ));
            }
            if row >= target.row_count || col >= target.col_count {
                return Some(format!(
                    "대상 셀이 표 범위를 벗어납니다 (tableIndex: {table_index}, 요청: [{row}, {col}], 표 크기: {}×{}).",
                    target.row_count, target.col_count
                ));
            }
        }
    }
    None
}

fn sanitize_typed_op_markdown(op: &mut UnifiedOp) {
    match op {
        UnifiedOp::ReplaceBodyText { replace, .. } => {
            *replace = sanitize_markdown_formatting(replace);
        }
        UnifiedOp::SetParagraphText { text, .. }
        | UnifiedOp::AppendParagraph { text, .. }
        | UnifiedOp::AppendHeading { text, .. }
        | UnifiedOp::SetCellText { text, .. }
        | UnifiedOp::SetHeaderFooter { text, .. }
        | UnifiedOp::InsertFootnote { text, .. } => {
            *text = sanitize_markdown_formatting(text);
        }
        UnifiedOp::InsertTable { headers, rows, .. } => {
            *headers = sanitize_vec_string(headers);
            *rows = sanitize_matrix_string(rows);
        }
        UnifiedOp::InsertNestedTable { data, .. } => {
            if let Some(d) = data.as_mut() {
                *d = sanitize_matrix_string(d);
            }
        }
        UnifiedOp::InsertTableRow { cells, .. } | UnifiedOp::InsertTableCol { cells, .. } => {
            if let Some(c) = cells.as_mut() {
                *c = sanitize_vec_string(c);
            }
        }
        UnifiedOp::ApplyTextStyle { target_text, .. } => {
            *target_text = sanitize_markdown_formatting(target_text);
        }
        UnifiedOp::ApplyParagraphStyle { target_text, .. }
        | UnifiedOp::SetListStyle { target_text, .. } => {
            *target_text = sanitize_opt_string(target_text);
        }
        UnifiedOp::InsertImage { anchor_text, .. } => {
            *anchor_text = sanitize_opt_string(anchor_text);
        }
        UnifiedOp::PlaceSeal { anchor_text, .. } => {
            *anchor_text = sanitize_markdown_formatting(anchor_text);
        }
        _ => {}
    }
}

/// Runs static PolicyGuard G1–G6 checks on typed [`UnifiedOp`]s.
pub fn lint_ops(ops: &[UnifiedOp], context: &PolicyGuardContext) -> PolicyGuardResult {
    let mut rejected_ops = Vec::new();
    let mut sanitized_ops = Vec::new();
    let mut warnings = Vec::new();

    let negative_constraints = extract_negative_constraints(&context.user_prompt);
    let prompt_complains_clipping = RE_CLIPPING.is_match(&context.user_prompt);
    let is_style_requested = RE_STYLE_REQ.is_match(&context.user_prompt);
    let has_dimension_expansion = ops.iter().any(|op| {
        matches!(
            op,
            UnifiedOp::SetRowHeight { .. }
                | UnifiedOp::SetColWidth { .. }
                | UnifiedOp::FitTableToPage { .. }
        )
    });
    let excluded_cell_keys = build_excluded_cell_keys(context);

    for raw_op in ops {
        let mut op = raw_op.clone();

        // G2: excludedRegions + negative constraints
        if let Some(reason) = check_g2_excluded_typed(&op, &excluded_cell_keys).or_else(|| {
            check_g2_negative_constraints_typed(
                &op,
                &negative_constraints,
                context.tables_meta.as_deref(),
            )
        }) {
            rejected_ops.push(RejectedOp { op, reason });
            continue;
        }

        // G5 / nested table policy
        if let UnifiedOp::InsertNestedTable {
            table_index,
            row,
            col,
            rows,
            cols,
            width_mode,
            ..
        } = &op
        {
            let wm_str = width_mode.map(|w| match w {
                hopclaw_types::NestedTableWidthMode::FitCell => "fit_cell",
            });
            if let Some(reason) = check_nested_table_policy(
                *table_index,
                *row,
                *col,
                *rows as isize,
                *cols as isize,
                wm_str,
                context,
            ) {
                rejected_ops.push(RejectedOp { op, reason });
                continue;
            }
        }

        // G1: Anti-worsening overflow
        if let UnifiedOp::ApplyTextStyle { size_pt, bold, .. } = &op {
            let increases_font = size_pt.is_some_and(|s| s > 11.0) || *bold == Some(true);
            if prompt_complains_clipping
                && !is_style_requested
                && increases_font
                && !has_dimension_expansion
            {
                rejected_ops.push(RejectedOp {
                    op,
                    reason: "G1 위반: 사용자가 잘림/오버플로 문제 해결을 요청한 상황에서 글자 크기를 키우면 잘림이 악화됩니다. 대신 set_row_height, set_col_width, fit_table_to_page 또는 shrink_cell_font를 사용하세요.".to_string(),
                });
                continue;
            }
        }

        // G6: Min font size 8pt clamping
        if let UnifiedOp::ApplyTextStyle {
            size_pt: Some(size_pt),
            ..
        } = &mut op
        {
            if *size_pt < 8.0 {
                warnings.push(format!(
                    "G6 준수: apply_text_style의 글자 크기({size_pt}pt)가 최소 하한(8pt) 미만이어서 8pt로 자동 보정되었습니다."
                ));
                *size_pt = 8.0;
            }
        }
        if let UnifiedOp::ShrinkCellFont {
            size_pt, min_pt, ..
        } = &mut op
        {
            if *size_pt < 8.0 {
                warnings.push(format!(
                    "G6 준수: shrink_cell_font의 글자 크기({size_pt}pt)가 최소 하한(8pt) 미만이어서 8pt로 자동 보정되었습니다."
                ));
                *size_pt = 8.0;
            }
            if let Some(m) = min_pt.as_mut() {
                if *m < 8.0 {
                    *m = 8.0;
                }
            }
        }

        // G3: Sanitize markdown markers
        sanitize_typed_op_markdown(&mut op);

        sanitized_ops.push(op);
    }

    PolicyGuardResult {
        passed: rejected_ops.is_empty(),
        rejected_ops,
        sanitized_ops,
        warnings,
    }
}

/// Runs static PolicyGuard G1–G6 checks on raw JSON operation objects (including
/// legacy/untyped `setup_document` for G4 and unvalidated `widthMode` strings for G5).
pub fn lint_raw_json_ops(ops: &[Value], context: &PolicyGuardContext) -> RawPolicyGuardResult {
    let mut rejected_ops = Vec::new();
    let mut sanitized_ops = Vec::new();
    let mut warnings = Vec::new();

    let is_new_doc = context.is_new_document.unwrap_or(false);
    let negative_constraints = extract_negative_constraints(&context.user_prompt);
    let prompt_complains_clipping = RE_CLIPPING.is_match(&context.user_prompt);
    let is_style_requested = RE_STYLE_REQ.is_match(&context.user_prompt);
    let has_dimension_expansion = ops.iter().any(|op| {
        matches!(
            op.get("type").and_then(Value::as_str),
            Some("set_row_height" | "set_col_width" | "fit_table_to_page")
        )
    });
    let excluded_cell_keys = build_excluded_cell_keys(context);

    for raw_op in ops {
        let mut op = raw_op.clone();
        let op_type = op
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();

        // G4: Reject setup_document on existing documents
        if op_type == "setup_document" && !is_new_doc {
            rejected_ops.push(RawRejectedOp {
                op,
                reason: "G4 위반: 기존 문서에 setup_document를 실행하면 전체 서식이 초기화됩니다."
                    .to_string(),
            });
            continue;
        }

        // G2: excludedRegions
        let mut g2_reason = None;
        if op_type == "set_cell_align" {
            if let (Some(ti), Some(sr), Some(sc)) = (
                op.get("tableIndex").and_then(Value::as_u64),
                op.get("row").and_then(Value::as_u64),
                op.get("col").and_then(Value::as_u64),
            ) {
                let er = op.get("endRow").and_then(Value::as_u64).unwrap_or(sr);
                let ec = op.get("endCol").and_then(Value::as_u64).unwrap_or(sc);
                'outer: for r in sr.min(er)..=sr.max(er) {
                    for c in sc.min(ec)..=sc.max(ec) {
                        if excluded_cell_keys.contains(&(ti as usize, r as usize, c as usize)) {
                            g2_reason = Some(format!(
                                "G2 위반: 셀 ({ti}번 표 [{r}, {c}])은 사용자가 AI 수정 제외로 지정한 영역입니다."
                            ));
                            break 'outer;
                        }
                    }
                }
            }
        } else if let (Some(ti), Some(r), Some(c)) = (
            op.get("tableIndex").and_then(Value::as_u64),
            op.get("row").and_then(Value::as_u64),
            op.get("col").and_then(Value::as_u64),
        ) {
            if excluded_cell_keys.contains(&(ti as usize, r as usize, c as usize)) {
                g2_reason = Some(format!(
                    "G2 위반: 셀 ({ti}번 표 [{r}, {c}])은 사용자가 AI 수정 제외로 지정한 영역입니다."
                ));
            }
        }

        // G2: negative constraints
        if g2_reason.is_none() && !negative_constraints.is_empty() {
            for keyword in &negative_constraints {
                let mut matched = false;
                if op
                    .get("targetText")
                    .and_then(Value::as_str)
                    .is_some_and(|s| s.contains(keyword))
                    || op
                        .get("find")
                        .and_then(Value::as_str)
                        .is_some_and(|s| s.contains(keyword))
                    || op
                        .get("text")
                        .and_then(Value::as_str)
                        .is_some_and(|s| s.contains(keyword))
                {
                    matched = true;
                } else if let (Some(ti), Some(r), Some(c), Some(metas)) = (
                    op.get("tableIndex").and_then(Value::as_u64),
                    op.get("row").and_then(Value::as_u64),
                    op.get("col").and_then(Value::as_u64),
                    context.tables_meta.as_deref(),
                ) {
                    if let Some(t_meta) = metas.get(ti as usize) {
                        if t_meta
                            .cells
                            .iter()
                            .find(|cell| cell.row == r as usize && cell.col == c as usize)
                            .is_some_and(|cell| cell.text.contains(keyword))
                        {
                            matched = true;
                        }
                    }
                }
                if matched {
                    g2_reason = Some(format!(
                        "G2 위반: 사용자가 수정을 금지한 대상(\"{keyword}\")이 포함된 연산입니다."
                    ));
                    break;
                }
            }
        }

        if let Some(reason) = g2_reason {
            rejected_ops.push(RawRejectedOp { op, reason });
            continue;
        }

        // insert_nested_table policy check
        if op_type == "insert_nested_table" {
            let ti = op.get("tableIndex").and_then(Value::as_u64).unwrap_or(0) as usize;
            let row = op.get("row").and_then(Value::as_i64).unwrap_or(-1);
            let col = op.get("col").and_then(Value::as_i64).unwrap_or(-1);
            let rows = op.get("rows").and_then(Value::as_i64).unwrap_or(0) as isize;
            let cols = op.get("cols").and_then(Value::as_i64).unwrap_or(0) as isize;
            let wm = op.get("widthMode").and_then(Value::as_str);

            if row < 0 || col < 0 {
                if let Some(metas) = context.tables_meta.as_deref() {
                    if let Some(target) = metas.iter().find(|t| t.table_index == ti) {
                        rejected_ops.push(RawRejectedOp {
                            op,
                            reason: format!(
                                "대상 셀이 표 범위를 벗어납니다 (tableIndex: {ti}, 요청: [{row}, {col}], 표 크기: {}×{}).",
                                target.row_count, target.col_count
                            ),
                        });
                        continue;
                    }
                }
            }

            if let Some(reason) = check_nested_table_policy(
                ti,
                row.max(0) as usize,
                col.max(0) as usize,
                rows,
                cols,
                wm,
                context,
            ) {
                rejected_ops.push(RawRejectedOp { op, reason });
                continue;
            }
        }

        // G1: Anti-worsening overflow
        if op_type == "apply_text_style" {
            let increases_font = op
                .get("sizePt")
                .and_then(Value::as_f64)
                .is_some_and(|s| s > 11.0)
                || op.get("bold").and_then(Value::as_bool) == Some(true);
            if prompt_complains_clipping
                && !is_style_requested
                && increases_font
                && !has_dimension_expansion
            {
                rejected_ops.push(RawRejectedOp {
                    op,
                    reason: "G1 위반: 사용자가 잘림/오버플로 문제 해결을 요청한 상황에서 글자 크기를 키우면 잘림이 악화됩니다. 대신 set_row_height, set_col_width, fit_table_to_page 또는 shrink_cell_font를 사용하세요.".to_string(),
                });
                continue;
            }
        }

        // G6: Min font size 8pt clamping
        if op_type == "apply_text_style" {
            if let Some(size_pt) = op.get("sizePt").and_then(Value::as_f64) {
                if size_pt < 8.0 {
                    warnings.push(format!(
                        "G6 준수: apply_text_style의 글자 크기({size_pt}pt)가 최소 하한(8pt) 미만이어서 8pt로 자동 보정되었습니다."
                    ));
                    if let Some(obj) = op.as_object_mut() {
                        obj.insert("sizePt".into(), json_number(8.0));
                    }
                }
            }
        }
        if op_type == "shrink_cell_font" {
            if let Some(size_pt) = op.get("sizePt").and_then(Value::as_f64) {
                if size_pt < 8.0 {
                    warnings.push(format!(
                        "G6 준수: shrink_cell_font의 글자 크기({size_pt}pt)가 최소 하한(8pt) 미만이어서 8pt로 자동 보정되었습니다."
                    ));
                    if let Some(obj) = op.as_object_mut() {
                        obj.insert("sizePt".into(), json_number(8.0));
                    }
                }
            }
            if let Some(min_pt) = op.get("minPt").and_then(Value::as_f64) {
                if min_pt < 8.0 {
                    if let Some(obj) = op.as_object_mut() {
                        obj.insert("minPt".into(), json_number(8.0));
                    }
                }
            }
        }

        // G3: Sanitize markdown markers across JSON fields
        if let Some(obj) = op.as_object_mut() {
            for key in ["text", "replace", "targetText", "anchorText"] {
                if let Some(Value::String(s)) = obj.get_mut(key) {
                    *s = sanitize_markdown_formatting(s);
                }
            }
            for arr_key in ["headers", "cells"] {
                if let Some(Value::Array(arr)) = obj.get_mut(arr_key) {
                    for item in arr.iter_mut() {
                        if let Value::String(s) = item {
                            *s = sanitize_markdown_formatting(s);
                        }
                    }
                }
            }
            for matrix_key in ["rows", "data"] {
                if let Some(Value::Array(rows)) = obj.get_mut(matrix_key) {
                    for row in rows.iter_mut() {
                        if let Value::Array(cols) = row {
                            for cell in cols.iter_mut() {
                                if let Value::String(s) = cell {
                                    *s = sanitize_markdown_formatting(s);
                                }
                            }
                        }
                    }
                }
            }
        }

        sanitized_ops.push(op);
    }

    RawPolicyGuardResult {
        passed: rejected_ops.is_empty(),
        rejected_ops,
        sanitized_ops,
        warnings,
    }
}

/// Keep unused import clean when callers inspect `ParagraphAlign` / `ParaIndexTarget` / `HwpUnit`.
const _: Option<(ParagraphAlign, ParaIndexTarget, HwpUnit)> = None;
