pub mod agent;
pub mod attachment;
pub mod doc;
pub mod edit_plan;
pub mod error;
pub mod layout;
pub mod memory;
pub mod ops;
pub mod traits;
pub mod units;

pub use agent::{
    AgentLoopMode, AgentStepEvent, AgentStepType, ExcludedRegion, ExcludedRegionTargetType,
    OrchestrateAiEditResult, ProviderId, ReasoningEffort,
};
pub use attachment::{AttachmentKind, ParsedAttachment};
pub use doc::{
    CellBBox, ClawExecResult, DirectEditingTarget, DocFormat, EngineBackend, InspectDocumentResult,
    OpenedDocSession, ParentCellCoord, SelectedCell, SelectionRect, SelectionTarget, TableCellMeta,
    TableCoordinateMeta,
};
pub use edit_plan::{
    EditPlan, Hwp5CellImageTarget, Hwp5EditPlan, Hwp5EditStep, Hwp5TableTarget, HwpxEditPlan,
    HwpxEditStep,
};
pub use error::{ErrorCode, ErrorModel};
pub use layout::{
    CellLayoutInfo, LayoutComparison, LayoutInspectorOptions, LayoutIssue, LayoutIssueCellRef,
    LayoutIssueCode, LayoutIssueMetrics, LayoutIssueSeverity, LayoutMetricsDelta, LayoutReport,
    TableLayoutInfo,
};
pub use memory::{
    MemoryDoc, MemoryMeta, PiiCategory, PiiRedactionHit, ScanAndRedactResult, SectionDiff,
    SectionDiffOp, SectionDiffToken, SourceDocRef, FIXED_MEMORY_SECTIONS,
};
pub use ops::{
    BorderLineType, BorderSide, BorderSides, CellHAlign, CellVAlign, ColInsertPosition, DiffBBox,
    DiffItem, DiffMetricsDelta, DiffScope, DiffStatus, DiffTargetCoord, DiffVerificationStatus,
    EqualizeMode, HeaderFooterLocation, HighlightValue, ImageCellTarget, ListStyleMode,
    NestedTableWidthMode, PageNumberAlign, ParaIndexTarget, ParagraphAlign, RowInsertPosition,
    SealMode, UnifiedOp,
};
pub use traits::{
    validate_preorder_coords, BoxFuture, SessionSnapshotInfo, SessionStorage, TableCoord,
    TableWalker, TrialStore, TrialWorkspace,
};
pub use units::{
    hwpunit_to_mm, hwpunit_to_pt, mm_to_hwpunit, pt_to_hwpunit, HwpUnit, HWPUNIT_PER_MM,
    HWPUNIT_PER_MM_APPROX, HWPUNIT_PER_PT, HWPUNIT_PER_PX,
};
