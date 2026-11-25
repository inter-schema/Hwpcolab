use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use uuid::Uuid;

use crate::docx::parse_docx_bytes_to_markdown;
use crate::error::AttachError;
use crate::preflight::{
    assert_attachment_file_size, assert_zip_expansion_within_limits, ZipExpansionLimits,
    MAX_XLSX_ENTRY_BYTES, MAX_XLSX_TOTAL_BYTES,
};
use hopclaw_types::{AttachmentKind, ParsedAttachment};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub trait AttachmentParser: Send + Sync {
    fn parse<'a>(
        &'a self,
        file_path: &'a str,
        file_bytes: &'a [u8],
    ) -> BoxFuture<'a, Result<ParsedAttachment, AttachError>>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct DefaultAttachmentParser;

impl AttachmentParser for DefaultAttachmentParser {
    fn parse<'a>(
        &'a self,
        file_path: &'a str,
        file_bytes: &'a [u8],
    ) -> BoxFuture<'a, Result<ParsedAttachment, AttachError>> {
        Box::pin(async move { parse_local_attachment(file_path, file_bytes) })
    }
}

pub fn parse_local_attachment(
    file_path: &str,
    file_bytes: &[u8],
) -> Result<ParsedAttachment, AttachError> {
    assert_attachment_file_size(file_bytes.len() as u64)?;

    let path = Path::new(file_path);
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();
    let file_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(file_path)
        .to_string();

    match ext.as_str() {
        "docx" => {
            let summary = match parse_docx_bytes_to_markdown(file_bytes) {
                Ok(md) if md.is_empty() => "[빈 DOCX 문서]".to_string(),
                Ok(md) => md,
                Err(e) => format!("[DOCX 파싱 실패: {e}]"),
            };
            Ok(ParsedAttachment {
                id: Uuid::new_v4().to_string(),
                file_name,
                file_path: file_path.to_string(),
                kind: AttachmentKind::Docx,
                markdown_summary: summary,
                structured_data: None,
            })
        }
        "xlsx" | "xls" | "csv" => parse_spreadsheet(file_path, &file_name, &ext, file_bytes),
        "hwp" | "hwpx" => Ok(ParsedAttachment {
            id: Uuid::new_v4().to_string(),
            file_name,
            file_path: file_path.to_string(),
            kind: AttachmentKind::Hwp,
            markdown_summary: "[HWP 파싱은 hopclaw-doc 통합 후 연결됩니다]".to_string(),
            structured_data: None,
        }),
        "pdf" => parse_pdf_stub(file_path, &file_name, file_bytes),
        "txt" | "md" | "json" => Ok(ParsedAttachment {
            id: Uuid::new_v4().to_string(),
            file_name,
            file_path: file_path.to_string(),
            kind: AttachmentKind::Text,
            markdown_summary: String::from_utf8_lossy(file_bytes).into_owned(),
            structured_data: None,
        }),
        _ => Ok(ParsedAttachment {
            id: Uuid::new_v4().to_string(),
            file_name,
            file_path: file_path.to_string(),
            kind: AttachmentKind::Image,
            markdown_summary: format!(
                "[로컬 이미지 파일 첨부됨: {file_path} — insert_image 또는 place_seal의 path/source 인자로 직접 사용 가능]"
            ),
            structured_data: None,
        }),
    }
}

fn parse_spreadsheet(
    file_path: &str,
    file_name: &str,
    ext: &str,
    file_bytes: &[u8],
) -> Result<ParsedAttachment, AttachError> {
    if ext == "xlsx" {
        if let Err(e) = assert_zip_expansion_within_limits(
            file_bytes,
            &ZipExpansionLimits::new(MAX_XLSX_ENTRY_BYTES, MAX_XLSX_TOTAL_BYTES),
        ) {
            return Ok(ParsedAttachment {
                id: Uuid::new_v4().to_string(),
                file_name: file_name.to_string(),
                file_path: file_path.to_string(),
                kind: AttachmentKind::Excel,
                markdown_summary: format!("[엑셀 파싱 실패: {e}]"),
                structured_data: None,
            });
        }
    }

    if ext == "csv" {
        let text = String::from_utf8_lossy(file_bytes);
        let rows: Vec<Vec<String>> = text
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| l.split(',').map(|c| c.trim().to_string()).collect())
            .collect();
        let summary = if let Some(header) = rows.first() {
            let divider = vec!["---"; header.len()];
            let mut lines = vec![
                "### [시트: CSV]".to_string(),
                format!("| {} |", header.join(" | ")),
                format!("| {} |", divider.join(" | ")),
            ];
            for row in &rows[1..] {
                lines.push(format!("| {} |", row.join(" | ")));
            }
            lines.join("\n")
        } else {
            String::new()
        };
        return Ok(ParsedAttachment {
            id: Uuid::new_v4().to_string(),
            file_name: file_name.to_string(),
            file_path: file_path.to_string(),
            kind: AttachmentKind::Excel,
            markdown_summary: summary,
            structured_data: Some(serde_json::json!({ "CSV": rows })),
        });
    }

    #[cfg(not(feature = "excel-calamine"))]
    {
        Ok(ParsedAttachment {
            id: Uuid::new_v4().to_string(),
            file_name: file_name.to_string(),
            file_path: file_path.to_string(),
            kind: AttachmentKind::Excel,
            markdown_summary: "[엑셀 파싱 비활성화: `excel-calamine` feature가 필요합니다]"
                .to_string(),
            structured_data: None,
        })
    }

    #[cfg(feature = "excel-calamine")]
    {
        Ok(ParsedAttachment {
            id: Uuid::new_v4().to_string(),
            file_name: file_name.to_string(),
            file_path: file_path.to_string(),
            kind: AttachmentKind::Excel,
            markdown_summary: "[excel-calamine feature hook]".to_string(),
            structured_data: None,
        })
    }
}

fn parse_pdf_stub(
    file_path: &str,
    file_name: &str,
    _file_bytes: &[u8],
) -> Result<ParsedAttachment, AttachError> {
    #[cfg(not(feature = "pdf-lopdf"))]
    {
        Ok(ParsedAttachment {
            id: Uuid::new_v4().to_string(),
            file_name: file_name.to_string(),
            file_path: file_path.to_string(),
            kind: AttachmentKind::Pdf,
            markdown_summary:
                "[PDF 파싱 비활성화: `pdf-lopdf` feature가 필요합니다 (pdfjs 폴백 사용 가능)]"
                    .to_string(),
            structured_data: None,
        })
    }

    #[cfg(feature = "pdf-lopdf")]
    {
        Ok(ParsedAttachment {
            id: Uuid::new_v4().to_string(),
            file_name: file_name.to_string(),
            file_path: file_path.to_string(),
            kind: AttachmentKind::Pdf,
            markdown_summary: "[pdf-lopdf feature hook]".to_string(),
            structured_data: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_text_and_csv_and_image() {
        let txt = parse_local_attachment("notes.txt", "안녕하세요".as_bytes()).unwrap();
        assert_eq!(txt.kind, AttachmentKind::Text);
        assert_eq!(txt.markdown_summary, "안녕하세요");

        let csv = parse_local_attachment("data.csv", b"col1,col2\n1,2").unwrap();
        assert_eq!(csv.kind, AttachmentKind::Excel);
        assert!(csv.markdown_summary.contains("| col1 | col2 |"));
        assert!(csv.markdown_summary.contains("| 1 | 2 |"));

        let img = parse_local_attachment("seal.png", b"png").unwrap();
        assert_eq!(img.kind, AttachmentKind::Image);
        assert!(img.markdown_summary.contains("seal.png"));
    }
}
