use hopclaw_types::{
    BorderLineType, BorderSide, CellHAlign, CellVAlign, ColInsertPosition, EqualizeMode,
    ErrorCode, ErrorModel, HeaderFooterLocation, ListStyleMode, NestedTableWidthMode,
    PageNumberAlign, ParaIndexTarget, ParagraphAlign, RowInsertPosition, SealMode,
    TableCoordinateMeta, UnifiedOp,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::sanitize::{
    json_number, sanitize_markdown_formatting, sanitize_matrix_string, sanitize_vec_string,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnginePayload {
    pub is_hwpx: bool,
    pub stdin_json: String,
}

/// Debug / legacy-parity JSON payload translator matching `src/core/opTranslator.ts`.
pub struct OpTranslator;

impl OpTranslator {
    pub fn sanitize_markdown_formatting(text: &str) -> String {
        sanitize_markdown_formatting(text)
    }

    pub fn to_engine_payload(
        staging_file_path: &str,
        is_hwpx: bool,
        ops: &[UnifiedOp],
        tables_meta: &[TableCoordinateMeta],
        is_new_document: bool,
    ) -> Result<EnginePayload, ErrorModel> {
        if is_hwpx {
            let hwpx_ops = Self::to_hwpx_ops(ops, is_new_document);
            if is_new_document {
                let hwp_ops = Self::to_hwp_create_ops(ops, tables_meta, true)?;
                return Ok(EnginePayload {
                    is_hwpx: false,
                    stdin_json: json!({
                        "path": staging_file_path,
                        "operations": hwp_ops,
                    })
                    .to_string(),
                });
            }
            return Ok(EnginePayload {
                is_hwpx: true,
                stdin_json: json!({
                    "path": staging_file_path,
                    "output": staging_file_path,
                    "operations": hwpx_ops,
                })
                .to_string(),
            });
        }

        let hwp_ops = Self::to_hwp_create_ops(ops, tables_meta, is_new_document)?;
        Ok(EnginePayload {
            is_hwpx: false,
            stdin_json: json!({
                "path": staging_file_path,
                "operations": hwp_ops,
            })
            .to_string(),
        })
    }

    fn resolve_hwp_table_coords(
        table_index: usize,
        tables_meta: &[TableCoordinateMeta],
        explicit: Option<(Option<usize>, Option<usize>, Option<usize>)>,
    ) -> Map<String, Value> {
        let mut coords = Map::new();
        if let Some((Some(sec), Some(para), Some(ctrl))) = explicit {
            coords.insert("section".into(), json!(sec));
            coords.insert("para".into(), json!(para));
            coords.insert("control".into(), json!(ctrl));
            coords.insert("tableIndex".into(), json!(table_index));
            return coords;
        }
        if let Some(meta) = tables_meta.get(table_index) {
            coords.insert("section".into(), json!(meta.sec));
            coords.insert("para".into(), json!(meta.para));
            coords.insert("control".into(), json!(meta.ctrl));
            coords.insert("tableIndex".into(), json!(table_index));
            return coords;
        }
        coords.insert("section".into(), json!(0));
        coords.insert("tableIndex".into(), json!(table_index));
        coords
    }

    fn to_hwp_create_ops(
        ops: &[UnifiedOp],
        tables_meta: &[TableCoordinateMeta],
        is_new_document: bool,
    ) -> Result<Vec<Value>, ErrorModel> {
        let mut out = Vec::new();
        if is_new_document {
            out.push(json!({
                "type": "setup_document",
                "page_size": "a4",
                "orientation": "portrait",
                "margin_mm": 20,
            }));
        }

        for op in ops {
            match op {
                UnifiedOp::ReplaceBodyText { find, replace } => {
                    if !find.is_empty() {
                        out.push(json!({
                            "type": "replace_text",
                            "query": find,
                            "replacement": sanitize_markdown_formatting(replace),
                        }));
                    }
                }
                UnifiedOp::SetParagraphText {
                    text,
                    old_text_for_hwp,
                    ..
                } => match old_text_for_hwp.as_deref() {
                    None | Some("") => {
                        if !text.is_empty() {
                            out.push(json!({
                                "type": "append_paragraph",
                                "text": sanitize_markdown_formatting(text),
                            }));
                        }
                    }
                    Some(q) => {
                        out.push(json!({
                            "type": "replace_text",
                            "query": q,
                            "replacement": sanitize_markdown_formatting(text),
                        }));
                    }
                },
                UnifiedOp::AppendParagraph { text, align } => {
                    let mut m = Map::new();
                    m.insert("type".into(), json!("append_paragraph"));
                    m.insert("text".into(), json!(sanitize_markdown_formatting(text)));
                    if let Some(a) = align {
                        m.insert("align".into(), json!(paragraph_align_str(*a)));
                    }
                    out.push(Value::Object(m));
                }
                UnifiedOp::AppendHeading { level, text } => {
                    out.push(json!({
                        "type": "append_heading",
                        "level": level,
                        "text": sanitize_markdown_formatting(text),
                    }));
                }
                UnifiedOp::SetCellText {
                    table_index,
                    sec,
                    para,
                    ctrl,
                    row,
                    col,
                    text,
                    fit,
                } => {
                    let mut m = Self::resolve_hwp_table_coords(
                        *table_index,
                        tables_meta,
                        Some((*sec, *para, *ctrl)),
                    );
                    m.insert("type".into(), json!("set_cell_text"));
                    m.insert("row".into(), json!(row));
                    m.insert("col".into(), json!(col));
                    m.insert("text".into(), json!(sanitize_markdown_formatting(text)));
                    if fit.unwrap_or(false) {
                        m.insert("fit".into(), json!(true));
                    }
                    out.push(Value::Object(m));
                }
                UnifiedOp::InsertTable { headers, rows, .. } => {
                    out.push(json!({
                        "type": "append_table",
                        "headers": sanitize_vec_string(headers),
                        "rows": sanitize_matrix_string(rows),
                    }));
                }
                UnifiedOp::InsertNestedTable { .. } => {
                    return Err(ErrorModel::new(
                        ErrorCode::E_OP_UNSUPPORTED_FORMAT,
                        "HWP 바이너리는 중첩 표 미지원 — HWPX로 저장 후 시도",
                    ));
                }
                UnifiedOp::InsertTableRow {
                    table_index,
                    row,
                    position,
                    cells,
                } => {
                    let coords = Self::resolve_hwp_table_coords(*table_index, tables_meta, None);
                    let mut m = coords.clone();
                    m.insert("type".into(), json!("insert_table_row"));
                    m.insert("row".into(), json!(row));
                    m.insert("position".into(), json!(row_pos_str(*position)));
                    out.push(Value::Object(m));

                    if let Some(cell_list) = cells {
                        if !cell_list.is_empty() {
                            let target_row = match position {
                                RowInsertPosition::Below => row + 1,
                                RowInsertPosition::Above => *row,
                            };
                            for (col_idx, cell_text) in cell_list.iter().enumerate() {
                                if !cell_text.is_empty() {
                                    let mut cm = coords.clone();
                                    cm.insert("type".into(), json!("set_cell_text"));
                                    cm.insert("row".into(), json!(target_row));
                                    cm.insert("col".into(), json!(col_idx));
                                    cm.insert(
                                        "text".into(),
                                        json!(sanitize_markdown_formatting(cell_text)),
                                    );
                                    out.push(Value::Object(cm));
                                }
                            }
                        }
                    }
                }
                UnifiedOp::DeleteTableRow { table_index, row } => {
                    let mut m = Self::resolve_hwp_table_coords(*table_index, tables_meta, None);
                    m.insert("type".into(), json!("delete_table_row"));
                    m.insert("row".into(), json!(row));
                    out.push(Value::Object(m));
                }
                UnifiedOp::InsertTableCol {
                    table_index,
                    col,
                    position,
                    ..
                } => {
                    let mut m = Self::resolve_hwp_table_coords(*table_index, tables_meta, None);
                    m.insert("type".into(), json!("insert_table_col"));
                    m.insert("col".into(), json!(col));
                    m.insert("position".into(), json!(col_pos_str(*position)));
                    out.push(Value::Object(m));
                }
                UnifiedOp::DeleteTableCol { table_index, col } => {
                    let mut m = Self::resolve_hwp_table_coords(*table_index, tables_meta, None);
                    m.insert("type".into(), json!("delete_table_col"));
                    m.insert("col".into(), json!(col));
                    out.push(Value::Object(m));
                }
                UnifiedOp::MergeCells {
                    table_index,
                    from_row,
                    from_col,
                    to_row,
                    to_col,
                } => {
                    let mut m = Self::resolve_hwp_table_coords(*table_index, tables_meta, None);
                    m.insert("type".into(), json!("merge_cells"));
                    m.insert("from_row".into(), json!(from_row));
                    m.insert("from_col".into(), json!(from_col));
                    m.insert("to_row".into(), json!(to_row));
                    m.insert("to_col".into(), json!(to_col));
                    out.push(Value::Object(m));
                }
                UnifiedOp::UnmergeCells {
                    table_index,
                    row,
                    col,
                } => {
                    let mut m = Self::resolve_hwp_table_coords(*table_index, tables_meta, None);
                    m.insert("type".into(), json!("unmerge_cells"));
                    m.insert("row".into(), json!(row));
                    m.insert("col".into(), json!(col));
                    out.push(Value::Object(m));
                }
                UnifiedOp::SplitCell {
                    table_index,
                    row,
                    col,
                    into_rows,
                    ..
                } => {
                    let mut m = Self::resolve_hwp_table_coords(*table_index, tables_meta, None);
                    m.insert("type".into(), json!("split_cell"));
                    m.insert("row".into(), json!(row));
                    m.insert("col".into(), json!(col));
                    m.insert("into_rows".into(), json!(into_rows));
                    out.push(Value::Object(m));
                }
                UnifiedOp::EqualizeTable { table_index, mode } => {
                    let coords = Self::resolve_hwp_table_coords(*table_index, tables_meta, None);
                    if matches!(mode, EqualizeMode::Width | EqualizeMode::Both) {
                        let mut m = coords.clone();
                        m.insert("type".into(), json!("equalize_table_columns"));
                        out.push(Value::Object(m));
                    }
                    if matches!(mode, EqualizeMode::Height | EqualizeMode::Both) {
                        let mut m = coords;
                        m.insert("type".into(), json!("equalize_table_rows"));
                        out.push(Value::Object(m));
                    }
                }
                UnifiedOp::SetCellAlign {
                    table_index,
                    sec,
                    para,
                    ctrl,
                    row,
                    col,
                    end_row,
                    end_col,
                    align,
                    horizontal,
                    valign,
                    vertical,
                } => {
                    let coords = Self::resolve_hwp_table_coords(
                        *table_index,
                        tables_meta,
                        Some((*sec, *para, *ctrl)),
                    );
                    let start_r = *row;
                    let end_r = end_row.unwrap_or(*row);
                    let start_c = *col;
                    let end_c = end_col.unwrap_or(*col);
                    let align_val = align.or(*horizontal).map(|a| cell_h_str(a).to_lowercase());
                    let valign_val = valign.or(*vertical).map(|v| cell_v_str(v).to_lowercase());
                    for r in start_r..=end_r {
                        for c in start_c..=end_c {
                            let mut m = coords.clone();
                            m.insert("type".into(), json!("set_cell_align"));
                            m.insert("row".into(), json!(r));
                            m.insert("col".into(), json!(c));
                            if let Some(ref a) = align_val {
                                m.insert("align".into(), json!(a));
                            }
                            if let Some(ref v) = valign_val {
                                m.insert("valign".into(), json!(v));
                            }
                            out.push(Value::Object(m));
                        }
                    }
                }
                UnifiedOp::SetCellBackground {
                    table_index,
                    row,
                    col,
                    color,
                } => {
                    let mut m = Self::resolve_hwp_table_coords(*table_index, tables_meta, None);
                    m.insert("type".into(), json!("set_cell_background"));
                    m.insert("row".into(), json!(row));
                    m.insert("col".into(), json!(col));
                    m.insert("background_color".into(), json!(color));
                    out.push(Value::Object(m));
                }
                UnifiedOp::SetCellBorder {
                    table_index,
                    row,
                    col,
                    sides,
                    color,
                    line_type,
                    ..
                } => {
                    let mut m = Self::resolve_hwp_table_coords(*table_index, tables_meta, None);
                    m.insert("type".into(), json!("set_cell_border"));
                    m.insert("row".into(), json!(row));
                    m.insert("col".into(), json!(col));
                    m.insert("sides".into(), json!(sides));
                    if let Some(c) = color {
                        if !c.is_empty() {
                            m.insert("color".into(), json!(c));
                        }
                    }
                    if let Some(lt) = line_type {
                        m.insert("line_type".into(), json!(border_line_str(*lt)));
                    }
                    out.push(Value::Object(m));
                }
                UnifiedOp::ApplyTextStyle {
                    target_text,
                    font_family,
                    size_pt,
                    bold,
                    italic,
                    underline,
                    color,
                    highlight,
                    letter_spacing,
                    char_ratio,
                } => {
                    let mut m = Map::new();
                    m.insert("type".into(), json!("apply_text_style"));
                    m.insert("target".into(), json!(target_text));
                    if let Some(ff) = font_family {
                        if !ff.is_empty() {
                            m.insert("font_family".into(), json!(ff));
                        }
                    }
                    if let Some(s) = size_pt {
                        if *s != 0.0 {
                            m.insert("size".into(), json_number(*s));
                        }
                    }
                    if let Some(b) = bold {
                        m.insert("bold".into(), json!(b));
                    }
                    if let Some(i) = italic {
                        m.insert("italic".into(), json!(i));
                    }
                    if let Some(u) = underline {
                        m.insert("underline".into(), json!(u));
                    }
                    if let Some(c) = color {
                        if !c.is_empty() {
                            m.insert("color".into(), json!(c));
                        }
                    }
                    if let Some(h) = highlight {
                        m.insert("highlight".into(), json!(h));
                    }
                    if let Some(ls) = letter_spacing {
                        m.insert("letter_spacing".into(), json_number(*ls));
                    }
                    if let Some(cr) = char_ratio {
                        m.insert("char_ratio".into(), json_number(*cr));
                    }
                    out.push(Value::Object(m));
                }
                UnifiedOp::ApplyParagraphStyle {
                    para_index,
                    target_text,
                    align,
                    indent_hu,
                    line_spacing_percent,
                    spacing_before_hu,
                    spacing_after_hu,
                } => {
                    let mut m = Map::new();
                    m.insert("type".into(), json!("apply_paragraph_style"));
                    if let Some(t) = target_text.as_deref().filter(|s| !s.is_empty()) {
                        m.insert("target".into(), json!(t));
                    } else {
                        let idx = match para_index {
                            Some(ParaIndexTarget::Index(i)) if *i >= 0 => *i,
                            _ => 0,
                        };
                        m.insert("index".into(), json!(idx));
                    }
                    if let Some(a) = align {
                        m.insert("align".into(), json!(paragraph_align_str(*a)));
                    }
                    if let Some(i) = indent_hu {
                        m.insert("indent".into(), json_number(*i));
                    }
                    if let Some(ls) = line_spacing_percent {
                        m.insert("line_spacing".into(), json_number(*ls));
                    }
                    if let Some(sb) = spacing_before_hu {
                        m.insert("spacing_before".into(), json_number(*sb));
                    }
                    if let Some(sa) = spacing_after_hu {
                        m.insert("spacing_after".into(), json_number(*sa));
                    }
                    out.push(Value::Object(m));
                }
                UnifiedOp::SetListStyle {
                    para_index,
                    target_text,
                    mode,
                } => {
                    let mut m = Map::new();
                    m.insert(
                        "type".into(),
                        json!(match mode {
                            ListStyleMode::Numbered => "set_numbered_list",
                            ListStyleMode::Bullet => "set_bullet_list",
                        }),
                    );
                    if let Some(idx) = para_index {
                        m.insert("index".into(), json!(idx));
                    }
                    if let Some(t) = target_text.as_deref().filter(|s| !s.is_empty()) {
                        m.insert("target".into(), json!(t));
                    }
                    out.push(Value::Object(m));
                }
                UnifiedOp::InsertImage {
                    path,
                    anchor_text,
                    width_mm,
                    height_mm,
                    cell,
                } => {
                    let mut m = Map::new();
                    m.insert("type".into(), json!("insert_image"));
                    m.insert("path".into(), json!(path));
                    if let Some(c) = cell {
                        let mut cell_obj =
                            Self::resolve_hwp_table_coords(c.table_index, tables_meta, None);
                        cell_obj.insert("row".into(), json!(c.row));
                        cell_obj.insert("col".into(), json!(c.col));
                        m.insert("cell".into(), Value::Object(cell_obj));
                    } else if let Some(a) = anchor_text.as_deref().filter(|s| !s.is_empty()) {
                        m.insert("anchor".into(), json!(a));
                    }
                    if let Some(w) = width_mm {
                        if *w != 0.0 {
                            m.insert("width_mm".into(), json_number(*w));
                        }
                    }
                    if let Some(h) = height_mm {
                        if *h != 0.0 {
                            m.insert("height_mm".into(), json_number(*h));
                        }
                    }
                    out.push(Value::Object(m));
                }
                UnifiedOp::PlaceSeal {
                    anchor_text,
                    seal_image_path,
                    mode,
                } => {
                    out.push(json!({
                        "type": "place_seal",
                        "anchor": anchor_text,
                        "source": seal_image_path,
                        "mode": seal_mode_str(mode.unwrap_or(SealMode::Auto)),
                    }));
                }
                UnifiedOp::SetHeaderFooter { location, text } => {
                    out.push(json!({
                        "type": match location {
                            HeaderFooterLocation::Header => "insert_header_text",
                            HeaderFooterLocation::Footer => "insert_footer_text",
                        },
                        "text": text,
                    }));
                }
                UnifiedOp::InsertPageNumber { location, align } => {
                    out.push(json!({
                        "type": "insert_page_number",
                        "where": hf_loc_str(location.unwrap_or(HeaderFooterLocation::Footer)),
                        "align": page_num_align_str(align.unwrap_or(PageNumberAlign::Center)),
                    }));
                }
                UnifiedOp::InsertFootnote {
                    anchor_text, text, ..
                } => {
                    out.push(json!({
                        "type": "insert_footnote",
                        "anchor": anchor_text,
                        "text": text,
                    }));
                }
                UnifiedOp::SetColumns { count, spacing_mm } => {
                    let mut m = Map::new();
                    m.insert("type".into(), json!("set_columns"));
                    m.insert("count".into(), json!(count));
                    if let Some(sp) = spacing_mm {
                        if *sp != 0.0 {
                            m.insert("spacing_mm".into(), json_number(*sp));
                        }
                    }
                    out.push(Value::Object(m));
                }
                UnifiedOp::SetColWidth {
                    table_index,
                    col,
                    width_mm,
                    keep_table_width,
                } => {
                    let mut m = Self::resolve_hwp_table_coords(*table_index, tables_meta, None);
                    m.insert("type".into(), json!("set_col_width"));
                    m.insert("col".into(), json!(col));
                    m.insert("width_mm".into(), json_number(*width_mm));
                    if let Some(ktw) = keep_table_width {
                        m.insert("keep_table_width".into(), json!(ktw));
                    }
                    out.push(Value::Object(m));
                }
                UnifiedOp::SetRowHeight {
                    table_index,
                    row,
                    height_mm,
                } => {
                    let mut m = Self::resolve_hwp_table_coords(*table_index, tables_meta, None);
                    m.insert("type".into(), json!("set_row_height"));
                    m.insert("row".into(), json!(row));
                    m.insert("height_mm".into(), json_number(*height_mm));
                    out.push(Value::Object(m));
                }
                UnifiedOp::SetCellPadding {
                    table_index,
                    row,
                    col,
                    left_mm,
                    right_mm,
                    top_mm,
                    bottom_mm,
                    margin_mm,
                } => {
                    let mut m = Self::resolve_hwp_table_coords(*table_index, tables_meta, None);
                    m.insert("type".into(), json!("set_cell_padding"));
                    m.insert("row".into(), json!(row));
                    m.insert("col".into(), json!(col));
                    if let Some(v) = left_mm {
                        m.insert("left_mm".into(), json_number(*v));
                    }
                    if let Some(v) = right_mm {
                        m.insert("right_mm".into(), json_number(*v));
                    }
                    if let Some(v) = top_mm {
                        m.insert("top_mm".into(), json_number(*v));
                    }
                    if let Some(v) = bottom_mm {
                        m.insert("bottom_mm".into(), json_number(*v));
                    }
                    if let Some(v) = margin_mm {
                        m.insert("margin_mm".into(), json_number(*v));
                    }
                    out.push(Value::Object(m));
                }
                UnifiedOp::FitTableToPage {
                    table_index,
                    target_width_mm,
                } => {
                    let mut m = Self::resolve_hwp_table_coords(*table_index, tables_meta, None);
                    m.insert("type".into(), json!("fit_table_to_page"));
                    if let Some(tw) = target_width_mm {
                        m.insert("target_width_mm".into(), json_number(*tw));
                    }
                    out.push(Value::Object(m));
                }
                UnifiedOp::SetPageMargin {
                    left_mm,
                    right_mm,
                    top_mm,
                    bottom_mm,
                    margin_mm,
                } => {
                    let mut m = Map::new();
                    m.insert("type".into(), json!("set_page_margin"));
                    if let Some(v) = left_mm {
                        m.insert("left_mm".into(), json_number(*v));
                    }
                    if let Some(v) = right_mm {
                        m.insert("right_mm".into(), json_number(*v));
                    }
                    if let Some(v) = top_mm {
                        m.insert("top_mm".into(), json_number(*v));
                    }
                    if let Some(v) = bottom_mm {
                        m.insert("bottom_mm".into(), json_number(*v));
                    }
                    if let Some(v) = margin_mm {
                        m.insert("margin_mm".into(), json_number(*v));
                    }
                    out.push(Value::Object(m));
                }
                UnifiedOp::ShrinkCellFont {
                    table_index,
                    row,
                    col,
                    size_pt,
                    min_pt,
                } => {
                    let mut m = Self::resolve_hwp_table_coords(*table_index, tables_meta, None);
                    m.insert("type".into(), json!("shrink_cell_font"));
                    m.insert("row".into(), json!(row));
                    m.insert("col".into(), json!(col));
                    m.insert("size_pt".into(), json_number(*size_pt));
                    if let Some(mp) = min_pt {
                        m.insert("min_pt".into(), json_number(*mp));
                    }
                    out.push(Value::Object(m));
                }
            }
        }

        Ok(out)
    }

    fn to_hwpx_ops(ops: &[UnifiedOp], _is_new_document: bool) -> Vec<Value> {
        let mut out = Vec::new();
        for op in ops {
            match op {
                UnifiedOp::ReplaceBodyText { find, replace } => {
                    if !find.is_empty() {
                        out.push(json!({
                            "type": "replace_text",
                            "find": find,
                            "replace": sanitize_markdown_formatting(replace),
                        }));
                    }
                }
                UnifiedOp::SetParagraphText {
                    para_index, text, ..
                } => {
                    out.push(json!({
                        "type": "set_paragraph_text",
                        "index": para_index,
                        "text": sanitize_markdown_formatting(text),
                    }));
                }
                UnifiedOp::AppendParagraph { text, align } => {
                    let clean = sanitize_markdown_formatting(text);
                    out.push(json!({
                        "type": "append_paragraph",
                        "text": clean,
                    }));
                    if let Some(a) = align {
                        let mut m = Map::new();
                        m.insert("type".into(), json!("apply_paragraph_style"));
                        m.insert("index".into(), json!(-1));
                        if !clean.is_empty() {
                            m.insert("target".into(), json!(clean));
                        }
                        m.insert("align".into(), json!(paragraph_align_str(*a)));
                        out.push(Value::Object(m));
                    }
                }
                UnifiedOp::AppendHeading { level, text } => {
                    let clean = sanitize_markdown_formatting(text);
                    out.push(json!({
                        "type": "append_paragraph",
                        "text": clean,
                    }));
                    let size_pt = match level {
                        1 => 16,
                        2 => 14,
                        3 => 12,
                        4 => 11,
                        _ => 13,
                    };
                    if !clean.is_empty() {
                        out.push(json!({
                            "type": "apply_text_style",
                            "target": clean,
                            "bold": true,
                            "size_pt": size_pt,
                        }));
                    }
                }
                UnifiedOp::SetCellText {
                    table_index,
                    row,
                    col,
                    text,
                    fit,
                    ..
                } => {
                    let mut m = Map::new();
                    m.insert("type".into(), json!("set_cell_text"));
                    m.insert("table".into(), json!(table_index));
                    m.insert("row".into(), json!(row));
                    m.insert("col".into(), json!(col));
                    m.insert("text".into(), json!(sanitize_markdown_formatting(text)));
                    if fit.unwrap_or(false) {
                        m.insert("fit".into(), json!(true));
                    }
                    out.push(Value::Object(m));
                }
                UnifiedOp::InsertTable {
                    after_para_index,
                    headers,
                    rows,
                } => {
                    let mut all_cells = Vec::with_capacity(1 + rows.len());
                    all_cells.push(sanitize_vec_string(headers));
                    all_cells.extend(sanitize_matrix_string(rows));
                    let index_val = match after_para_index {
                        Some(idx) => json!(idx),
                        None => json!("end"),
                    };
                    out.push(json!({
                        "type": "insert_table",
                        "index": index_val,
                        "rows": all_cells.len(),
                        "cols": headers.len(),
                        "cells": all_cells,
                    }));
                }
                UnifiedOp::InsertNestedTable {
                    table_index,
                    row,
                    col,
                    rows,
                    cols,
                    data,
                    header_row,
                    width_mode,
                } => {
                    let mut m = Map::new();
                    m.insert("type".into(), json!("insert_nested_table"));
                    m.insert("table".into(), json!(table_index));
                    m.insert("row".into(), json!(row));
                    m.insert("col".into(), json!(col));
                    m.insert("rows".into(), json!(rows));
                    m.insert("cols".into(), json!(cols));
                    if let Some(d) = data {
                        let sanitized = sanitize_matrix_string(d);
                        m.insert("cells".into(), json!(sanitized));
                        m.insert("data".into(), json!(sanitized));
                    }
                    if let Some(hr) = header_row {
                        m.insert("headerRow".into(), json!(hr));
                    }
                    if let Some(wm) = width_mode {
                        m.insert("widthMode".into(), json!(nested_width_mode_str(*wm)));
                    }
                    out.push(Value::Object(m));
                }
                UnifiedOp::InsertTableRow {
                    table_index,
                    row,
                    position,
                    cells,
                } => {
                    let mut m = Map::new();
                    m.insert("type".into(), json!("insert_table_row"));
                    m.insert("table".into(), json!(table_index));
                    m.insert("row".into(), json!(row));
                    m.insert(
                        "where".into(),
                        json!(match position {
                            RowInsertPosition::Above => "before",
                            RowInsertPosition::Below => "after",
                        }),
                    );
                    if let Some(c) = cells {
                        m.insert("cells".into(), json!(sanitize_vec_string(c)));
                    }
                    out.push(Value::Object(m));
                }
                UnifiedOp::DeleteTableRow { table_index, row } => {
                    out.push(json!({
                        "type": "delete_table_row",
                        "table": table_index,
                        "row": row,
                    }));
                }
                UnifiedOp::InsertTableCol {
                    table_index,
                    col,
                    position,
                    cells,
                } => {
                    let mut m = Map::new();
                    m.insert("type".into(), json!("insert_table_column"));
                    m.insert("table".into(), json!(table_index));
                    m.insert("col".into(), json!(col));
                    m.insert(
                        "where".into(),
                        json!(match position {
                            ColInsertPosition::Left => "before",
                            ColInsertPosition::Right => "after",
                        }),
                    );
                    if let Some(c) = cells {
                        m.insert("cells".into(), json!(sanitize_vec_string(c)));
                    }
                    out.push(Value::Object(m));
                }
                UnifiedOp::DeleteTableCol { table_index, col } => {
                    out.push(json!({
                        "type": "delete_table_column",
                        "table": table_index,
                        "col": col,
                    }));
                }
                UnifiedOp::MergeCells {
                    table_index,
                    from_row,
                    from_col,
                    to_row,
                    to_col,
                } => {
                    let row_span = to_row.saturating_sub(*from_row) + 1;
                    let col_span = to_col.saturating_sub(*from_col) + 1;
                    if row_span == 1 && col_span >= 2 {
                        out.push(json!({
                            "type": "merge_cells",
                            "table": table_index,
                            "mode": "horizontal",
                            "row": from_row,
                            "start": from_col,
                            "count": col_span,
                        }));
                    } else if col_span == 1 && row_span >= 2 {
                        out.push(json!({
                            "type": "merge_cells",
                            "table": table_index,
                            "mode": "vertical",
                            "col": from_col,
                            "start": from_row,
                            "count": row_span,
                        }));
                    } else if row_span >= 2 && col_span >= 2 {
                        for r in *from_row..=*to_row {
                            out.push(json!({
                                "type": "merge_cells",
                                "table": table_index,
                                "mode": "horizontal",
                                "row": r,
                                "start": from_col,
                                "count": col_span,
                            }));
                        }
                        out.push(json!({
                            "type": "merge_cells",
                            "table": table_index,
                            "mode": "vertical",
                            "col": from_col,
                            "start": from_row,
                            "count": row_span,
                        }));
                    }
                }
                UnifiedOp::UnmergeCells {
                    table_index,
                    row,
                    col,
                } => {
                    out.push(json!({
                        "type": "unmerge_cells",
                        "table": table_index,
                        "row": row,
                        "col": col,
                    }));
                }
                UnifiedOp::SplitCell {
                    table_index,
                    row,
                    col,
                    into_rows,
                    into_cols,
                } => {
                    out.push(json!({
                        "type": "split_cell",
                        "table": table_index,
                        "row": row,
                        "col": col,
                        "rows": into_rows,
                        "cols": into_cols.unwrap_or(1),
                    }));
                }
                UnifiedOp::EqualizeTable { table_index, mode } => {
                    out.push(json!({
                        "type": "distribute_table",
                        "table": table_index,
                        "mode": equalize_mode_str(*mode),
                    }));
                }
                UnifiedOp::SetCellAlign {
                    table_index,
                    row,
                    col,
                    end_row,
                    end_col,
                    align,
                    horizontal,
                    valign,
                    vertical,
                    ..
                } => {
                    let start_r = *row;
                    let end_r = end_row.unwrap_or(*row);
                    let start_c = *col;
                    let end_c = end_col.unwrap_or(*col);
                    let h_val = align.or(*horizontal).map(cell_h_str);
                    let v_val = valign.or(*vertical).map(cell_v_str);
                    for r in start_r..=end_r {
                        for c in start_c..=end_c {
                            let mut m = Map::new();
                            m.insert("type".into(), json!("set_cell_align"));
                            m.insert("table".into(), json!(table_index));
                            m.insert("row".into(), json!(r));
                            m.insert("col".into(), json!(c));
                            if let Some(h) = h_val {
                                m.insert("horizontal".into(), json!(h));
                            }
                            if let Some(v) = v_val {
                                m.insert("vertical".into(), json!(v));
                            }
                            out.push(Value::Object(m));
                        }
                    }
                }
                UnifiedOp::SetCellBackground {
                    table_index,
                    row,
                    col,
                    color,
                } => {
                    out.push(json!({
                        "type": "set_cell_background",
                        "table": table_index,
                        "row": row,
                        "col": col,
                        "color": color,
                        "mode": "both",
                    }));
                }
                UnifiedOp::SetCellBorder {
                    table_index,
                    row,
                    col,
                    sides,
                    color,
                    width_mm,
                    ..
                } => {
                    let side_strs: Vec<&'static str> =
                        sides.expand().into_iter().map(border_side_upper).collect();
                    let width_str = match width_mm {
                        Some(w) if *w != 0.0 => format!("{} mm", json_number(*w)),
                        _ => "0.3 mm".to_string(),
                    };
                    out.push(json!({
                        "type": "set_cell_border",
                        "table": table_index,
                        "row": row,
                        "col": col,
                        "color": color.as_deref().filter(|s| !s.is_empty()).unwrap_or("#000000"),
                        "width": width_str,
                        "sides": side_strs,
                    }));
                }
                UnifiedOp::ApplyTextStyle {
                    target_text,
                    font_family,
                    size_pt,
                    bold,
                    italic,
                    underline,
                    color,
                    highlight,
                    letter_spacing,
                    char_ratio,
                } => {
                    let mut m = Map::new();
                    m.insert("type".into(), json!("apply_text_style"));
                    m.insert("target".into(), json!(target_text));
                    if let Some(ff) = font_family {
                        if !ff.is_empty() {
                            m.insert("fontFace".into(), json!(ff));
                        }
                    }
                    if let Some(s) = size_pt {
                        if *s != 0.0 {
                            m.insert("size_pt".into(), json_number(*s));
                        }
                    }
                    if let Some(b) = bold {
                        m.insert("bold".into(), json!(b));
                    }
                    if let Some(i) = italic {
                        m.insert("italic".into(), json!(i));
                    }
                    if let Some(u) = underline {
                        m.insert("underline".into(), json!(u));
                    }
                    if let Some(c) = color {
                        if !c.is_empty() {
                            m.insert("color".into(), json!(c.trim_start_matches('#')));
                        }
                    }
                    if let Some(h) = highlight {
                        m.insert("highlight".into(), json!(h));
                    }
                    if let Some(ls) = letter_spacing {
                        m.insert("letter_spacing".into(), json_number(*ls));
                    }
                    if let Some(cr) = char_ratio {
                        m.insert("char_ratio".into(), json_number(*cr));
                    }
                    out.push(Value::Object(m));
                }
                UnifiedOp::ApplyParagraphStyle {
                    para_index,
                    target_text,
                    align,
                    indent_hu,
                    line_spacing_percent,
                    spacing_before_hu,
                    spacing_after_hu,
                } => {
                    let mut m = Map::new();
                    m.insert("type".into(), json!("apply_paragraph_style"));
                    let idx_val = match para_index {
                        Some(ParaIndexTarget::Last) => -1,
                        Some(ParaIndexTarget::Index(i)) => *i,
                        None => 0,
                    };
                    m.insert("index".into(), json!(idx_val));
                    if let Some(t) = target_text.as_deref().filter(|s| !s.is_empty()) {
                        m.insert("target".into(), json!(t));
                    }
                    if let Some(a) = align {
                        m.insert("align".into(), json!(paragraph_align_str(*a)));
                    }
                    if let Some(i) = indent_hu {
                        m.insert("indent".into(), json_number(*i));
                    }
                    if let Some(ls) = line_spacing_percent {
                        m.insert("lineSpacing".into(), json_number(*ls));
                    }
                    if let Some(sb) = spacing_before_hu {
                        m.insert("spacing_before".into(), json_number(*sb));
                    }
                    if let Some(sa) = spacing_after_hu {
                        m.insert("spacing_after".into(), json_number(*sa));
                    }
                    out.push(Value::Object(m));
                }
                UnifiedOp::SetListStyle {
                    para_index,
                    target_text,
                    mode,
                } => {
                    let mut m = Map::new();
                    m.insert(
                        "type".into(),
                        json!(match mode {
                            ListStyleMode::Numbered => "set_number_list",
                            ListStyleMode::Bullet => "set_bullet_list",
                        }),
                    );
                    m.insert("index".into(), json!(para_index.unwrap_or(0)));
                    if let Some(t) = target_text.as_deref().filter(|s| !s.is_empty()) {
                        m.insert("target".into(), json!(t));
                    }
                    out.push(Value::Object(m));
                }
                UnifiedOp::InsertImage {
                    path,
                    width_mm,
                    height_mm,
                    cell,
                    ..
                } => {
                    let mut m = Map::new();
                    if let Some(c) = cell {
                        m.insert("type".into(), json!("set_cell_image"));
                        m.insert("table".into(), json!(c.table_index));
                        m.insert("row".into(), json!(c.row));
                        m.insert("col".into(), json!(c.col));
                        m.insert("source".into(), json!(path));
                    } else {
                        m.insert("type".into(), json!("insert_image"));
                        m.insert("source".into(), json!(path));
                    }
                    if let Some(w) = width_mm {
                        if *w != 0.0 {
                            m.insert("width_mm".into(), json_number(*w));
                        }
                    }
                    if let Some(h) = height_mm {
                        if *h != 0.0 {
                            m.insert("height_mm".into(), json_number(*h));
                        }
                    }
                    out.push(Value::Object(m));
                }
                UnifiedOp::PlaceSeal {
                    anchor_text,
                    seal_image_path,
                    mode,
                } => {
                    out.push(json!({
                        "type": "place_seal",
                        "anchor": anchor_text,
                        "source": seal_image_path,
                        "mode": seal_mode_str(mode.unwrap_or(SealMode::Auto)),
                    }));
                }
                UnifiedOp::SetHeaderFooter { location, text } => {
                    out.push(json!({
                        "type": match location {
                            HeaderFooterLocation::Header => "set_header",
                            HeaderFooterLocation::Footer => "set_footer",
                        },
                        "text": text,
                    }));
                }
                UnifiedOp::InsertPageNumber { location, align } => {
                    out.push(json!({
                        "type": "set_page_number",
                        "where": hf_loc_str(location.unwrap_or(HeaderFooterLocation::Footer)),
                        "align": page_num_align_str(align.unwrap_or(PageNumberAlign::Center)).to_uppercase(),
                    }));
                }
                UnifiedOp::InsertFootnote {
                    anchor_text,
                    text,
                    para_index,
                } => {
                    out.push(json!({
                        "type": "insert_footnote",
                        "index": para_index.unwrap_or(-1),
                        "anchor": anchor_text,
                        "text": text,
                    }));
                }
                UnifiedOp::SetColumns { count, spacing_mm } => {
                    let mut m = Map::new();
                    m.insert("type".into(), json!("set_columns"));
                    m.insert("count".into(), json!(count));
                    if let Some(sp) = spacing_mm {
                        if *sp != 0.0 {
                            m.insert("gap_mm".into(), json_number(*sp));
                        }
                    }
                    out.push(Value::Object(m));
                }
                UnifiedOp::SetColWidth {
                    table_index,
                    col,
                    width_mm,
                    keep_table_width,
                } => {
                    let mut m = Map::new();
                    m.insert("type".into(), json!("set_col_width"));
                    m.insert("table".into(), json!(table_index));
                    m.insert("col".into(), json!(col));
                    m.insert("width_mm".into(), json_number(*width_mm));
                    if let Some(ktw) = keep_table_width {
                        m.insert("keep_table_width".into(), json!(ktw));
                    }
                    out.push(Value::Object(m));
                }
                UnifiedOp::SetRowHeight {
                    table_index,
                    row,
                    height_mm,
                } => {
                    out.push(json!({
                        "type": "set_row_height",
                        "table": table_index,
                        "row": row,
                        "height_mm": json_number(*height_mm),
                    }));
                }
                UnifiedOp::SetCellPadding {
                    table_index,
                    row,
                    col,
                    left_mm,
                    right_mm,
                    top_mm,
                    bottom_mm,
                    margin_mm,
                } => {
                    let mut m = Map::new();
                    m.insert("type".into(), json!("set_cell_padding"));
                    m.insert("table".into(), json!(table_index));
                    m.insert("row".into(), json!(row));
                    m.insert("col".into(), json!(col));
                    if let Some(v) = left_mm {
                        m.insert("left_mm".into(), json_number(*v));
                    }
                    if let Some(v) = right_mm {
                        m.insert("right_mm".into(), json_number(*v));
                    }
                    if let Some(v) = top_mm {
                        m.insert("top_mm".into(), json_number(*v));
                    }
                    if let Some(v) = bottom_mm {
                        m.insert("bottom_mm".into(), json_number(*v));
                    }
                    if let Some(v) = margin_mm {
                        m.insert("margin_mm".into(), json_number(*v));
                    }
                    out.push(Value::Object(m));
                }
                UnifiedOp::FitTableToPage {
                    table_index,
                    target_width_mm,
                } => {
                    let mut m = Map::new();
                    m.insert("type".into(), json!("fit_table_to_page"));
                    m.insert("table".into(), json!(table_index));
                    if let Some(tw) = target_width_mm {
                        m.insert("target_width_mm".into(), json_number(*tw));
                    }
                    out.push(Value::Object(m));
                }
                UnifiedOp::SetPageMargin {
                    left_mm,
                    right_mm,
                    top_mm,
                    bottom_mm,
                    margin_mm,
                } => {
                    let mut m = Map::new();
                    m.insert("type".into(), json!("set_page_margin"));
                    if let Some(v) = left_mm {
                        m.insert("left_mm".into(), json_number(*v));
                    }
                    if let Some(v) = right_mm {
                        m.insert("right_mm".into(), json_number(*v));
                    }
                    if let Some(v) = top_mm {
                        m.insert("top_mm".into(), json_number(*v));
                    }
                    if let Some(v) = bottom_mm {
                        m.insert("bottom_mm".into(), json_number(*v));
                    }
                    if let Some(v) = margin_mm {
                        m.insert("margin_mm".into(), json_number(*v));
                    }
                    out.push(Value::Object(m));
                }
                UnifiedOp::ShrinkCellFont {
                    table_index,
                    row,
                    col,
                    size_pt,
                    min_pt,
                } => {
                    let mut m = Map::new();
                    m.insert("type".into(), json!("shrink_cell_font"));
                    m.insert("table".into(), json!(table_index));
                    m.insert("row".into(), json!(row));
                    m.insert("col".into(), json!(col));
                    m.insert("size_pt".into(), json_number(*size_pt));
                    if let Some(mp) = min_pt {
                        m.insert("min_pt".into(), json_number(*mp));
                    }
                    out.push(Value::Object(m));
                }
            }
        }
        out
    }
}

fn paragraph_align_str(a: ParagraphAlign) -> &'static str {
    match a {
        ParagraphAlign::Left => "LEFT",
        ParagraphAlign::Center => "CENTER",
        ParagraphAlign::Right => "RIGHT",
        ParagraphAlign::Justify => "JUSTIFY",
        ParagraphAlign::Distribute => "DISTRIBUTE",
    }
}

fn row_pos_str(p: RowInsertPosition) -> &'static str {
    match p {
        RowInsertPosition::Above => "above",
        RowInsertPosition::Below => "below",
    }
}

