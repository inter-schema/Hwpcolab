use hopclaw_types::ParsedAttachment;
use serde::{Deserialize, Serialize};

pub use hopclaw_types::{AttachmentKind, ParsedAttachment as HopParsedAttachment};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerRequest {
    pub file_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerResponse {
    pub ok: bool,
    pub attachment: Option<ParsedAttachment>,
    pub error: Option<String>,
}
