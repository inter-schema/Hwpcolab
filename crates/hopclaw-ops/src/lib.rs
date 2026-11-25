//! `UnifiedOp` compiler (`EditPlan` IR + debug JSON serializer), `PolicyGuard` G1–G6,
//! and Korean official document typography rules for HOP-Claw.

pub mod compiler;
pub mod debug_json;
pub mod official_doc;
pub mod policy_guard;
mod sanitize;

pub use compiler::compile_edit_plan;
pub use debug_json::{EnginePayload, OpTranslator};
pub use official_doc::{
    build_official_doc_standard_ops, official_outline_levels, OfficialDocTypography,
    OfficialOutlineLevel, OFFICIAL_DOC_TYPOGRAPHY,
};
pub use policy_guard::{
    extract_negative_constraints, lint_ops, lint_raw_json_ops, PolicyGuardContext,
    PolicyGuardResult, RawPolicyGuardResult, RawRejectedOp, RejectedOp,
};
pub use sanitize::sanitize_markdown_formatting;
