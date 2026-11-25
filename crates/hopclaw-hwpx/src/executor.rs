use hopclaw_types::{ErrorModel, HwpxEditPlan, HwpxEditStep};

use crate::error::HwpxError;
use crate::model::HwpxDocument;
use crate::ops::objects::{
    op_insert_footnote, op_insert_image, op_insert_page_number, op_place_seal, op_set_columns,
    op_set_header_footer, op_set_page_margin,
};
use crate::ops::table::{
    op_delete_table_col, op_delete_table_row, op_equalize_table, op_fit_table_to_page,
    op_insert_nested_table, op_insert_table, op_insert_table_col, op_insert_table_row,
    op_merge_cells, op_set_cell_align, op_set_cell_background, op_set_cell_border,
    op_set_cell_padding, op_set_cell_text, op_set_col_width, op_set_row_height,
    op_shrink_cell_font, op_split_cell, op_unmerge_cells,
};
use crate::ops::text::{
    op_append_heading, op_append_paragraph, op_apply_paragraph_style, op_apply_text_style,
    op_replace_text, op_set_list_style, op_set_paragraph_text, TextStyleOpts,
};
use crate::BLANK_HWPX_TEMPLATE;

/// Execute a compiled [`HwpxEditPlan`] on `.hwpx` document bytes (or [`BLANK_HWPX_TEMPLATE`]
/// when `plan.is_new_document` or `bytes` is empty) and return valid, repackaged `.hwpx` bytes.
pub fn execute_hwpx_edit_plan(bytes: &[u8], plan: &HwpxEditPlan) -> Result<Vec<u8>, HwpxError> {
    let source = if plan.is_new_document || bytes.is_empty() {
        BLANK_HWPX_TEMPLATE.to_vec()
    } else {
        bytes.to_vec()
    };

    let mut doc = HwpxDocument::open(source)?;
    for step in &plan.steps {
        execute_step(&mut doc, step)?;
    }
    doc.sync_header_counts()?;
    doc.archive.save()
}

/// Convenience wrapper returning [`ErrorModel`] for IPC and orchestrator callers.
pub fn execute_hwpx_edit_plan_model(
    bytes: &[u8],
    plan: &HwpxEditPlan,
) -> Result<Vec<u8>, ErrorModel> {
    execute_hwpx_edit_plan(bytes, plan).map_err(ErrorModel::from)
}

