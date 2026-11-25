use hopclaw_types::{
    mm_to_hwpunit, pt_to_hwpunit, BorderLineType, DocFormat, EditPlan, ErrorCode, ErrorModel,
    HeaderFooterLocation, Hwp5CellImageTarget, Hwp5EditPlan, Hwp5EditStep, Hwp5TableTarget,
    HwpxEditPlan, HwpxEditStep, HwpUnit, NestedTableWidthMode, PageNumberAlign, SealMode,
    TableCoordinateMeta, UnifiedOp,
};

use crate::sanitize::{sanitize_markdown_formatting, sanitize_matrix_string, sanitize_vec_string};

fn heading_pt(level: u8) -> f64 {
    match level {
        1 => 16.0,
        2 => 14.0,
        3 => 12.0,
        4 => 11.0,
        _ => 13.0,
    }
}

fn validate_table_index(
    table_index: usize,
    tables_meta: &[TableCoordinateMeta],
    appended_tables: usize,
) -> Result<Option<&TableCoordinateMeta>, ErrorModel> {
    if let Some(meta) = tables_meta.iter().find(|t| t.table_index == table_index) {
        return Ok(Some(meta));
    }
    if table_index < tables_meta.len() + appended_tables {
        // Refers to a table appended earlier in the same batch.
        return Ok(None);
    }
    Err(ErrorModel::new(
        ErrorCode::E_TABLE_NOT_FOUND,
        format!(
            "대상 표를 찾을 수 없습니다 (tableIndex: {table_index}, 문서 표 개수: {})",
            tables_meta.len()
        ),
    ))
}

fn resolve_hwp5_target(
    table_index: usize,
    tables_meta: &[TableCoordinateMeta],
    appended_tables: usize,
    explicit: Option<(Option<usize>, Option<usize>, Option<usize>)>,
) -> Result<Hwp5TableTarget, ErrorModel> {
    if let Some((Some(sec), Some(para), Some(ctrl))) = explicit {
        return Ok(Hwp5TableTarget {
            table_index,
            sec: Some(sec),
            para: Some(para),
            ctrl: Some(ctrl),
        });
    }
    let meta = validate_table_index(table_index, tables_meta, appended_tables)?;
    match meta {
        Some(m) => Ok(Hwp5TableTarget {
            table_index,
            sec: Some(m.sec),
            para: Some(m.para),
            ctrl: Some(m.ctrl),
        }),
        None => Ok(Hwp5TableTarget {
            table_index,
            sec: None,
            para: None,
            ctrl: None,
        }),
    }
}

/// Compiles a slice of [`UnifiedOp`]s into a format-specific [`EditPlan`] (`HwpxEditPlan` or `Hwp5EditPlan`)
/// with all physical dimensions resolved to integer `HwpUnit`.
///
/// Enforces:
/// - Missing `tableIndex` (not in `tables_meta` and not appended earlier in the batch) -> `E_TABLE_NOT_FOUND`
/// - `insert_nested_table` on `DocFormat::Hwp5` -> `E_OP_UNSUPPORTED_FORMAT`
pub fn compile_edit_plan(
    format: DocFormat,
    ops: &[UnifiedOp],
    tables_meta: &[TableCoordinateMeta],
    is_new_document: bool,
) -> Result<EditPlan, ErrorModel> {
    match format {
        DocFormat::Hwpx => compile_hwpx_plan(ops, tables_meta, is_new_document).map(EditPlan::Hwpx),
        DocFormat::Hwp5 => compile_hwp5_plan(ops, tables_meta, is_new_document).map(EditPlan::Hwp5),
    }
}

