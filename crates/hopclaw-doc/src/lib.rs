//! Pure-Rust document inspection, canonical `TableWalker`, layout measurement,
//! and Strangler-pattern native `rhwp` adapter for HOP-Claw.

pub mod hwpx_walker;
pub mod layout;
pub mod rhwp_adapter;
pub mod strangler;
pub mod types;

use hopclaw_types::{ErrorCode, ErrorModel};
use thiserror::Error;

/// Errors returned by `hopclaw-doc` operations.
#[derive(Debug, Error)]
pub enum DocError {
    #[error("invalid document container: {0}")]
    InvalidDocument(String),

    #[error("XML parse error in {section}: {message}")]
    XmlParse { section: String, message: String },

    #[error("UTF-8 decode error: {0}")]
    Utf8(#[from] std::string::FromUtf8Error),

    #[error("ZIP error: {0}")]
    Zip(#[from] zip::result::ZipError),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("unsupported feature: {0}")]
    UnsupportedFeature(String),

    #[error("invalid table invariants: {0}")]
    Contract(String),
}

impl From<DocError> for ErrorModel {
    fn from(err: DocError) -> Self {
        match err {
            DocError::UnsupportedFeature(msg) => {
                ErrorModel::new(ErrorCode::E_OP_UNSUPPORTED_FORMAT, msg)
            }
            other => ErrorModel::new(ErrorCode::E_DOC_INVALID, other.to_string()),
        }
    }
}

pub use hwpx_walker::{
    build_tables_meta_for_hwpx, extract_hwpx_markdown, inspect_hwpx, is_hwpx_bytes,
    merge_hwpx_tables_with_rhwp, parse_hwpx_section_tables, parse_hwpx_tables_from_bytes,
    HwpxParsedCell, HwpxParsedTable, HwpxTableWalker,
};
pub use layout::{
    compare_layout_reports, format_layout_report_for_prompt, inspect_layout_from_source,
    measure_korean_text_pt, DocumentLayoutSource, HU_PER_MM, HU_PER_PX, PX_PER_MM,
};
pub use rhwp_adapter::{inspect_document_bytes, inspect_hwp5_cfb_summary, is_hwp5_cfb_bytes};
pub use strangler::{
    inspect_shadow_diff, markdown_shadow_diff, parse_engine_backend, resolve_engine_backend_from_env,
};
pub use types::{
    CellBBox, CellLayoutInfo, CellProps, InspectDocumentResult, LayoutComparison,
    LayoutInspectorOptions, LayoutIssue, LayoutIssueCellRef, LayoutIssueCode, LayoutIssueMetrics,
    LayoutIssueSeverity, LayoutMetricsDelta, LayoutReport, PageDef, ParentCellCoord,
    TableCellMeta, TableCoord, TableCoordinateMeta, TableLayoutInfo, TableWalker,
};
