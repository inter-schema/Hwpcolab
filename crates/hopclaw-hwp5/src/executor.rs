use std::collections::BTreeMap;

use hopclaw_types::{
    ErrorCode, ErrorModel, Hwp5EditPlan, Hwp5EditStep, InspectDocumentResult, TableCoordinateMeta,
    TableWalker, UnifiedOp,
};

use crate::cfb_container::Hwp5Container;
use crate::docinfo::DocInfoDoc;
use crate::error::{Hwp5Error, Result};
use crate::layout;
use crate::objects::{self, GsoPlacement};
use crate::record::tag;
use crate::section::SectionDoc;
use crate::style::{self, ParaStyleRequest, TextStyleRequest};
use crate::table;
use crate::text;

/// Official blank A4 portrait `.hwp` template (generated from `rhwp` without `Sh33tJ5`).
pub const BLANK_HWP_TEMPLATE: &[u8] = include_bytes!("../../../assets/templates/blank.hwp");

/// Result of executing an [`Hwp5EditPlan`].
#[derive(Debug, Clone)]
pub struct Hwp5ExecOutput {
    pub bytes: Vec<u8>,
    pub steps_applied: usize,
    pub log: Vec<String>,
}

/// Convert crate-local [`Hwp5Error`] to canonical [`ErrorModel`].
pub fn error_to_model(err: Hwp5Error) -> ErrorModel {
    let code = match &err {
        Hwp5Error::TargetNotFound(_) => ErrorCode::E_TABLE_NOT_FOUND,
        Hwp5Error::UnsupportedFormat(_) => ErrorCode::E_OP_UNSUPPORTED_FORMAT,
        _ => ErrorCode::E_DOC_INVALID,
    };
    ErrorModel::new(code, err.to_string())
}

/// Reject `.hwp`-unsupported operations from [`UnifiedOp`] (e.g., `insert_nested_table` -> `E_OP_UNSUPPORTED_FORMAT`).
pub fn reject_unsupported_unified_op(op: &UnifiedOp) -> std::result::Result<(), ErrorModel> {
    if let UnifiedOp::InsertNestedTable { .. } = op {
        return Err(ErrorModel::new(
            ErrorCode::E_OP_UNSUPPORTED_FORMAT,
            "E_OP_UNSUPPORTED_FORMAT: insert_nested_table is not supported on binary .hwp (only .hwpx)",
        ));
    }
    Ok(())
}

/// Pure-Rust [`TableWalker`] and structure inspector for HWP 5.0 documents (`extract_text.js --inspect` compatible).
pub struct Hwp5Inspector {
    docinfo: DocInfoDoc,
    sections: Vec<SectionDoc>,
}

impl Hwp5Inspector {
    /// Open and decode all sections + `DocInfo` of an HWP 5.0 byte buffer.
    pub fn open_bytes(bytes: &[u8]) -> std::result::Result<Self, ErrorModel> {
        Self::open_inner(bytes).map_err(error_to_model)
    }

    fn open_inner(bytes: &[u8]) -> Result<Self> {
        let mut container = Hwp5Container::open_bytes(bytes)?;
        let docinfo = DocInfoDoc::parse(&container.read_docinfo()?)?;
        let sec_paths = container.section_paths();
        let mut sections = Vec::with_capacity(sec_paths.len());
        for path in sec_paths {
            let plain = container.read_record_stream(&path)?;
            sections.push(SectionDoc::parse(&plain)?);
        }
        Ok(Self { docinfo, sections })
    }

    /// Produce an [`InspectDocumentResult`] matching `extract_text.js --inspect [--with-cell-text]`.
    pub fn inspect(&self, with_cell_text: bool) -> std::result::Result<InspectDocumentResult, ErrorModel> {
        let tables = self.walk_tables()?;
        let paragraph_count: usize = self
            .sections
            .iter()
            .map(|s| s.top_level_para_indices().len())
            .sum();
        let table_count = tables.len();
        let cell_count: usize = tables.iter().map(|t| t.cells.len()).sum();
        let image_count = self.docinfo.bodies_by_tag(tag::BIN_DATA).len();
        Ok(InspectDocumentResult {
            file_type: "hwp".to_string(),
            section_count: self.sections.len(),
            paragraph_count,
            table_count,
            cell_count,
            image_count: Some(image_count),
            tables: if with_cell_text { Some(tables) } else { None },
        })
    }
}

impl TableWalker for Hwp5Inspector {
    fn walk_tables(&self) -> std::result::Result<Vec<TableCoordinateMeta>, ErrorModel> {
        let mut tables = Vec::new();
        for (sec_idx, sec) in self.sections.iter().enumerate() {
            table::walk_section_tables(sec, sec_idx, &mut tables).map_err(error_to_model)?;
        }
        Ok(tables)
    }
}

