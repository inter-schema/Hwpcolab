use std::io::Cursor;
use zip::ZipArchive;

use crate::error::AttachError;

pub const MAX_ATTACHMENT_FILE_BYTES: u64 = 128 * 1024 * 1024;
pub const MAX_DOCX_DOCUMENT_XML_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_XLSX_ENTRY_BYTES: u64 = 32 * 1024 * 1024;
pub const MAX_XLSX_TOTAL_BYTES: u64 = 128 * 1024 * 1024;

pub fn assert_attachment_file_size(file_size: u64) -> Result<(), AttachError> {
    if file_size > MAX_ATTACHMENT_FILE_BYTES {
        return Err(AttachError::LimitExceeded(
            "첨부 파일은 128 MiB 이하만 처리할 수 있습니다.".to_string(),
        ));
    }
    Ok(())
}

pub struct ZipExpansionLimits<'a> {
    pub max_entry_bytes: u64,
    pub max_total_bytes: u64,
    pub max_entries: usize,
    pub include_file: Option<&'a (dyn Fn(&str) -> bool + Sync)>,
}

impl<'a> ZipExpansionLimits<'a> {
    pub fn new(max_entry_bytes: u64, max_total_bytes: u64) -> Self {
        Self {
            max_entry_bytes,
            max_total_bytes,
            max_entries: 10_000,
            include_file: None,
        }
    }
}

/// Preflights ZIP central-directory metadata without inflating any entry.
pub fn assert_zip_expansion_within_limits(
    file_bytes: &[u8],
    limits: &ZipExpansionLimits<'_>,
) -> Result<(), AttachError> {
    let mut archive = ZipArchive::new(Cursor::new(file_bytes))
        .map_err(|e| AttachError::InvalidArchive(format!("ZIP 아카이브를 열 수 없습니다: {e}")))?;

    let count = archive.len();
    if count > limits.max_entries {
        return Err(AttachError::LimitExceeded(format!(
            "ZIP 항목 수가 허용 한도({})를 초과했습니다.",
            limits.max_entries
        )));
    }

    let mut expanded_total: u64 = 0;
    for i in 0..count {
        let entry = archive
            .by_index_raw(i)
            .map_err(|e| AttachError::InvalidArchive(format!("ZIP 항목 메타데이터 오류: {e}")))?;
        let name = entry.name().to_string();

        if let Some(filter) = limits.include_file {
            if !filter(&name) {
                continue;
            }
        }

        let expanded_size = entry.size();
        if expanded_size > limits.max_entry_bytes {
            return Err(AttachError::LimitExceeded(format!(
                "ZIP 항목의 압축 해제 크기가 허용 한도({} bytes)를 초과했습니다: {name}",
                limits.max_entry_bytes
            )));
        }

        expanded_total = expanded_total
            .checked_add(expanded_size)
            .ok_or_else(|| {
                AttachError::LimitExceeded(format!(
                    "ZIP 전체 압축 해제 크기가 허용 한도({} bytes)를 초과했습니다.",
                    limits.max_total_bytes
                ))
            })?;

        if expanded_total > limits.max_total_bytes {
            return Err(AttachError::LimitExceeded(format!(
                "ZIP 전체 압축 해제 크기가 허용 한도({} bytes)를 초과했습니다.",
                limits.max_total_bytes
            )));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    use zip::{CompressionMethod, ZipWriter};

    fn make_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = Cursor::new(Vec::new());
        {
            let mut writer = ZipWriter::new(&mut buf);
            let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
            for (name, data) in entries {
                writer.start_file(name, opts).unwrap();
                writer.write_all(data).unwrap();
            }
            writer.finish().unwrap();
        }
        buf.into_inner()
    }

    #[test]
    fn size_limit_accepts_exact_128mib_and_rejects_above() {
        assert!(assert_attachment_file_size(MAX_ATTACHMENT_FILE_BYTES).is_ok());
        assert!(assert_attachment_file_size(MAX_ATTACHMENT_FILE_BYTES + 1).is_err());
    }

    #[test]
    fn zip_preflight_blocks_zip_bomb_by_uncompressed_metadata() {
        let big = vec![b'A'; 4096];
        let zip_bytes = make_zip(&[("word/document.xml", &big)]);

        let strict = ZipExpansionLimits {
            max_entry_bytes: 1024,
            max_total_bytes: 4096,
            max_entries: 100,
            include_file: None,
        };
        let err = assert_zip_expansion_within_limits(&zip_bytes, &strict).unwrap_err();
        assert!(matches!(err, AttachError::LimitExceeded(_)));

        let permissive = ZipExpansionLimits::new(4096, 4096);
        assert!(assert_zip_expansion_within_limits(&zip_bytes, &permissive).is_ok());
    }

    #[test]
    fn zip_preflight_enforces_entry_count_limit() {
        let zip_bytes = make_zip(&[("a.txt", b"1"), ("b.txt", b"2"), ("c.txt", b"3")]);
        let limits = ZipExpansionLimits {
            max_entry_bytes: 1024,
            max_total_bytes: 4096,
            max_entries: 2,
            include_file: None,
        };
        let err = assert_zip_expansion_within_limits(&zip_bytes, &limits).unwrap_err();
        assert!(err.to_string().contains("항목 수"));
    }
}