fn execute_step(doc: &mut HwpxDocument, step: &HwpxEditStep) -> Result<(), HwpxError> {
    match step {
        HwpxEditStep::ReplaceText { find, replace } => {
            op_replace_text(doc, find, replace)?;
        }
        HwpxEditStep::SetParagraphText { index, text } => {
            op_set_paragraph_text(doc, *index, text)?;
        }
        HwpxEditStep::AppendParagraph { text, align } => {
            op_append_paragraph(doc, text, *align)?;
        }
        HwpxEditStep::AppendHeading {
            level,
            text,
            size_hu,
        } => {
            op_append_heading(doc, *level, text, *size_hu)?;
        }
        HwpxEditStep::SetCellText {
            table_index,
            row,
            col,
            text,
            fit,
        } => {
            op_set_cell_text(doc, *table_index, *row, *col, text, *fit)?;
        }
        HwpxEditStep::InsertTable {
            after_para_index,
            headers,
            rows,
        } => {
            op_insert_table(doc, *after_para_index, headers, rows)?;
        }
        HwpxEditStep::InsertNestedTable {
            table_index,
            row,
            col,
            rows,
            cols,
            data,
            header_row,
            width_mode,
        } => {
            op_insert_nested_table(
                doc,
                *table_index,
                *row,
                *col,
                *rows,
                *cols,
                data.as_deref(),
                *header_row,
                *width_mode,
            )?;
        }
        HwpxEditStep::InsertTableRow {
            table_index,
            row,
            position,
            cells,
        } => {
            op_insert_table_row(doc, *table_index, *row, *position, cells.as_deref())?;
        }
        HwpxEditStep::DeleteTableRow { table_index, row } => {
            op_delete_table_row(doc, *table_index, *row)?;
        }
        HwpxEditStep::InsertTableCol {
            table_index,
            col,
            position,
            cells,
        } => {
            op_insert_table_col(doc, *table_index, *col, *position, cells.as_deref())?;
        }
        HwpxEditStep::DeleteTableCol { table_index, col } => {
            op_delete_table_col(doc, *table_index, *col)?;
        }
        HwpxEditStep::MergeCells {
            table_index,
            from_row,
            from_col,
            to_row,
            to_col,
        } => {
            op_merge_cells(doc, *table_index, *from_row, *from_col, *to_row, *to_col)?;
        }
        HwpxEditStep::UnmergeCells {
            table_index,
            row,
            col,
        } => {
            op_unmerge_cells(doc, *table_index, *row, *col)?;
        }
        HwpxEditStep::SplitCell {
            table_index,
            row,
            col,
            into_rows,
            into_cols,
        } => {
            op_split_cell(doc, *table_index, *row, *col, *into_rows, *into_cols)?;
        }
        HwpxEditStep::EqualizeTable { table_index, mode } => {
            op_equalize_table(doc, *table_index, *mode)?;
        }
        HwpxEditStep::SetCellBackground {
            table_index,
            row,
            col,
            color,
        } => {
            op_set_cell_background(doc, *table_index, *row, *col, color)?;
        }
        HwpxEditStep::SetCellAlign {
            table_index,
            row,
            col,
            end_row,
            end_col,
            horizontal,
            vertical,
        } => {
            op_set_cell_align(
                doc,
                *table_index,
                *row,
                *col,
                *end_row,
                *end_col,
                *horizontal,
                *vertical,
            )?;
        }
        HwpxEditStep::SetCellBorder {
            table_index,
            row,
            col,
            sides,
            color,
            width_hu,
            line_type,
        } => {
            op_set_cell_border(
                doc,
                *table_index,
                *row,
                *col,
                sides,
                color,
                *width_hu,
                *line_type,
            )?;
        }
        HwpxEditStep::ApplyTextStyle {
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
            op_apply_text_style(
                doc,
                target_text,
                TextStyleOpts {
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
        }
        HwpxEditStep::ApplyParagraphStyle {
            para_index,
            target_text,
            align,
            indent_hu,
            line_spacing_percent,
            spacing_before_hu,
            spacing_after_hu,
        } => {
            op_apply_paragraph_style(
                doc,
                *para_index,
                target_text.as_deref(),
                *align,
                *indent_hu,
                *line_spacing_percent,
                *spacing_before_hu,
                *spacing_after_hu,
            )?;
        }
        HwpxEditStep::SetListStyle {
            para_index,
            target_text,
            mode,
        } => {
            op_set_list_style(doc, *para_index, target_text.as_deref(), *mode)?;
        }
        HwpxEditStep::InsertImage {
            path,
            anchor_text,
            width_hu,
            height_hu,
            cell,
        } => {
            op_insert_image(
                doc,
                path,
                anchor_text.as_deref(),
                *width_hu,
                *height_hu,
                cell.as_ref(),
            )?;
        }
        HwpxEditStep::PlaceSeal {
            anchor_text,
            seal_image_path,
            mode,
        } => {
            op_place_seal(doc, anchor_text, seal_image_path, *mode)?;
        }
        HwpxEditStep::SetHeaderFooter { location, text } => {
            op_set_header_footer(doc, *location, text)?;
        }
        HwpxEditStep::InsertPageNumber { location, align } => {
            op_insert_page_number(doc, *location, *align)?;
        }
        HwpxEditStep::InsertFootnote {
            anchor_text,
            text,
            para_index,
        } => {
            op_insert_footnote(doc, anchor_text, text, *para_index)?;
        }
        HwpxEditStep::SetColumns { count, spacing_hu } => {
            op_set_columns(doc, *count, *spacing_hu)?;
        }
        HwpxEditStep::SetColWidth {
            table_index,
            col,
            width_hu,
            keep_table_width,
        } => {
            op_set_col_width(doc, *table_index, *col, *width_hu, *keep_table_width)?;
        }
        HwpxEditStep::SetRowHeight {
            table_index,
            row,
            height_hu,
        } => {
            op_set_row_height(doc, *table_index, *row, *height_hu)?;
        }
        HwpxEditStep::SetCellPadding {
            table_index,
            row,
            col,
            left_hu,
            right_hu,
            top_hu,
            bottom_hu,
        } => {
            op_set_cell_padding(
                doc,
                *table_index,
                *row,
                *col,
                *left_hu,
                *right_hu,
                *top_hu,
                *bottom_hu,
            )?;
        }
        HwpxEditStep::FitTableToPage {
            table_index,
            target_width_hu,
        } => {
            op_fit_table_to_page(doc, *table_index, *target_width_hu)?;
        }
        HwpxEditStep::SetPageMargin {
            left_hu,
            right_hu,
            top_hu,
            bottom_hu,
        } => {
            op_set_page_margin(doc, *left_hu, *right_hu, *top_hu, *bottom_hu)?;
        }
        HwpxEditStep::ShrinkCellFont {
            table_index,
            row,
            col,
            size_hu,
            min_hu,
        } => {
            op_shrink_cell_font(doc, *table_index, *row, *col, *size_hu, *min_hu)?;
        }
    }
    Ok(())
}