fn compile_hwpx_plan(
    ops: &[UnifiedOp],
    tables_meta: &[TableCoordinateMeta],
    is_new_document: bool,
) -> Result<HwpxEditPlan, ErrorModel> {
    let mut steps = Vec::with_capacity(ops.len());
    let mut appended_tables = 0usize;

    for op in ops {
        match op {
            UnifiedOp::ReplaceBodyText { find, replace } => {
                if !find.is_empty() {
                    steps.push(HwpxEditStep::ReplaceText {
                        find: find.clone(),
                        replace: sanitize_markdown_formatting(replace),
                    });
                }
            }
            UnifiedOp::SetParagraphText {
                para_index, text, ..
            } => {
                steps.push(HwpxEditStep::SetParagraphText {
                    index: *para_index,
                    text: sanitize_markdown_formatting(text),
                });
            }
            UnifiedOp::AppendParagraph { text, align } => {
                steps.push(HwpxEditStep::AppendParagraph {
                    text: sanitize_markdown_formatting(text),
                    align: *align,
                });
            }
            UnifiedOp::AppendHeading { level, text } => {
                steps.push(HwpxEditStep::AppendHeading {
                    level: *level,
                    text: sanitize_markdown_formatting(text),
                    size_hu: pt_to_hwpunit(heading_pt(*level)),
                });
            }
            UnifiedOp::SetCellText {
                table_index,
                row,
                col,
                text,
                fit,
                ..
            } => {
                validate_table_index(*table_index, tables_meta, appended_tables)?;
                steps.push(HwpxEditStep::SetCellText {
                    table_index: *table_index,
                    row: *row,
                    col: *col,
                    text: sanitize_markdown_formatting(text),
                    fit: fit.unwrap_or(false),
                });
            }
            UnifiedOp::InsertTable {
                after_para_index,
                headers,
                rows,
            } => {
                appended_tables += 1;
                steps.push(HwpxEditStep::InsertTable {
                    after_para_index: *after_para_index,
                    headers: sanitize_vec_string(headers),
                    rows: sanitize_matrix_string(rows),
                });
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
                validate_table_index(*table_index, tables_meta, appended_tables)?;
                steps.push(HwpxEditStep::InsertNestedTable {
                    table_index: *table_index,
                    row: *row,
                    col: *col,
                    rows: *rows,
                    cols: *cols,
                    data: data.as_deref().map(sanitize_matrix_string),
                    header_row: header_row.unwrap_or(false),
                    width_mode: width_mode.unwrap_or(NestedTableWidthMode::FitCell),
                });
            }
            UnifiedOp::InsertTableRow {
                table_index,
                row,
                position,
                cells,
            } => {
                validate_table_index(*table_index, tables_meta, appended_tables)?;
                steps.push(HwpxEditStep::InsertTableRow {
                    table_index: *table_index,
                    row: *row,
                    position: *position,
                    cells: cells.as_deref().map(sanitize_vec_string),
                });
            }
            UnifiedOp::DeleteTableRow { table_index, row } => {
                validate_table_index(*table_index, tables_meta, appended_tables)?;
                steps.push(HwpxEditStep::DeleteTableRow {
                    table_index: *table_index,
                    row: *row,
                });
            }
            UnifiedOp::InsertTableCol {
                table_index,
                col,
                position,
                cells,
            } => {
                validate_table_index(*table_index, tables_meta, appended_tables)?;
                steps.push(HwpxEditStep::InsertTableCol {
                    table_index: *table_index,
                    col: *col,
                    position: *position,
                    cells: cells.as_deref().map(sanitize_vec_string),
                });
            }
            UnifiedOp::DeleteTableCol { table_index, col } => {
                validate_table_index(*table_index, tables_meta, appended_tables)?;
                steps.push(HwpxEditStep::DeleteTableCol {
                    table_index: *table_index,
                    col: *col,
                });
            }
            UnifiedOp::MergeCells {
                table_index,
                from_row,
                from_col,
                to_row,
                to_col,
            } => {
                validate_table_index(*table_index, tables_meta, appended_tables)?;
                steps.push(HwpxEditStep::MergeCells {
                    table_index: *table_index,
                    from_row: *from_row,
                    from_col: *from_col,
                    to_row: *to_row,
                    to_col: *to_col,
                });
            }
            UnifiedOp::UnmergeCells {
                table_index,
                row,
                col,
            } => {
                validate_table_index(*table_index, tables_meta, appended_tables)?;
                steps.push(HwpxEditStep::UnmergeCells {
                    table_index: *table_index,
                    row: *row,
                    col: *col,
                });
            }
            UnifiedOp::SplitCell {
                table_index,
                row,
                col,
                into_rows,
                into_cols,
            } => {
                validate_table_index(*table_index, tables_meta, appended_tables)?;
                steps.push(HwpxEditStep::SplitCell {
                    table_index: *table_index,
                    row: *row,
                    col: *col,
                    into_rows: *into_rows,
                    into_cols: into_cols.unwrap_or(1),
                });
            }
            UnifiedOp::EqualizeTable { table_index, mode } => {
                validate_table_index(*table_index, tables_meta, appended_tables)?;
                steps.push(HwpxEditStep::EqualizeTable {
                    table_index: *table_index,
                    mode: *mode,
                });
            }
            UnifiedOp::SetCellBackground {
                table_index,
                row,
                col,
                color,
            } => {
                validate_table_index(*table_index, tables_meta, appended_tables)?;
                steps.push(HwpxEditStep::SetCellBackground {
                    table_index: *table_index,
                    row: *row,
                    col: *col,
                    color: color.clone(),
                });
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
                validate_table_index(*table_index, tables_meta, appended_tables)?;
                steps.push(HwpxEditStep::SetCellAlign {
                    table_index: *table_index,
                    row: *row,
                    col: *col,
                    end_row: *end_row,
                    end_col: *end_col,
                    horizontal: align.or(*horizontal),
                    vertical: valign.or(*vertical),
                });
            }
            UnifiedOp::SetCellBorder {
                table_index,
                row,
                col,
                sides,
                color,
                width_mm,
                line_type,
            } => {
                validate_table_index(*table_index, tables_meta, appended_tables)?;
                steps.push(HwpxEditStep::SetCellBorder {
                    table_index: *table_index,
                    row: *row,
                    col: *col,
                    sides: sides.expand(),
                    color: color.clone().unwrap_or_else(|| "#000000".to_string()),
                    width_hu: mm_to_hwpunit(width_mm.unwrap_or(0.3)),
                    line_type: line_type.unwrap_or(BorderLineType::Solid),
                });
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
                steps.push(HwpxEditStep::ApplyTextStyle {
                    target_text: target_text.clone(),
                    font_family: font_family.clone(),
                    size_hu: size_pt.map(pt_to_hwpunit),
                    bold: *bold,
                    italic: *italic,
                    underline: *underline,
                    color: color.clone(),
                    highlight: highlight.clone(),
                    letter_spacing: *letter_spacing,
                    char_ratio: *char_ratio,
                });
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
                steps.push(HwpxEditStep::ApplyParagraphStyle {
                    para_index: *para_index,
                    target_text: target_text.clone(),
                    align: *align,
                    indent_hu: indent_hu.map(|v| v.round() as HwpUnit),
                    line_spacing_percent: *line_spacing_percent,
                    spacing_before_hu: spacing_before_hu.map(|v| v.round() as HwpUnit),
                    spacing_after_hu: spacing_after_hu.map(|v| v.round() as HwpUnit),
                });
            }
            UnifiedOp::SetListStyle {
                para_index,
                target_text,
                mode,
            } => {
                steps.push(HwpxEditStep::SetListStyle {
                    para_index: *para_index,
                    target_text: target_text.clone(),
                    mode: *mode,
                });
            }
            UnifiedOp::InsertImage {
                path,
                anchor_text,
                width_mm,
                height_mm,
                cell,
            } => {
                if let Some(c) = cell {
                    validate_table_index(c.table_index, tables_meta, appended_tables)?;
                }
                steps.push(HwpxEditStep::InsertImage {
                    path: path.clone(),
                    anchor_text: anchor_text.clone(),
                    width_hu: width_mm.map(mm_to_hwpunit),
                    height_hu: height_mm.map(mm_to_hwpunit),
                    cell: cell.clone(),
                });
            }
            UnifiedOp::PlaceSeal {
                anchor_text,
                seal_image_path,
                mode,
            } => {
                steps.push(HwpxEditStep::PlaceSeal {
                    anchor_text: anchor_text.clone(),
                    seal_image_path: seal_image_path.clone(),
                    mode: mode.unwrap_or(SealMode::Auto),
                });
            }
            UnifiedOp::SetHeaderFooter { location, text } => {
                steps.push(HwpxEditStep::SetHeaderFooter {
                    location: *location,
                    text: text.clone(),
                });
            }
            UnifiedOp::InsertPageNumber { location, align } => {
                steps.push(HwpxEditStep::InsertPageNumber {
                    location: location.unwrap_or(HeaderFooterLocation::Footer),
                    align: align.unwrap_or(PageNumberAlign::Center),
                });
            }
            UnifiedOp::InsertFootnote {
                anchor_text,
                text,
                para_index,
            } => {
                steps.push(HwpxEditStep::InsertFootnote {
                    anchor_text: anchor_text.clone(),
                    text: text.clone(),
                    para_index: *para_index,
                });
            }
            UnifiedOp::SetColumns { count, spacing_mm } => {
                steps.push(HwpxEditStep::SetColumns {
                    count: *count,
                    spacing_hu: spacing_mm.map(mm_to_hwpunit),
                });
            }
            UnifiedOp::SetColWidth {
                table_index,
                col,
                width_mm,
                keep_table_width,
            } => {
                validate_table_index(*table_index, tables_meta, appended_tables)?;
                steps.push(HwpxEditStep::SetColWidth {
                    table_index: *table_index,
                    col: *col,
                    width_hu: mm_to_hwpunit(*width_mm),
                    keep_table_width: keep_table_width.unwrap_or(false),
                });
            }
            UnifiedOp::SetRowHeight {
                table_index,
                row,
                height_mm,
            } => {
                validate_table_index(*table_index, tables_meta, appended_tables)?;
                steps.push(HwpxEditStep::SetRowHeight {
                    table_index: *table_index,
                    row: *row,
                    height_hu: mm_to_hwpunit(*height_mm),
                });
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
                validate_table_index(*table_index, tables_meta, appended_tables)?;
                steps.push(HwpxEditStep::SetCellPadding {
                    table_index: *table_index,
                    row: *row,
                    col: *col,
                    left_hu: left_mm.or(*margin_mm).map(mm_to_hwpunit),
                    right_hu: right_mm.or(*margin_mm).map(mm_to_hwpunit),
                    top_hu: top_mm.or(*margin_mm).map(mm_to_hwpunit),
                    bottom_hu: bottom_mm.or(*margin_mm).map(mm_to_hwpunit),
                });
            }
            UnifiedOp::FitTableToPage {
                table_index,
                target_width_mm,
            } => {
                validate_table_index(*table_index, tables_meta, appended_tables)?;
                steps.push(HwpxEditStep::FitTableToPage {
                    table_index: *table_index,
                    target_width_hu: target_width_mm.map(mm_to_hwpunit),
                });
            }
            UnifiedOp::SetPageMargin {
                left_mm,
                right_mm,
                top_mm,
                bottom_mm,
                margin_mm,
            } => {
                steps.push(HwpxEditStep::SetPageMargin {
                    left_hu: left_mm.or(*margin_mm).map(mm_to_hwpunit),
                    right_hu: right_mm.or(*margin_mm).map(mm_to_hwpunit),
                    top_hu: top_mm.or(*margin_mm).map(mm_to_hwpunit),
                    bottom_hu: bottom_mm.or(*margin_mm).map(mm_to_hwpunit),
                });
            }
            UnifiedOp::ShrinkCellFont {
                table_index,
                row,
                col,
                size_pt,
                min_pt,
            } => {
                validate_table_index(*table_index, tables_meta, appended_tables)?;
                steps.push(HwpxEditStep::ShrinkCellFont {
                    table_index: *table_index,
                    row: *row,
                    col: *col,
                    size_hu: pt_to_hwpunit(*size_pt),
                    min_hu: pt_to_hwpunit(min_pt.unwrap_or(8.0)),
                });
            }
        }
    }

    Ok(HwpxEditPlan {
        is_new_document,
        steps,
    })
}

