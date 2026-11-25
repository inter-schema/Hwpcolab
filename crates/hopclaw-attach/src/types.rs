use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AttachmentKind {
    Excel,
    Docx,
    Pdf,
    Hwp,
    Image,
    Text,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedAttachment {
    pub id: String,
    pub file_name: String,
    pub file_path: String,
    pub kind: AttachmentKind,
    pub markdown_summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub structured_data: Option<Value>,
}

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
