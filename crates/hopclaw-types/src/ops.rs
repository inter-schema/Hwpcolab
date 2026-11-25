use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Paragraph horizontal alignment (`append_paragraph`, `apply_paragraph_style`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ParagraphAlign {
    #[serde(rename = "LEFT", alias = "left")]
    Left,
    #[serde(rename = "CENTER", alias = "center")]
    Center,
    #[serde(rename = "RIGHT", alias = "right")]
    Right,
    #[serde(rename = "JUSTIFY", alias = "justify")]
    Justify,
    #[serde(rename = "DISTRIBUTE", alias = "distribute")]
    Distribute,
}

/// Nested table width mode (`insert_nested_table`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NestedTableWidthMode {
    #[serde(rename = "fit_cell")]
    FitCell,
}

/// Row insertion direction (`insert_table_row`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RowInsertPosition {
    Above,
    Below,
}

/// Column insertion direction (`insert_table_col`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ColInsertPosition {
    Left,
    Right,
}

/// Table equalization mode (`equalize_table`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EqualizeMode {
    Width,
    Height,
    Both,
}

/// Cell horizontal alignment (`set_cell_align`). Accepts both uppercase and lowercase on input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CellHAlign {
    #[serde(rename = "LEFT", alias = "left")]
    Left,
    #[serde(rename = "CENTER", alias = "center")]
    Center,
    #[serde(rename = "RIGHT", alias = "right")]
    Right,
    #[serde(rename = "JUSTIFY", alias = "justify")]
    Justify,
}

/// Cell vertical alignment (`set_cell_align`). Accepts both uppercase and lowercase on input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CellVAlign {
    #[serde(rename = "TOP", alias = "top")]
    Top,
    #[serde(rename = "CENTER", alias = "center")]
    Center,
    #[serde(rename = "MIDDLE", alias = "middle")]
    Middle,
    #[serde(rename = "BOTTOM", alias = "bottom")]
    Bottom,
}

/// Individual cell border side (`set_cell_border`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BorderSide {
    Top,
    Bottom,
    Left,
    Right,
}

/// Border sides target (`Array<"top" | "bottom" | "left" | "right"> | "all"`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BorderSides {
    All,
    Sides(Vec<BorderSide>),
}

impl BorderSides {
    /// Expand to concrete individual sides in canonical order.
    pub fn expand(&self) -> Vec<BorderSide> {
        match self {
            Self::All => vec![
                BorderSide::Left,
                BorderSide::Right,
                BorderSide::Top,
                BorderSide::Bottom,
            ],
            Self::Sides(list) => list.clone(),
        }
    }
}

impl Serialize for BorderSides {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::All => serializer.serialize_str("all"),
            Self::Sides(list) => list.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for BorderSides {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Helper {
            Str(String),
            List(Vec<BorderSide>),
        }

        match Helper::deserialize(deserializer)? {
            Helper::Str(s) if s.eq_ignore_ascii_case("all") => Ok(Self::All),
            Helper::Str(s) => Err(serde::de::Error::custom(format!(
                "invalid BorderSides string '{s}'; expected 'all' or array of sides"
            ))),
            Helper::List(list) => Ok(Self::Sides(list)),
        }
    }
}

/// Cell border line pattern (`set_cell_border`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BorderLineType {
    Solid,
    Dashed,
    Dotted,
    Double,
}

/// Highlight value (`string | boolean` in `apply_text_style`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum HighlightValue {
    Bool(bool),
    Color(String),
}

/// Paragraph index target (`number | "last"` in `apply_paragraph_style`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParaIndexTarget {
    Index(isize),
    Last,
}

impl Serialize for ParaIndexTarget {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Index(idx) => serializer.serialize_i64(*idx as i64),
            Self::Last => serializer.serialize_str("last"),
        }
    }
}

impl<'de> Deserialize<'de> for ParaIndexTarget {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Helper {
            Num(isize),
            Str(String),
        }

        match Helper::deserialize(deserializer)? {
            Helper::Num(n) => Ok(Self::Index(n)),
            Helper::Str(s) if s.eq_ignore_ascii_case("last") => Ok(Self::Last),
            Helper::Str(s) => Err(serde::de::Error::custom(format!(
                "invalid paraIndex '{s}'; expected number or 'last'"
            ))),
        }
    }
}

/// List formatting mode (`set_list_style`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ListStyleMode {
    Numbered,
    Bullet,
}

/// Target cell for `insert_image`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageCellTarget {
    pub table_index: usize,
    pub row: usize,
    pub col: usize,
}

/// Seal placement mode (`place_seal`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SealMode {
    Auto,
    Overlap,
    Right,
}

