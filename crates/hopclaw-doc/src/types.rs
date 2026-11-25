//! Local Phase-0 mirror types aligned with `src/types/hwp.ts`,
//! `src/core/layoutInspector.ts`, and `extract_text.js --inspect`.
//! Once `hopclaw-types` freezes, these types will be replaced/aliased to `hopclaw_types::*`.

use serde::{Deserialize, Serialize};

use crate::DocError;

/// Bounding box for a single cell (page-relative pixels from rhwp layout engine).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CellBBox {
    pub cell_idx: usize,
    pub row: usize,
    pub col: usize,
    pub row_span: usize,
    pub col_span: usize,
    pub page_index: usize,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Parent cell coordinate for a nested table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParentCellRef {
    pub row: usize,
    pub col: usize,
}

/// Cell metadata inside a traversed table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TableCellMeta {
    pub idx: usize,
    pub row: usize,
    pub col: usize,
    pub row_span: usize,
    pub col_span: usize,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bbox: Option<CellBBox>,
}

/// Canonical pre-order table metadata matching `TableCoordinateMeta` in `src/types/hwp.ts`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TableCoordinateMeta {
    pub table_index: usize,
    pub sec: usize,
    pub para: usize,
    pub ctrl: usize,
    pub row_count: usize,
    pub col_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_table_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_cell: Option<ParentCellRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub depth: Option<usize>,
    pub cells: Vec<TableCellMeta>,
}

/// Output of `inspect_document`, matching `extract_text.js --inspect [--with-cell-text]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentInspectReport {
    pub file_type: String,
    pub section_count: usize,
    pub paragraph_count: usize,
    pub table_count: usize,
    pub cell_count: usize,
    pub image_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tables: Option<Vec<TableCoordinateMeta>>,
}

/// Canonical `TableWalker` trait shared between inspection and editing.
pub trait TableWalker {
    fn walk_tables(&self, bytes: &[u8]) -> Result<Vec<TableCoordinateMeta>, DocError>;
}

/// Page dimensions and margins in HWPUNIT.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageDef {
    pub width: i64,
    pub height: i64,
    pub margin_left: i64,
    pub margin_right: i64,
    pub margin_top: i64,
    pub margin_bottom: i64,
}

impl Default for PageDef {
    fn default() -> Self {
        Self {
            width: 59_528,
            height: 84_188,
            margin_left: 8_504,
            margin_right: 8_504,
            margin_top: 5_669,
            margin_bottom: 4_252,
        }
    }
}

/// Cell inner padding in HWPUNIT.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CellProps {
    pub padding_left: i64,
    pub padding_right: i64,
    pub padding_top: i64,
    pub padding_bottom: i64,
}

impl Default for CellProps {
    fn default() -> Self {
        Self {
            padding_left: 400,
            padding_right: 400,
            padding_top: 400,
            padding_bottom: 400,
        }
    }
}

/// Issue classification codes matching `LayoutIssueCode` in `src/core/layoutInspector.ts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LayoutIssueCode {
    #[serde(rename = "CELL_OVERFLOW_V")]
    CellOverflowV,
    #[serde(rename = "CELL_OVERFLOW_H")]
    CellOverflowH,
    #[serde(rename = "TABLE_EXCEEDS_PAGE")]
    TableExceedsPage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LayoutSeverity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CellLayoutInfo {
    pub cell_idx: usize,
    pub row: usize,
    pub col: usize,
    pub row_span: usize,
    pub col_span: usize,
    pub width_mm: f64,
    pub height_mm: f64,
    pub avail_width_mm: f64,
    pub avail_height_mm: f64,
    pub estimated_lines: usize,
    pub content_height_mm: f64,
    pub is_vertical_overflow: bool,
    pub is_horizontal_overflow: bool,
    pub overflow_amount_mm: f64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutIssueCell {
    pub table_index: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sec: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub para: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ctrl: Option<usize>,
    pub row: usize,
    pub col: usize,
    pub cell_idx: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutIssueMetrics {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cell_height_mm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_height_mm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overflow_mm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lines: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub table_width_mm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub printable_width_mm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overflow_width_mm: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutIssue {
    pub id: String,
    pub code: LayoutIssueCode,
    pub severity: LayoutSeverity,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cell: Option<LayoutIssueCell>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub table_index: Option<usize>,
    pub metrics: LayoutIssueMetrics,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TableLayoutSummary {
    pub table_index: usize,
    pub section: usize,
    pub para: usize,
    pub ctrl: usize,
    pub cell_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub table_width_mm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub printable_width_mm: Option<f64>,
    pub cells: Vec<CellLayoutInfo>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutReport {
    pub timestamp: u64,
    pub table_count: usize,
    pub issues_count: usize,
    pub issues: Vec<LayoutIssue>,
    pub tables: Vec<TableLayoutSummary>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutInspectorOptions {
    pub safety_margin_horizontal: f64,
    pub safety_margin_vertical: f64,
    pub default_font_pt: f64,
    pub line_spacing_ratio: f64,
}

impl Default for LayoutInspectorOptions {
    fn default() -> Self {
        Self {
            safety_margin_horizontal: 0.05,
            safety_margin_vertical: 0.08,
            default_font_pt: 10.0,
            line_spacing_ratio: 1.10,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutMetricsDelta {
    pub before_issues_count: usize,
    pub after_issues_count: usize,
    pub issues_delta: isize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutComparison {
    pub ok: bool,
    pub resolved_issues: Vec<LayoutIssue>,
    pub remaining_issues: Vec<LayoutIssue>,
    pub new_issues: Vec<LayoutIssue>,
    pub summary_text: String,
    pub metrics_delta: LayoutMetricsDelta,
}