/// Inspect an HWP 5.0 byte slice directly (`extract_text.js --inspect` replacement).
pub fn inspect_hwp5(
    bytes: &[u8],
    with_cell_text: bool,
) -> std::result::Result<InspectDocumentResult, ErrorModel> {
    Hwp5Inspector::open_bytes(bytes)?.inspect(with_cell_text)
}

/// Execute an [`Hwp5EditPlan`] purely in Rust (opening `base_bytes` or `BLANK_HWP_TEMPLATE`).
pub fn execute_hwp5_plan(
    base_bytes: Option<&[u8]>,
    plan: &Hwp5EditPlan,
) -> std::result::Result<Hwp5ExecOutput, ErrorModel> {
    execute_inner(base_bytes, plan).map_err(error_to_model)
}

fn execute_inner(base_bytes: Option<&[u8]>, plan: &Hwp5EditPlan) -> Result<Hwp5ExecOutput> {
    let source = match base_bytes {
        Some(b) if !plan.is_new_document => b,
        _ => BLANK_HWP_TEMPLATE,
    };
    let mut container = Hwp5Container::open_bytes(source)?;
    let mut docinfo = DocInfoDoc::parse(&container.read_docinfo()?)?;

    // Lazy cache of parsed SectionDoc streams so multi-section targets stay clean.
    let mut sections: BTreeMap<usize, SectionDoc> = BTreeMap::new();
    let sec0 = SectionDoc::parse(&container.read_section(0)?)?;
    sections.insert(0, sec0);

    let mut image_changed = false;
    let mut log = Vec::with_capacity(plan.steps.len());

    for (idx, step) in plan.steps.iter().enumerate() {
        let sec_idx = target_section_for_step(step);
        if let std::collections::btree_map::Entry::Vacant(e) = sections.entry(sec_idx) {
            let loaded = SectionDoc::parse(&container.read_section(sec_idx)?)?;
            e.insert(loaded);
        }
        let sec = sections.get_mut(&sec_idx).unwrap();
        let entry = apply_step(&mut container, &mut docinfo, sec, step, &mut image_changed)
            .map_err(|err| match err {
                Hwp5Error::TargetNotFound(msg) => {
                    Hwp5Error::TargetNotFound(format!("step {idx}: {msg}"))
                }
                Hwp5Error::UnsupportedFormat(msg) => {
                    Hwp5Error::UnsupportedFormat(format!("step {idx}: {msg}"))
                }
                other => other,
            })?;
        log.push(entry);
    }

    // Write modified DocInfo & normalized sections back cleanly
    docinfo.sync_id_mappings()?;
    container.write_docinfo(&docinfo.serialize()?)?;
    for (sec_idx, mut sec) in sections {
        sec.normalize_last_para_flags()?;
        container.write_section(sec_idx, &sec.serialize()?)?;
    }

    // Invalidate PrvImage thumbnail if images/seals were embedded (avoid stale preview leak)
    if image_changed && container.has_stream("/PrvImage") {
        container.write_raw_stream("/PrvImage", &[])?;
    }

    let bytes = container.into_bytes()?;
    Ok(Hwp5ExecOutput {
        bytes,
        steps_applied: plan.steps.len(),
        log,
    })
}

fn target_section_for_step(step: &Hwp5EditStep) -> usize {
    match step {
        Hwp5EditStep::SetCellText { target, .. }
        | Hwp5EditStep::InsertTableRow { target, .. }
        | Hwp5EditStep::DeleteTableRow { target, .. }
        | Hwp5EditStep::InsertTableCol { target, .. }
        | Hwp5EditStep::DeleteTableCol { target, .. }
        | Hwp5EditStep::MergeCells { target, .. }
        | Hwp5EditStep::UnmergeCells { target, .. }
        | Hwp5EditStep::SplitCell { target, .. }
        | Hwp5EditStep::EqualizeTable { target, .. }
        | Hwp5EditStep::SetCellBackground { target, .. }
        | Hwp5EditStep::SetCellAlign { target, .. }
        | Hwp5EditStep::SetCellBorder { target, .. }
        | Hwp5EditStep::SetColWidth { target, .. }
        | Hwp5EditStep::SetRowHeight { target, .. }
        | Hwp5EditStep::SetCellPadding { target, .. }
        | Hwp5EditStep::FitTableToPage { target, .. }
        | Hwp5EditStep::ShrinkCellFont { target, .. } => target.sec.unwrap_or(0),
        Hwp5EditStep::InsertImage {
            cell: Some(cell), ..
        } => cell.table.sec.unwrap_or(0),
        _ => 0,
    }
}