/// Header or footer placement (`set_header_footer`, `insert_page_number`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HeaderFooterLocation {
    Header,
    Footer,
}

/// Page number horizontal alignment (`insert_page_number`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PageNumberAlign {
    Left,
    Center,
    Right,
}

/// All 33 `UnifiedOp` variants matching `src/types/operations.ts` (`tag = "type"`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum UnifiedOp {
    // 1. 본문 텍스트 치환 및 문단 조작
    #[serde(rename = "replace_body_text", rename_all = "camelCase")]
    ReplaceBodyText { find: String, replace: String },

    #[serde(rename = "set_paragraph_text", rename_all = "camelCase")]
    SetParagraphText {
        para_index: usize,
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        old_text_for_hwp: Option<String>,
    },

    #[serde(rename = "append_paragraph", rename_all = "camelCase")]
    AppendParagraph {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        align: Option<ParagraphAlign>,
    },

    #[serde(rename = "append_heading", rename_all = "camelCase")]
    AppendHeading { level: u8, text: String },

    // 2. 표 셀 값 채우기 및 수정
    #[serde(rename = "set_cell_text", rename_all = "camelCase")]
    SetCellText {
        table_index: usize,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sec: Option<usize>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        para: Option<usize>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ctrl: Option<usize>,
        row: usize,
        col: usize,
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fit: Option<bool>,
    },

    // 3. 표 구조 제어
    #[serde(rename = "insert_table", rename_all = "camelCase")]
    InsertTable {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        after_para_index: Option<isize>,
        headers: Vec<String>,
        rows: Vec<Vec<String>>,
    },

    #[serde(rename = "insert_nested_table", rename_all = "camelCase")]
    InsertNestedTable {
        table_index: usize,
        row: usize,
        col: usize,
        rows: usize,
        cols: usize,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        data: Option<Vec<Vec<String>>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        header_row: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        width_mode: Option<NestedTableWidthMode>,
    },

    #[serde(rename = "insert_table_row", rename_all = "camelCase")]
    InsertTableRow {
        table_index: usize,
        row: usize,
        position: RowInsertPosition,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cells: Option<Vec<String>>,
    },

    #[serde(rename = "delete_table_row", rename_all = "camelCase")]
    DeleteTableRow { table_index: usize, row: usize },

    #[serde(rename = "insert_table_col", rename_all = "camelCase")]
    InsertTableCol {
        table_index: usize,
        col: usize,
        position: ColInsertPosition,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cells: Option<Vec<String>>,
    },

    #[serde(rename = "delete_table_col", rename_all = "camelCase")]
    DeleteTableCol { table_index: usize, col: usize },

    #[serde(rename = "merge_cells", rename_all = "camelCase")]
    MergeCells {
        table_index: usize,
        from_row: usize,
        from_col: usize,
        to_row: usize,
        to_col: usize,
    },

    #[serde(rename = "unmerge_cells", rename_all = "camelCase")]
    UnmergeCells {
        table_index: usize,
        row: usize,
        col: usize,
    },

    #[serde(rename = "split_cell", rename_all = "camelCase")]
    SplitCell {
        table_index: usize,
        row: usize,
        col: usize,
        into_rows: usize,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        into_cols: Option<usize>,
    },

    #[serde(rename = "equalize_table", rename_all = "camelCase")]
    EqualizeTable {
        table_index: usize,
        mode: EqualizeMode,
    },

    // 4. 표 셀 스타일링
    #[serde(rename = "set_cell_background", rename_all = "camelCase")]
    SetCellBackground {
        table_index: usize,
        row: usize,
        col: usize,
        color: String,
    },

    #[serde(rename = "set_cell_align", rename_all = "camelCase")]
    SetCellAlign {
        table_index: usize,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sec: Option<usize>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        para: Option<usize>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ctrl: Option<usize>,
        row: usize,
        col: usize,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        end_row: Option<usize>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        end_col: Option<usize>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        align: Option<CellHAlign>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        horizontal: Option<CellHAlign>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        valign: Option<CellVAlign>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        vertical: Option<CellVAlign>,
    },

    #[serde(rename = "set_cell_border", rename_all = "camelCase")]
    SetCellBorder {
        table_index: usize,
        row: usize,
        col: usize,
        sides: BorderSides,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        color: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        width_mm: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        line_type: Option<BorderLineType>,
    },

    // 5. 문단 및 글자 서식
    #[serde(rename = "apply_text_style", rename_all = "camelCase")]
    ApplyTextStyle {
        target_text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        font_family: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        size_pt: Option<f64>,
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

    #[serde(rename = "apply_paragraph_style", rename_all = "camelCase")]
    ApplyParagraphStyle {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        para_index: Option<ParaIndexTarget>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target_text: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        align: Option<ParagraphAlign>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        indent_hu: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        line_spacing_percent: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        spacing_before_hu: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        spacing_after_hu: Option<f64>,
    },

    #[serde(rename = "set_list_style", rename_all = "camelCase")]
    SetListStyle {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        para_index: Option<usize>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target_text: Option<String>,
        mode: ListStyleMode,
    },

    // 6. 개체 및 페이지 설정
    #[serde(rename = "insert_image", rename_all = "camelCase")]
    InsertImage {
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        anchor_text: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        width_mm: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        height_mm: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cell: Option<ImageCellTarget>,
    },

    #[serde(rename = "place_seal", rename_all = "camelCase")]
    PlaceSeal {
        anchor_text: String,
        seal_image_path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mode: Option<SealMode>,
    },

    #[serde(rename = "set_header_footer", rename_all = "camelCase")]
    SetHeaderFooter {
        #[serde(rename = "where")]
        location: HeaderFooterLocation,
        text: String,
    },

    #[serde(rename = "insert_page_number", rename_all = "camelCase")]
    InsertPageNumber {
        #[serde(rename = "where", default, skip_serializing_if = "Option::is_none")]
        location: Option<HeaderFooterLocation>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        align: Option<PageNumberAlign>,
    },

    #[serde(rename = "insert_footnote", rename_all = "camelCase")]
    InsertFootnote {
        anchor_text: String,
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        para_index: Option<isize>,
    },

    #[serde(rename = "set_columns", rename_all = "camelCase")]
    SetColumns {
        count: u8,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        spacing_mm: Option<f64>,
    },

    // 7. 표 및 페이지 레이아웃 치수 제어 (신규 6종)
    #[serde(rename = "set_col_width", rename_all = "camelCase")]
    SetColWidth {
        table_index: usize,
        col: usize,
        width_mm: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        keep_table_width: Option<bool>,
    },

    #[serde(rename = "set_row_height", rename_all = "camelCase")]
    SetRowHeight {
        table_index: usize,
        row: usize,
        height_mm: f64,
    },

    #[serde(rename = "set_cell_padding", rename_all = "camelCase")]
    SetCellPadding {
        table_index: usize,
        row: usize,
        col: usize,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        left_mm: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        right_mm: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        top_mm: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bottom_mm: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        margin_mm: Option<f64>,
    },

    #[serde(rename = "fit_table_to_page", rename_all = "camelCase")]
    FitTableToPage {
        table_index: usize,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target_width_mm: Option<f64>,
    },

    #[serde(rename = "set_page_margin", rename_all = "camelCase")]
    SetPageMargin {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        left_mm: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        right_mm: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        top_mm: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bottom_mm: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        margin_mm: Option<f64>,
    },

    #[serde(rename = "shrink_cell_font", rename_all = "camelCase")]
    ShrinkCellFont {
        table_index: usize,
        row: usize,
        col: usize,
        size_pt: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        min_pt: Option<f64>,
    },
}

