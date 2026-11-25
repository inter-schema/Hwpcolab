pub mod docx;
pub mod error;
pub mod parser;
pub mod preflight;
pub mod types;
pub mod worker;

pub use error::AttachError;
pub use parser::{parse_local_attachment, AttachmentParser, DefaultAttachmentParser};
pub use preflight::{
    assert_attachment_file_size, assert_zip_expansion_within_limits, ZipExpansionLimits,
    MAX_ATTACHMENT_FILE_BYTES, MAX_DOCX_DOCUMENT_XML_BYTES, MAX_XLSX_ENTRY_BYTES,
    MAX_XLSX_TOTAL_BYTES,
};
pub use types::{AttachmentKind, ParsedAttachment, WorkerRequest, WorkerResponse};
pub use worker::{spawn_and_run_worker, WorkerOptions};