fn apply_step(
    container: &mut Hwp5Container,
    docinfo: &mut DocInfoDoc,
    sec: &mut SectionDoc,
    step: &Hwp5EditStep,
    image_changed: &mut bool,
) -> Result<String> {
    match step {
        Hwp5EditStep::SetupDocument {
            width_hu,
            height_hu,
            margin_hu,
        } => {
            layout::setup_document(sec, *width_hu, *height_hu, *margin_hu)?;
            Ok(format!(
                "setup_document: {width_hu}x{height_hu} margin={margin_hu}"
            ))
        }
        Hwp5EditStep::ReplaceText { find, replace } => {
            let count = text::replace_text(sec, find, replace)?;
            Ok(format!("replace_text: '{find}' -> '{replace}' ({count})"))
        }
        Hwp5EditStep::AppendParagraph { text: txt, align } => {
            let idx = text::append_paragraph_with_char_shape(docinfo, sec, txt, *align, None)?;
            Ok(format!("append_paragraph: para={idx}"))
        }
        Hwp5EditStep::AppendHeading {
            level,
            text: txt,
            size_hu,
        } => {
            let idx = text::append_heading(docinfo, sec, *level, txt, *size_hu)?;
            Ok(format!("append_heading: level={level} para={idx}"))
        }
        Hwp5EditStep::SetCellText {
            target,
            row,
            col,
            text: txt,
            fit,
        } => {
            table::set_cell_text(docinfo, sec, target, *row, *col, txt, *fit)?;
            Ok(format!("set_cell_text: ({row},{col})"))
        }
        Hwp5EditStep::AppendTable { headers, rows } => {
            let idx = table::append_table(docinfo, sec, headers, rows)?;
            Ok(format!("append_table: table={idx}"))
        }
        Hwp5EditStep::InsertTableRow {
            target,
            row,
            position,
            cells,
        } => {
            table::insert_table_row(docinfo, sec, target, *row, *position, cells.as_deref())?;
            Ok(format!("insert_table_row: row={row}"))
        }
        Hwp5EditStep::DeleteTableRow { target, row } => {
            table::delete_table_row(sec, target, *row)?;
            Ok(format!("delete_table_row: row={row}"))
        }
        Hwp5EditStep::InsertTableCol {
            target,
            col,
            position,
            cells,
        } => {
            table::insert_table_col(docinfo, sec, target, *col, *position, cells.as_deref())?;
            Ok(format!("insert_table_col: col={col}"))
        }
        Hwp5EditStep::DeleteTableCol { target, col } => {
            table::delete_table_col(sec, target, *col)?;
            Ok(format!("delete_table_col: col={col}"))
        }
        Hwp5EditStep::MergeCells {
            target,
            from_row,
            from_col,
            to_row,
            to_col,
        } => {
            table::merge_cells(sec, target, *from_row, *from_col, *to_row, *to_col)?;
            Ok(format!("merge_cells: ({from_row},{from_col})..({to_row},{to_col})"))
        }
        Hwp5EditStep::UnmergeCells { target, row, col } => {
            table::unmerge_cells(sec, target, *row, *col)?;
            Ok(format!("unmerge_cells: ({row},{col})"))
        }
        Hwp5EditStep::SplitCell {
            target,
            row,
            col,
            into_rows,
            into_cols,
        } => {
            table::split_cell(sec, target, *row, *col, *into_rows, *into_cols)?;
            Ok(format!("split_cell: ({row},{col}) {into_rows}x{into_cols}"))
        }
        Hwp5EditStep::EqualizeTable { target, mode } => {
            table::equalize_table(sec, target, *mode)?;
            Ok("equalize_table".to_string())
        }
        Hwp5EditStep::SetCellBackground {
            target,
            row,
            col,
            color,
        } => {
            table::set_cell_background(docinfo, sec, target, *row, *col, color)?;
            Ok(format!("set_cell_background: ({row},{col}) {color}"))
        }
        Hwp5EditStep::SetCellAlign {
            target,
            row,
            col,
            end_row,
            end_col,
            horizontal,
            vertical,
        } => {
            table::set_cell_align(
                docinfo,
                sec,
                target,
                *row,
                *col,
                *end_row,
                *end_col,
                *horizontal,
                *vertical,
            )?;
            Ok(format!("set_cell_align: ({row},{col})"))
        }
        Hwp5EditStep::SetCellBorder {
            target,
            row,
            col,
            sides,
            color,
            width_hu,
            line_type,
        } => {
            table::set_cell_border(
                docinfo,
                sec,
                target,
                *row,
                *col,
                sides,
                color.as_deref(),
                *width_hu,
                *line_type,
            )?;
            Ok(format!("set_cell_border: ({row},{col})"))
        }
        Hwp5EditStep::ApplyTextStyle {
            target_text,
            font_family,
            size_hu,
            bold,
            italic,
            underline,
            color,
            highlight,
            letter_spacing,
            char_ratio,
        } => {
            style::apply_text_style(
                docinfo,
                sec,
                &TextStyleRequest {
                    target_text,
                    font_family: font_family.as_deref(),
                    size_hu: *size_hu,
                    bold: *bold,
                    italic: *italic,
                    underline: *underline,
                    color: color.as_deref(),
                    highlight: highlight.as_ref(),
                    letter_spacing: *letter_spacing,
                    char_ratio: *char_ratio,
                },
            )?;
            Ok(format!("apply_text_style: '{target_text}'"))
        }
        Hwp5EditStep::ApplyParagraphStyle {
            para_index,
            target_text,
            align,
            indent_hu,
            line_spacing_percent,
            spacing_before_hu,
            spacing_after_hu,
        } => {
            style::apply_paragraph_style(
                docinfo,
                sec,
                &ParaStyleRequest {
                    para_index: *para_index,
                    target_text: target_text.as_deref(),
                    align: *align,
                    indent_hu: *indent_hu,
                    line_spacing_percent: *line_spacing_percent,
                    spacing_before_hu: *spacing_before_hu,
                    spacing_after_hu: *spacing_after_hu,
                },
            )?;
            Ok("apply_paragraph_style".to_string())
        }
        Hwp5EditStep::SetListStyle {
            para_index,
            target_text,
            mode,
        } => {
            text::set_list_style(docinfo, sec, *para_index, target_text.as_deref(), *mode)?;
            Ok("set_list_style".to_string())
        }
        Hwp5EditStep::InsertImage {
            path,
            anchor_text,
            width_hu,
            height_hu,
            cell,
        } => {
            objects::insert_image(
                container,
                docinfo,
                sec,
                path,
                anchor_text.as_deref(),
                *width_hu,
                *height_hu,
                cell.as_ref(),
                GsoPlacement::Inline,
            )?;
            *image_changed = true;
            Ok(format!("insert_image: '{path}'"))
        }
        Hwp5EditStep::PlaceSeal {
            anchor_text,
            seal_image_path,
            mode,
        } => {
            objects::place_seal(container, docinfo, sec, anchor_text, seal_image_path, *mode)?;
            *image_changed = true;
            Ok(format!("place_seal: '{anchor_text}'"))
        }
        Hwp5EditStep::SetHeaderFooter { location, text: txt } => {
            layout::set_header_footer(docinfo, sec, *location, txt)?;
            Ok("set_header_footer".to_string())
        }
        Hwp5EditStep::InsertPageNumber { location, align } => {
            layout::insert_page_number(docinfo, sec, *location, *align)?;
            Ok("insert_page_number".to_string())
        }
        Hwp5EditStep::InsertFootnote {
            anchor_text,
            text: txt,
            para_index,
        } => {
            layout::insert_footnote(docinfo, sec, anchor_text, txt, *para_index)?;
            Ok(format!("insert_footnote: '{anchor_text}'"))
        }
        Hwp5EditStep::SetColumns { count, spacing_hu } => {
            layout::set_columns(sec, *count, *spacing_hu)?;
            Ok(format!("set_columns: count={count}"))
        }
        Hwp5EditStep::SetColWidth {
            target,
            col,
            width_hu,
            keep_table_width,
        } => {
            table::set_col_width(sec, target, *col, *width_hu, *keep_table_width)?;
            Ok(format!("set_col_width: col={col}"))
        }
        Hwp5EditStep::SetRowHeight {
            target,
            row,
            height_hu,
        } => {
            table::set_row_height(sec, target, *row, *height_hu)?;
            Ok(format!("set_row_height: row={row}"))
        }
        Hwp5EditStep::SetCellPadding {
            target,
            row,
            col,
            left_hu,
            right_hu,
            top_hu,
            bottom_hu,
        } => {
            table::set_cell_padding(
                sec,
                target,
                *row,
                *col,
                *left_hu,
                *right_hu,
                *top_hu,
                *bottom_hu,
            )?;
            Ok(format!("set_cell_padding: ({row},{col})"))
        }
        Hwp5EditStep::FitTableToPage {
            target,
            target_width_hu,
        } => {
            table::fit_table_to_page(sec, target, *target_width_hu)?;
            Ok("fit_table_to_page".to_string())
        }
        Hwp5EditStep::SetPageMargin {
            left_hu,
            right_hu,
            top_hu,
            bottom_hu,
        } => {
            layout::set_page_margin(sec, *left_hu, *right_hu, *top_hu, *bottom_hu)?;
            Ok("set_page_margin".to_string())
        }
        Hwp5EditStep::ShrinkCellFont {
            target,
            row,
            col,
            size_hu,
            min_hu,
        } => {
            table::shrink_cell_font(docinfo, sec, target, *row, *col, *size_hu, *min_hu)?;
            Ok(format!("shrink_cell_font: ({row},{col})"))
        }
    }
}