fn compile_hwp5_plan(
    ops: &[UnifiedOp],
    tables_meta: &[TableCoordinateMeta],
    is_new_document: bool,
) -> Result<Hwp5EditPlan, ErrorModel> {
    let mut steps = Vec::with_capacity(ops.len() + usize::from(is_new_document));
    let mut appended_tables = 0usize;

    if is_new_document {
        steps.push(Hwp5EditStep::SetupDocument {
            width_hu: mm_to_hwpunit(210.0),
            height_hu: mm_to_hwpunit(297.0),
            margin_hu: mm_to_hwpunit(20.0),
        });
    }

    for op in ops {
        match op {
            UnifiedOp::ReplaceBodyText { find, replace } => {
                if !find.is_empty() {
                    steps.push(Hwp5EditStep::ReplaceText {
                        find: find.clone(),
                        replace: sanitize_markdown_formatting(replace),
                    });
                }
            }
            UnifiedOp::SetParagraphText {
                text,
                old_text_for_hwp,
                ..
            } => match old_text_for_hwp.as_deref() {
                Some(q) if !q.is_empty() => {
                    steps.push(Hwp5EditStep::ReplaceText {
                        find: q.to_string(),
                        replace: sanitize_markdown_formatting(text),
                    });
                }
                _ => {
                    if !text.is_empty() {
                        steps.push(Hwp5EditStep::AppendParagraph {
                            text: sanitize_markdown_formatting(text),
                            align: None,
                        });
                    }
                }
            },
            UnifiedOp::AppendParagraph { text, align } => {
                steps.push(Hwp5EditStep::AppendParagraph {
                    text: sanitize_markdown_formatting(text),
                    align: *align,
                });
            }
            UnifiedOp::AppendHeading { level, text } => {
                steps.push(Hwp5EditStep::AppendHeading {
                    level: *level,
                    text: sanitize_markdown_formatting(text),
                    size_hu: pt_to_hwpunit(heading_pt(*level)),
                });
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
                let target = resolve_hwp5_target(
                    *table_index,
                    tables_meta,
                    appended_tables,
                    Some((*sec, *para, *ctrl)),
                )?;
                steps.push(Hwp5EditStep::SetCellText {
                    target,
                    row: *row,
                    col: *col,
                    text: sanitize_markdown_formatting(text),
                    fit: fit.unwrap_or(false),
                });
            }
            UnifiedOp::InsertTable { headers, rows, .. } => {
                appended_tables += 1;
                steps.push(Hwp5EditStep::AppendTable {
                    headers: sanitize_vec_string(headers),
                    rows: sanitize_matrix_string(rows),
                });
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
                let target = resolve_hwp5_target(*table_index, tables_meta, appended_tables, None)?;
                steps.push(Hwp5EditStep::InsertTableRow {
                    target,
                    row: *row,
                    position: *position,
                    cells: cells.as_deref().map(sanitize_vec_string),
                });
            }
            UnifiedOp::DeleteTableRow { table_index, row } => {
                let target = resolve_hwp5_target(*table_index, tables_meta, appended_tables, None)?;
                steps.push(Hwp5EditStep::DeleteTableRow { target, row: *row });
            }
            UnifiedOp::InsertTableCol {
                table_index,
                col,
                position,
                cells,
            } => {
                let target = resolve_hwp5_target(*table_index, tables_meta, appended_tables, None)?;
                steps.push(Hwp5EditStep::InsertTableCol {
                    target,
                    col: *col,
                    position: *position,
                    cells: cells.as_deref().map(sanitize_vec_string),
                });
            }
            UnifiedOp::DeleteTableCol { table_index, col } => {
                let target = resolve_hwp5_target(*table_index, tables_meta, appended_tables, None)?;
                steps.push(Hwp5EditStep::DeleteTableCol { target, col: *col });
            }
            UnifiedOp::MergeCells {
                table_index,
                from_row,
                from_col,
                to_row,
                to_col,
            } => {
                let target = resolve_hwp5_target(*table_index, tables_meta, appended_tables, None)?;
                steps.push(Hwp5EditStep::MergeCells {
                    target,
                    from_row: *from_row,
                    from_col: *from_col,
                    to_row: *to_row,
                    to_col: *to_col,
                });
            }
            UnifiedOp::UnmergeCells {
                table_index,
                row,
                col,
            } => {
                let target = resolve_hwp5_target(*table_index, tables_meta, appended_tables, None)?;
                steps.push(Hwp5EditStep::UnmergeCells {
                    target,
                    row: *row,
                    col: *col,
                });
            }
            UnifiedOp::SplitCell {
                table_index,
                row,
                col,
                into_rows,
                into_cols,
            } => {
                let target = resolve_hwp5_target(*table_index, tables_meta, appended_tables, None)?;
                steps.push(Hwp5EditStep::SplitCell {
                    target,
                    row: *row,
                    col: *col,
                    into_rows: *into_rows,
                    into_cols: into_cols.unwrap_or(1),
                });
            }
            UnifiedOp::EqualizeTable { table_index, mode } => {
                let target = resolve_hwp5_target(*table_index, tables_meta, appended_tables, None)?;
                steps.push(Hwp5EditStep::EqualizeTable {
                    target,
                    mode: *mode,
                });
            }
            UnifiedOp::SetCellBackground {
                table_index,
                row,
                col,
                color,
            } => {
                let target = resolve_hwp5_target(*table_index, tables_meta, appended_tables, None)?;
                steps.push(Hwp5EditStep::SetCellBackground {
                    target,
                    row: *row,
                    col: *col,
                    color: color.clone(),
                });
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
                let target = resolve_hwp5_target(
                    *table_index,
                    tables_meta,
                    appended_tables,
                    Some((*sec, *para, *ctrl)),
                )?;
                steps.push(Hwp5EditStep::SetCellAlign {
                    target,
                    row: *row,
                    col: *col,
                    end_row: *end_row,
                    end_col: *end_col,
                    horizontal: align.or(*horizontal),
                    vertical: valign.or(*vertical),
                });
            }
            UnifiedOp::SetCellBorder {
                table_index,
                row,
                col,
                sides,
                color,
                width_mm,
                line_type,
            } => {
                let target = resolve_hwp5_target(*table_index, tables_meta, appended_tables, None)?;
                steps.push(Hwp5EditStep::SetCellBorder {
                    target,
                    row: *row,
                    col: *col,
                    sides: sides.expand(),
                    color: color.clone(),
                    width_hu: width_mm.map(mm_to_hwpunit),
                    line_type: *line_type,
                });
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
                steps.push(Hwp5EditStep::ApplyTextStyle {
                    target_text: target_text.clone(),
                    font_family: font_family.clone(),
                    size_hu: size_pt.map(pt_to_hwpunit),
                    bold: *bold,
                    italic: *italic,
                    underline: *underline,
                    color: color.clone(),
                    highlight: highlight.clone(),
                    letter_spacing: *letter_spacing,
                    char_ratio: *char_ratio,
                });
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
                steps.push(Hwp5EditStep::ApplyParagraphStyle {
                    para_index: *para_index,
                    target_text: target_text.clone(),
                    align: *align,
                    indent_hu: indent_hu.map(|v| v.round() as HwpUnit),
                    line_spacing_percent: *line_spacing_percent,
                    spacing_before_hu: spacing_before_hu.map(|v| v.round() as HwpUnit),
                    spacing_after_hu: spacing_after_hu.map(|v| v.round() as HwpUnit),
                });
            }
            UnifiedOp::SetListStyle {
                para_index,
                target_text,
                mode,
            } => {
                steps.push(Hwp5EditStep::SetListStyle {
                    para_index: *para_index,
                    target_text: target_text.clone(),
                    mode: *mode,
                });
            }
            UnifiedOp::InsertImage {
                path,
                anchor_text,
                width_mm,
                height_mm,
                cell,
            } => {
                let target_cell = match cell {
                    Some(c) => {
                        let table = resolve_hwp5_target(
                            c.table_index,
                            tables_meta,
                            appended_tables,
                            None,
                        )?;
                        Some(Hwp5CellImageTarget {
                            table,
                            row: c.row,
                            col: c.col,
                        })
                    }
                    None => None,
                };
                steps.push(Hwp5EditStep::InsertImage {
                    path: path.clone(),
                    anchor_text: anchor_text.clone(),
                    width_hu: width_mm.map(mm_to_hwpunit),
                    height_hu: height_mm.map(mm_to_hwpunit),
                    cell: target_cell,
                });
            }
            UnifiedOp::PlaceSeal {
                anchor_text,
                seal_image_path,
                mode,
            } => {
                steps.push(Hwp5EditStep::PlaceSeal {
                    anchor_text: anchor_text.clone(),
                    seal_image_path: seal_image_path.clone(),
                    mode: mode.unwrap_or(SealMode::Auto),
                });
            }
            UnifiedOp::SetHeaderFooter { location, text } => {
                steps.push(Hwp5EditStep::SetHeaderFooter {
                    location: *location,
                    text: text.clone(),
                });
            }
            UnifiedOp::InsertPageNumber { location, align } => {
                steps.push(Hwp5EditStep::InsertPageNumber {
                    location: location.unwrap_or(HeaderFooterLocation::Footer),
                    align: align.unwrap_or(PageNumberAlign::Center),
                });
            }
            UnifiedOp::InsertFootnote {
                anchor_text,
                text,
                para_index,
            } => {
                steps.push(Hwp5EditStep::InsertFootnote {
                    anchor_text: anchor_text.clone(),
                    text: text.clone(),
                    para_index: *para_index,
                });
            }
            UnifiedOp::SetColumns { count, spacing_mm } => {
                steps.push(Hwp5EditStep::SetColumns {
                    count: *count,
                    spacing_hu: spacing_mm.map(mm_to_hwpunit),
                });
            }
            UnifiedOp::SetColWidth {
                table_index,
                col,
                width_mm,
                keep_table_width,
            } => {
                let target = resolve_hwp5_target(*table_index, tables_meta, appended_tables, None)?;
                steps.push(Hwp5EditStep::SetColWidth {
                    target,
                    col: *col,
                    width_hu: mm_to_hwpunit(*width_mm),
                    keep_table_width: keep_table_width.unwrap_or(false),
                });
            }
            UnifiedOp::SetRowHeight {
                table_index,
                row,
                height_mm,
            } => {
                let target = resolve_hwp5_target(*table_index, tables_meta, appended_tables, None)?;
                steps.push(Hwp5EditStep::SetRowHeight {
                    target,
                    row: *row,
                    height_hu: mm_to_hwpunit(*height_mm),
                });
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
                let target = resolve_hwp5_target(*table_index, tables_meta, appended_tables, None)?;
                steps.push(Hwp5EditStep::SetCellPadding {
                    target,
                    row: *row,
                    col: *col,
                    left_hu: left_mm.or(*margin_mm).map(mm_to_hwpunit),
                    right_hu: right_mm.or(*margin_mm).map(mm_to_hwpunit),
                    top_hu: top_mm.or(*margin_mm).map(mm_to_hwpunit),
                    bottom_hu: bottom_mm.or(*margin_mm).map(mm_to_hwpunit),
                });
            }
            UnifiedOp::FitTableToPage {
                table_index,
                target_width_mm,
            } => {
                let target = resolve_hwp5_target(*table_index, tables_meta, appended_tables, None)?;
                steps.push(Hwp5EditStep::FitTableToPage {
                    target,
                    target_width_hu: target_width_mm.map(mm_to_hwpunit),
                });
            }
            UnifiedOp::SetPageMargin {
                left_mm,
                right_mm,
                top_mm,
                bottom_mm,
                margin_mm,
            } => {
                steps.push(Hwp5EditStep::SetPageMargin {
                    left_hu: left_mm.or(*margin_mm).map(mm_to_hwpunit),
                    right_hu: right_mm.or(*margin_mm).map(mm_to_hwpunit),
                    top_hu: top_mm.or(*margin_mm).map(mm_to_hwpunit),
                    bottom_hu: bottom_mm.or(*margin_mm).map(mm_to_hwpunit),
                });
            }
            UnifiedOp::ShrinkCellFont {
                table_index,
                row,
                col,
                size_pt,
                min_pt,
            } => {
                let target = resolve_hwp5_target(*table_index, tables_meta, appended_tables, None)?;
                steps.push(Hwp5EditStep::ShrinkCellFont {
                    target,
                    row: *row,
                    col: *col,
                    size_hu: pt_to_hwpunit(*size_pt),
                    min_hu: pt_to_hwpunit(min_pt.unwrap_or(8.0)),
                });
            }
        }
    }

    Ok(Hwp5EditPlan {
        is_new_document,
        steps,
    })
}