/// Review state of a proposed `DiffItem`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DiffStatus {
    Pending,
    Accepted,
    Rejected,
}

/// Structural scope of a proposed `DiffItem`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffScope {
    Paragraph,
    Cell,
    TableStructure,
    Style,
    Document,
}

/// Target coordinate reference attached to a `DiffItem`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffTargetCoord {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sec: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub para: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table_index: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub col: Option<usize>,
}

/// Bounding box for SmartDiff overlay rendering (`DiffItem.bbox`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffBBox {
    pub page_index: usize,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Layout verification status attached to a `DiffItem`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffVerificationStatus {
    Verified,
    WarningUnresolved,
    WarningNewIssues,
    Unverified,
}

/// Cell geometry/overflow deltas captured during trial verification.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffMetricsDelta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_overflow_mm: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_overflow_mm: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width_before_mm: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width_after_mm: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height_before_mm: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height_after_mm: Option<f64>,
}

/// Smart Diff card contract matching `src/types/operations.ts` plus optional `baseRevision`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffItem {
    pub id: String,
    pub status: DiffStatus,
    pub scope: DiffScope,
    pub title: String,
    pub before_text: String,
    pub after_text: String,
    pub unified_ops: Vec<UnifiedOp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_coord: Option<DiffTargetCoord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bbox: Option<DiffBBox>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_status: Option<DiffVerificationStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warnings: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metrics_delta: Option<DiffMetricsDelta>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_revision: Option<u64>,
}