fn col_pos_str(p: ColInsertPosition) -> &'static str {
    match p {
        ColInsertPosition::Left => "left",
        ColInsertPosition::Right => "right",
    }
}

fn equalize_mode_str(m: EqualizeMode) -> &'static str {
    match m {
        EqualizeMode::Width => "width",
        EqualizeMode::Height => "height",
        EqualizeMode::Both => "both",
    }
}

fn cell_h_str(a: CellHAlign) -> &'static str {
    match a {
        CellHAlign::Left => "LEFT",
        CellHAlign::Center => "CENTER",
        CellHAlign::Right => "RIGHT",
        CellHAlign::Justify => "JUSTIFY",
    }
}

fn cell_v_str(v: CellVAlign) -> &'static str {
    match v {
        CellVAlign::Top => "TOP",
        CellVAlign::Center => "CENTER",
        CellVAlign::Middle => "MIDDLE",
        CellVAlign::Bottom => "BOTTOM",
    }
}

fn border_line_str(l: BorderLineType) -> &'static str {
    match l {
        BorderLineType::Solid => "solid",
        BorderLineType::Dashed => "dashed",
        BorderLineType::Dotted => "dotted",
        BorderLineType::Double => "double",
    }
}

fn border_side_upper(s: BorderSide) -> &'static str {
    match s {
        BorderSide::Left => "LEFT",
        BorderSide::Right => "RIGHT",
        BorderSide::Top => "TOP",
        BorderSide::Bottom => "BOTTOM",
    }
}

fn nested_width_mode_str(w: NestedTableWidthMode) -> &'static str {
    match w {
        NestedTableWidthMode::FitCell => "fit_cell",
    }
}

fn seal_mode_str(m: SealMode) -> &'static str {
    match m {
        SealMode::Auto => "auto",
        SealMode::Overlap => "overlap",
        SealMode::Right => "right",
    }
}

fn hf_loc_str(l: HeaderFooterLocation) -> &'static str {
    match l {
        HeaderFooterLocation::Header => "header",
        HeaderFooterLocation::Footer => "footer",
    }
}

fn page_num_align_str(a: PageNumberAlign) -> &'static str {
    match a {
        PageNumberAlign::Left => "left",
        PageNumberAlign::Center => "center",
        PageNumberAlign::Right => "right",
    }
}
