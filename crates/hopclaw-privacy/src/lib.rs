//! Korean PII scanning/redaction and session-scoped prompt masking for HOP-Claw.

pub mod masker;
pub mod pii;
mod regex_util;

pub use hopclaw_types::{
    ExcludedRegion, ExcludedRegionTargetType, PiiCategory, PiiRedactionHit, ScanAndRedactResult,
    SelectionRect, UnifiedOp,
};
pub use masker::{PiiTokenMap, PrivacySessionMasker};
pub use pii::{
    compute_sha256_8, is_valid_biz_reg_checksum, is_valid_luhn_checksum, is_valid_rrn_checksum,
    mask_memory_source_text, scan_and_redact_pii,
};
