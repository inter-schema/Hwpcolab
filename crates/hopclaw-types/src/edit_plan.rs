use crate::ops::{
    BorderLineType, BorderSide, CellHAlign, CellVAlign, ColInsertPosition, EqualizeMode,
    HeaderFooterLocation, HighlightValue, ImageCellTarget, ListStyleMode, NestedTableWidthMode,
    PageNumberAlign, ParaIndexTarget, ParagraphAlign, RowInsertPosition, SealMode,
};
use crate::units::HwpUnit;
use serde::{Deserialize, Serialize};

/// Format-split intermediate representation compiled by `hopclaw-ops` and consumed
/// by `hopclaw-hwpx` or `hopclaw-hwp5`. All physical dimensions (`mm`, `pt`) are
/// resolved to integer `HwpUnit` during compilation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "format", rename_all = "snake_case")]
pub enum EditPlan {
    Hwpx(HwpxEditPlan),
    Hwp5(Hwp5EditPlan),
}

/// Compiled edit plan targeting an `.hwpx` (OWPML ZIP) container.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HwpxEditPlan {
    pub is_new_document: bool,
    pub steps: Vec<HwpxEditStep>,
}

/// Compiled edit plan targeting an `.hwp` (HWP 5.0 CFB) container.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hwp5EditPlan {
    pub is_new_document: bool,
    pub steps: Vec<Hwp5EditStep>,
}

/// Resolved HWP 5.0 table target carrying both canonical pre-order `tableIndex`
/// and optional `(sec, para, ctrl)` hints. Never defaults missing coordinates to `0,0,0`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hwp5TableTarget {
    pub table_index: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sec: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub para: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ctrl: Option<usize>,
}

/// Cell-anchored image placement target for HWP 5.0.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hwp5CellImageTarget {
    pub table: Hwp5TableTarget,
    pub row: usize,
    pub col: usize,
}

/// Single resolved operation in an [`HwpxEditPlan`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum HwpxEditStep {
    ReplaceText {
        find: String,
        replace: String,
    },
    SetParagraphText {
        index: usize,
        text: String,
    },
    AppendParagraph {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        align: Option<ParagraphAlign>,
    },
    AppendHeading {
        level: u8,
        text: String,
        size_hu: HwpUnit,
    },
    SetCellText {
        table_index: usize,
        row: usize,
        col: usize,
        text: String,
        fit: bool,
    },
    InsertTable {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        after_para_index: Option<isize>,
        headers: Vec<String>,
        rows: Vec<Vec<String>>,
    },
    InsertNestedTable {
        table_index: usize,
        row: usize,
        col: usize,
        rows: usize,
        cols: usize,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        data: Option<Vec<Vec<String>>>,
        header_row: bool,
        width_mode: NestedTableWidthMode,
    },
    InsertTableRow {
        table_index: usize,
        row: usize,
        position: RowInsertPosition,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cells: Option<Vec<String>>,
    },
    DeleteTableRow {
        table_index: usize,
        row: usize,
    },
    InsertTableCol {
        table_index: usize,
        col: usize,
        position: ColInsertPosition,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cells: Option<Vec<String>>,
    },
    DeleteTableCol {
        table_index: usize,
        col: usize,
    },
    MergeCells {
        table_index: usize,
        from_row: usize,
        from_col: usize,
        to_row: usize,
        to_col: usize,
    },
    UnmergeCells {
        table_index: usize,
        row: usize,
        col: usize,
    },
    SplitCell {
        table_index: usize,
        row: usize,
        col: usize,
        into_rows: usize,
        into_cols: usize,
    },
    EqualizeTable {
        table_index: usize,
        mode: EqualizeMode,
    },
    SetCellBackground {
        table_index: usize,
        row: usize,
        col: usize,
        color: String,
    },
    SetCellAlign {
        table_index: usize,
        row: usize,
        col: usize,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        end_row: Option<usize>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        end_col: Option<usize>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        horizontal: Option<CellHAlign>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        vertical: Option<CellVAlign>,
    },
    SetCellBorder {
        table_index: usize,
        row: usize,
        col: usize,
        sides: Vec<BorderSide>,
        color: String,
        width_hu: HwpUnit,
        line_type: BorderLineType,
    },
    ApplyTextStyle {
        target_text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        font_family: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        size_hu: Option<HwpUnit>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bold: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        italic: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        underline: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        color: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        highlight: Option<HighlightValue>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        letter_spacing: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        char_ratio: Option<f64>,
    },
    ApplyParagraphStyle {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        para_index: Option<ParaIndexTarget>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target_text: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        align: Option<ParagraphAlign>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        indent_hu: Option<HwpUnit>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        line_spacing_percent: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        spacing_before_hu: Option<HwpUnit>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        spacing_after_hu: Option<HwpUnit>,
    },
    SetListStyle {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        para_index: Option<usize>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target_text: Option<String>,
        mode: ListStyleMode,
    },
    InsertImage {
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        anchor_text: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        width_hu: Option<HwpUnit>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        height_hu: Option<HwpUnit>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cell: Option<ImageCellTarget>,
    },
    PlaceSeal {
        anchor_text: String,
        seal_image_path: String,
        mode: SealMode,
    },
    SetHeaderFooter {
        location: HeaderFooterLocation,
        text: String,
    },
    InsertPageNumber {
        location: HeaderFooterLocation,
        align: PageNumberAlign,
    },
    InsertFootnote {
        anchor_text: String,
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        para_index: Option<isize>,
    },
    SetColumns {
        count: u8,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        spacing_hu: Option<HwpUnit>,
    },
    SetColWidth {
        table_index: usize,
        col: usize,
        width_hu: HwpUnit,
        keep_table_width: bool,
    },
    SetRowHeight {
        table_index: usize,
        row: usize,
        height_hu: HwpUnit,
    },
    SetCellPadding {
        table_index: usize,
        row: usize,
        col: usize,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        left_hu: Option<HwpUnit>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        right_hu: Option<HwpUnit>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        top_hu: Option<HwpUnit>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bottom_hu: Option<HwpUnit>,
    },
    FitTableToPage {
        table_index: usize,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target_width_hu: Option<HwpUnit>,
    },
    SetPageMargin {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        left_hu: Option<HwpUnit>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        right_hu: Option<HwpUnit>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        top_hu: Option<HwpUnit>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bottom_hu: Option<HwpUnit>,
    },
    ShrinkCellFont {
        table_index: usize,
        row: usize,
        col: usize,
        size_hu: HwpUnit,
        min_hu: HwpUnit,
    },
}

