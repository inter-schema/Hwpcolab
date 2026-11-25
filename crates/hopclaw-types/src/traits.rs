use crate::doc::{DocFormat, ParentCellCoord, TableCoordinateMeta};
use crate::error::{ErrorCode, ErrorModel};
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

/// Heap-allocated `Send` future returned by object-safe storage traits without extra macro deps.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Canonical pre-order table coordinate descriptor shared across inspection (`hopclaw-doc`)
/// and editors (`hopclaw-hwpx`, `hopclaw-hwp5`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TableCoord {
    pub table_index: usize,
    pub sec: usize,
    pub para: usize,
    pub ctrl: usize,
    pub depth: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_table_index: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_cell: Option<ParentCellCoord>,
}

/// Shared canonical pre-order table traversal contract.
///
/// Implementors MUST guarantee that:
/// 1. Sections are visited in ascending index (`sec = 0..`), paragraphs in ascending order,
///    and controls in ascending order.
/// 2. Nested tables are visited in pre-order (parent table emitted before recursively
///    visiting child tables inside its cells in row-major order).
/// 3. `table.table_index` equals its 0-based position in the returned slice.
pub trait TableWalker {
    /// Enumerate all tables in canonical pre-order with coordinates and cell metadata.
    fn walk_tables(&self) -> Result<Vec<TableCoordinateMeta>, ErrorModel>;

    /// Enumerate lightweight `TableCoord` entries in canonical pre-order.
    fn walk_table_coords(&self) -> Result<Vec<TableCoord>, ErrorModel> {
        Ok(self
            .walk_tables()?
            .iter()
            .map(TableCoordinateMeta::coord)
            .collect())
    }

    /// Resolve `table_index` into its canonical `TableCoord`, or fail with `E_TABLE_NOT_FOUND`.
    fn resolve_table_coord(&self, table_index: usize) -> Result<TableCoord, ErrorModel> {
        let coords = self.walk_table_coords()?;
        coords.get(table_index).cloned().ok_or_else(|| {
            ErrorModel::new(
                ErrorCode::E_TABLE_NOT_FOUND,
                format!(
                    "표 인덱스 {}를 찾을 수 없습니다 (전체 표 개수: {}).",
                    table_index,
                    coords.len()
                ),
            )
        })
    }
}

/// Verify canonical pre-order coordinate invariants on a walked table list.
pub fn validate_preorder_coords(tables: &[TableCoordinateMeta]) -> Result<(), ErrorModel> {
    for (idx, table) in tables.iter().enumerate() {
        if table.table_index != idx {
            return Err(ErrorModel::new(
                ErrorCode::E_DOC_INVALID,
                format!(
                    "TableWalker canonical order violated at position {}: tableIndex={}",
                    idx, table.table_index
                ),
            ));
        }
        let depth = table.depth.unwrap_or(0);
        if depth > 0 {
            match table.parent_table_index {
                Some(parent_idx) if parent_idx < idx => {}
                other => {
                    return Err(ErrorModel::new(
                        ErrorCode::E_DOC_INVALID,
                        format!(
                            "Nested table {} (depth={}) has invalid parentTableIndex {:?}",
                            idx, depth, other
                        ),
                    ));
                }
            }
        }
    }
    Ok(())
}

/// Isolated trial workspace handle returned by [`TrialStore`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrialWorkspace {
    pub trial_id: String,
    pub path: PathBuf,
    pub format: DocFormat,
    pub base_revision: u64,
}

/// Snapshot metadata returned by [`SessionStorage`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSnapshotInfo {
    pub session_id: String,
    pub revision: u64,
    pub format: DocFormat,
    pub staging_path: PathBuf,
    pub byte_len: u64,
}

/// Isolated trial storage trait consumed by `hopclaw-ai` for `apply_trial_edits`
/// and auto-verification without mutating active session staging files.
pub trait TrialStore: Send + Sync {
    /// Create or reset an isolated trial copy from `base_bytes`.
    fn prepare_trial<'a>(
        &'a self,
        request_id: &'a str,
        format: DocFormat,
        base_bytes: &'a [u8],
        base_revision: u64,
    ) -> BoxFuture<'a, Result<TrialWorkspace, ErrorModel>>;

    /// Read current bytes from the isolated trial file.
    fn read_trial_bytes<'a>(
        &'a self,
        trial: &'a TrialWorkspace,
    ) -> BoxFuture<'a, Result<Vec<u8>, ErrorModel>>;

    /// Write updated bytes to the isolated trial file.
    fn write_trial_bytes<'a>(
        &'a self,
        trial: &'a TrialWorkspace,
        bytes: &'a [u8],
    ) -> BoxFuture<'a, Result<(), ErrorModel>>;

    /// Remove a single trial file after completion/cancellation.
    fn cleanup_trial<'a>(
        &'a self,
        trial: &'a TrialWorkspace,
    ) -> BoxFuture<'a, Result<(), ErrorModel>>;

    /// Remove all trial artifacts associated with `request_id`.
    fn cleanup_request_trials<'a>(
        &'a self,
        request_id: &'a str,
    ) -> BoxFuture<'a, Result<(), ErrorModel>>;
}

/// Abstract session storage and revision/undo lifecycle trait implemented by `DocumentSession`
/// management in `src-tauri`.
pub trait SessionStorage: Send + Sync {
    /// Query current staging path, format, and revision for `session_id`.
    fn snapshot_info<'a>(
        &'a self,
        session_id: &'a str,
    ) -> BoxFuture<'a, Result<SessionSnapshotInfo, ErrorModel>>;

    /// Read the active staging document bytes for `session_id`.
    fn read_staging_bytes<'a>(
        &'a self,
        session_id: &'a str,
    ) -> BoxFuture<'a, Result<Vec<u8>, ErrorModel>>;

    /// Commit new document bytes, optionally asserting `expected_revision` (failing with
    /// `E_STALE_REVISION` on mismatch).
    fn commit_revision<'a>(
        &'a self,
        session_id: &'a str,
        expected_revision: Option<u64>,
        new_bytes: &'a [u8],
    ) -> BoxFuture<'a, Result<SessionSnapshotInfo, ErrorModel>>;

    /// Restore the previous revision snapshot.
    fn undo<'a>(
        &'a self,
        session_id: &'a str,
    ) -> BoxFuture<'a, Result<SessionSnapshotInfo, ErrorModel>>;

    /// Re-apply an undone revision snapshot.
    fn redo<'a>(
        &'a self,
        session_id: &'a str,
    ) -> BoxFuture<'a, Result<SessionSnapshotInfo, ErrorModel>>;
}
