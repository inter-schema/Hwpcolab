use serde::{Deserialize, Serialize};

/// Layout issue code matching `src/core/layoutInspector.ts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[allow(non_camel_case_types)]
pub enum LayoutIssueCode {
    CELL_OVERFLOW_V,
    CELL_OVERFLOW_H,
    TABLE_EXCEEDS_PAGE,
}

/// Layout issue severity (`"error" | "warning"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LayoutIssueSeverity {
    Error,
    Warning,
}

/// Measured cell geometry and overflow metrics matching `CellLayoutInfo` in `layoutInspector.ts`.
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

/// Cell coordinate reference attached to `LayoutIssue`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutIssueCellRef {
    pub table_index: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sec: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub para: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ctrl: Option<usize>,
    pub row: usize,
    pub col: usize,
    pub cell_idx: usize,
}

/// Quantitative metrics payload attached to `LayoutIssue`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutIssueMetrics {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cell_height_mm: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_height_mm: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overflow_mm: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lines: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table_width_mm: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub printable_width_mm: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overflow_width_mm: Option<f64>,
}

/// Single layout diagnostic matching `LayoutIssue` in `layoutInspector.ts`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutIssue {
    pub id: String,
    pub code: LayoutIssueCode,
    pub severity: LayoutIssueSeverity,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cell: Option<LayoutIssueCellRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table_index: Option<usize>,
    pub metrics: LayoutIssueMetrics,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

/// Per-table layout measurement summary inside `LayoutReport.tables`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TableLayoutInfo {
    pub table_index: usize,
    pub section: usize,
    pub para: usize,
    pub ctrl: usize,
    pub cell_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table_width_mm: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub printable_width_mm: Option<f64>,
    pub cells: Vec<CellLayoutInfo>,
}

/// Document layout inspection report matching `LayoutReport` in `layoutInspector.ts`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutReport {
    pub timestamp: u64,
    pub table_count: usize,
    pub issues_count: usize,
    pub issues: Vec<LayoutIssue>,
    pub tables: Vec<TableLayoutInfo>,
}

/// Optional aggregate metric deltas across before/after layout reports.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutMetricsDelta {
    pub before_issues_count: usize,
    pub after_issues_count: usize,
    pub resolved_count: usize,
    pub remaining_count: usize,
    pub new_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_overflow_before_mm: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_overflow_after_mm: Option<f64>,
}

/// Before/after layout comparison matching `compareLayoutReports` in `layoutInspector.ts`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutComparison {
    pub ok: bool,
    pub resolved_issues: Vec<LayoutIssue>,
    pub remaining_issues: Vec<LayoutIssue>,
    pub new_issues: Vec<LayoutIssue>,
    pub summary_text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metrics_delta: Option<LayoutMetricsDelta>,
}

/// Layout measurement parameters matching `LayoutInspectorOptions` in `layoutInspector.ts`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutInspectorOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safety_margin_horizontal: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safety_margin_vertical: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_font_pt: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_spacing_ratio: Option<f64>,
}