/// Single resolved operation in an [`Hwp5EditPlan`].
/// Note: `insert_nested_table` is intentionally absent; HWP 5.0 compilation rejects it with `E_OP_UNSUPPORTED_FORMAT`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Hwp5EditStep {
    SetupDocument {
        width_hu: HwpUnit,
        height_hu: HwpUnit,
        margin_hu: HwpUnit,
    },
    ReplaceText {
        find: String,
        replace: String,
    },
    AppendParagraph {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        align: Option<ParagraphAlign>,
    },
    AppendHeading {
        level: u8,
        text: String,
        size_hu: HwpUnit,
    },
    SetCellText {
        target: Hwp5TableTarget,
        row: usize,
        col: usize,
        text: String,
        fit: bool,
    },
    AppendTable {
        headers: Vec<String>,
        rows: Vec<Vec<String>>,
    },
    InsertTableRow {
        target: Hwp5TableTarget,
        row: usize,
        position: RowInsertPosition,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cells: Option<Vec<String>>,
    },
    DeleteTableRow {
        target: Hwp5TableTarget,
        row: usize,
    },
    InsertTableCol {
        target: Hwp5TableTarget,
        col: usize,
        position: ColInsertPosition,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cells: Option<Vec<String>>,
    },
    DeleteTableCol {
        target: Hwp5TableTarget,
        col: usize,
    },
    MergeCells {
        target: Hwp5TableTarget,
        from_row: usize,
        from_col: usize,
        to_row: usize,
        to_col: usize,
    },
    UnmergeCells {
        target: Hwp5TableTarget,
        row: usize,
        col: usize,
    },
    SplitCell {
        target: Hwp5TableTarget,
        row: usize,
        col: usize,
        into_rows: usize,
        into_cols: usize,
    },
    EqualizeTable {
        target: Hwp5TableTarget,
        mode: EqualizeMode,
    },
    SetCellBackground {
        target: Hwp5TableTarget,
        row: usize,
        col: usize,
        color: String,
    },
    SetCellAlign {
        target: Hwp5TableTarget,
        row: usize,
        col: usize,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        end_row: Option<usize>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        end_col: Option<usize>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        horizontal: Option<CellHAlign>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        vertical: Option<CellVAlign>,
    },
    SetCellBorder {
        target: Hwp5TableTarget,
        row: usize,
        col: usize,
        sides: Vec<BorderSide>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        color: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        width_hu: Option<HwpUnit>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        line_type: Option<BorderLineType>,
    },
    ApplyTextStyle {
        target_text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        font_family: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        size_hu: Option<HwpUnit>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bold: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        italic: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        underline: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        color: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        highlight: Option<HighlightValue>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        letter_spacing: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        char_ratio: Option<f64>,
    },
    ApplyParagraphStyle {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        para_index: Option<ParaIndexTarget>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target_text: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        align: Option<ParagraphAlign>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        indent_hu: Option<HwpUnit>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        line_spacing_percent: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        spacing_before_hu: Option<HwpUnit>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        spacing_after_hu: Option<HwpUnit>,
    },
    SetListStyle {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        para_index: Option<usize>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target_text: Option<String>,
        mode: ListStyleMode,
    },
    InsertImage {
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        anchor_text: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        width_hu: Option<HwpUnit>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        height_hu: Option<HwpUnit>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cell: Option<Hwp5CellImageTarget>,
    },
    PlaceSeal {
        anchor_text: String,
        seal_image_path: String,
        mode: SealMode,
    },
    SetHeaderFooter {
        location: HeaderFooterLocation,
        text: String,
    },
    InsertPageNumber {
        location: HeaderFooterLocation,
        align: PageNumberAlign,
    },
    InsertFootnote {
        anchor_text: String,
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        para_index: Option<isize>,
    },
    SetColumns {
        count: u8,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        spacing_hu: Option<HwpUnit>,
    },
    SetColWidth {
        target: Hwp5TableTarget,
        col: usize,
        width_hu: HwpUnit,
        keep_table_width: bool,
    },
    SetRowHeight {
        target: Hwp5TableTarget,
        row: usize,
        height_hu: HwpUnit,
    },
    SetCellPadding {
        target: Hwp5TableTarget,
        row: usize,
        col: usize,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        left_hu: Option<HwpUnit>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        right_hu: Option<HwpUnit>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        top_hu: Option<HwpUnit>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bottom_hu: Option<HwpUnit>,
    },
    FitTableToPage {
        target: Hwp5TableTarget,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target_width_hu: Option<HwpUnit>,
    },
    SetPageMargin {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        left_hu: Option<HwpUnit>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        right_hu: Option<HwpUnit>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        top_hu: Option<HwpUnit>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bottom_hu: Option<HwpUnit>,
    },
    ShrinkCellFont {
        target: Hwp5TableTarget,
        row: usize,
        col: usize,
        size_hu: HwpUnit,
        min_hu: HwpUnit,
    },
}
