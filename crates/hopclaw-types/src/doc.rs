use crate::traits::TableCoord;
use serde::{Deserialize, Serialize};

/// Container format for an active document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DocFormat {
    Hwpx,
    Hwp5,
}

/// Strangler-pattern runtime routing flag (`engine.backend = node | rust | shadow`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EngineBackend {
    #[default]
    Node,
    Rust,
    Shadow,
}

/// Rendered cell bounding box from `@rhwp/core` or native layout engine (`src/types/hwp.ts`).
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

/// Parent cell coordinate for nested tables (`TableCoordinateMeta.parentCell`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParentCellCoord {
    pub row: usize,
    pub col: usize,
}

/// Cell metadata within `TableCoordinateMeta` (`src/types/hwp.ts`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TableCellMeta {
    pub idx: usize,
    pub row: usize,
    pub col: usize,
    pub row_span: usize,
    pub col_span: usize,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bbox: Option<CellBBox>,
}

/// Table coordinate & cell inventory matching `src/types/hwp.ts`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TableCoordinateMeta {
    #[serde(default)]
    pub table_index: usize,
    #[serde(default)]
    pub sec: usize,
    #[serde(default)]
    pub para: usize,
    #[serde(default)]
    pub ctrl: usize,
    pub row_count: usize,
    pub col_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_table_index: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_cell: Option<ParentCellCoord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depth: Option<usize>,
    pub cells: Vec<TableCellMeta>,
}

impl TableCoordinateMeta {
    /// Extract canonical pre-order `TableCoord` for this table.
    pub fn coord(&self) -> TableCoord {
        TableCoord {
            table_index: self.table_index,
            sec: self.sec,
            para: self.para,
            ctrl: self.ctrl,
            depth: self.depth.unwrap_or(0),
            parent_table_index: self.parent_table_index,
            parent_cell: self.parent_cell,
        }
    }
}

/// Selection rectangle matching `SelectionTarget` in `src/types/hwp.ts`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionRect {
    pub page_index: usize,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Selected table cell entry (`SelectionTarget` table_cells variant).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectedCell {
    pub cell_idx: usize,
    pub row: usize,
    pub col: usize,
    pub text: String,
    pub bbox: CellBBox,
}

/// Canvas selection target matching `src/types/hwp.ts` (`tag = "kind"`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum SelectionTarget {
    #[serde(rename = "body_range", rename_all = "camelCase")]
    BodyRange {
        sec: usize,
        start_para: usize,
        start_offset: usize,
        end_para: usize,
        end_offset: usize,
        selected_text: String,
        rects: Vec<SelectionRect>,
    },
    #[serde(rename = "table_cells", rename_all = "camelCase")]
    TableCells {
        table_index: usize,
        sec: usize,
        para: usize,
        ctrl: usize,
        start_row: usize,
        start_col: usize,
        end_row: usize,
        end_col: usize,
        selected_cells: Vec<SelectedCell>,
        union_rect: SelectionRect,
    },
}

/// Direct inline editing target matching `DirectEditingTarget` in `src/types/hwp.ts`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum DirectEditingTarget {
    #[serde(rename = "cell", rename_all = "camelCase")]
    Cell {
        table_index: usize,
        row: usize,
        col: usize,
        text: String,
        bbox: CellBBox,
    },
    #[serde(rename = "paragraph", rename_all = "camelCase")]
    Paragraph {
        sec: usize,
        para: usize,
        text: String,
        rect: SelectionRect,
    },
}

/// Session response returned by `open_document_session` and `create_blank_session`.
/// Preserves exact `snake_case` field casing per `src/types/hwp.ts` while adding optional `session_id` and `revision`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenedDocSession {
    pub original_path: String,
    pub staging_path: String,
    pub file_name: String,
    pub is_hwpx: bool,
    pub bytes: Vec<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none", alias = "sessionId")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<u64>,
}

/// Execution result contract returned by `execute_claw_ops` (`src-tauri/src/commands/claw_bridge.rs`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClawExecResult {
    pub ok: bool,
    pub stdout_json: serde_json::Value,
    pub stderr: String,
}

/// Document inspection result contract matching `extract_text.js --inspect [--with-cell-text]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectDocumentResult {
    pub file_type: String,
    pub section_count: usize,
    pub paragraph_count: usize,
    pub table_count: usize,
    pub cell_count: usize,
    #[serde(default)]
    pub image_count: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tables: Option<Vec<TableCoordinateMeta>>,
}
