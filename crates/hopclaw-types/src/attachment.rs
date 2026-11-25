use serde::{Deserialize, Serialize};

/// Attachment format category matching `ParsedAttachment.kind` in `src/core/multiFileParser.ts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AttachmentKind {
    Excel,
    Docx,
    Pdf,
    Hwp,
    Image,
    Text,
}

/// Parsed attachment summary passed to the AI prompt context (`src/core/multiFileParser.ts`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedAttachment {
    pub id: String,
    pub file_name: String,
    pub file_path: String,
    pub kind: AttachmentKind,
    pub markdown_summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structured_data: Option<serde_json::Value>,
}
