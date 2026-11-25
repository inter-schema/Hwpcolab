//! Re-exports shared types from `hopclaw_types` plus internal layout measurement inputs
//! (`PageDef`, `CellProps`) that are local to `hopclaw-doc`.

use serde::{Deserialize, Serialize};

pub use hopclaw_types::{
    CellBBox, CellLayoutInfo, InspectDocumentResult, LayoutComparison, LayoutInspectorOptions,
    LayoutIssue, LayoutIssueCellRef, LayoutIssueCode, LayoutIssueMetrics, LayoutIssueSeverity,
    LayoutMetricsDelta, LayoutReport, ParentCellCoord, TableCellMeta, TableCoord,
    TableCoordinateMeta, TableLayoutInfo, TableWalker,
};

/// Page dimensions and margins in HWPUNIT (`getPageDef`).
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

/// Cell inner padding in HWPUNIT (`getCellProperties`).
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

/// Default `LayoutInspectorOptions` matching `layoutInspector.ts`.
pub fn default_layout_options() -> LayoutInspectorOptions {
    LayoutInspectorOptions {
        safety_margin_horizontal: Some(0.05),
        safety_margin_vertical: Some(0.08),
        default_font_pt: Some(10.0),
        line_spacing_ratio: Some(1.10),
    }
}
