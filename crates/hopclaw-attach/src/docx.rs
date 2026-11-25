use quick_xml::events::Event;
use quick_xml::name::QName;
use quick_xml::Reader;
use std::io::{Cursor, Read};
use zip::ZipArchive;

use crate::error::AttachError;
use crate::preflight::{
    assert_zip_expansion_within_limits, ZipExpansionLimits, MAX_DOCX_DOCUMENT_XML_BYTES,
};

const WORDPROCESSINGML_NS: &[u8] =
    b"http://schemas.openxmlformats.org/wordprocessingml/2006/main";

fn local_name(bytes: &[u8]) -> &[u8] {
    if let Some(pos) = bytes.iter().rposition(|&b| b == b':') {
        &bytes[pos + 1..]
    } else {
        bytes
    }
}

fn is_word_local(name: QName<'_>, expected: &[u8]) -> bool {
    local_name(name.as_ref().as_bytes()) == expected
}

/// Parses DOCX bytes into Markdown, matching `parseDocxBytesToMarkdown` in
/// `src/core/multiFileParser.ts` (including ZIP-bomb preflight and bounded XML read).
pub fn parse_docx_bytes_to_markdown(file_bytes: &[u8]) -> Result<String, AttachError> {
    let include = |name: &str| name == "word/document.xml";
    assert_zip_expansion_within_limits(
        file_bytes,
        &ZipExpansionLimits {
            max_entry_bytes: MAX_DOCX_DOCUMENT_XML_BYTES,
            max_total_bytes: MAX_DOCX_DOCUMENT_XML_BYTES,
            max_entries: 10_000,
            include_file: Some(&include),
        },
    )?;

    let mut archive = ZipArchive::new(Cursor::new(file_bytes))
        .map_err(|e| AttachError::InvalidArchive(format!("DOCX ZIP 열기 실패: {e}")))?;

    let entry = archive
        .by_name("word/document.xml")
        .map_err(|_| AttachError::ParseFailed("word/document.xml을 찾을 수 없습니다.".into()))?;

    let mut xml_buf = Vec::new();
    entry
        .take(MAX_DOCX_DOCUMENT_XML_BYTES + 1)
        .read_to_end(&mut xml_buf)?;
    if xml_buf.len() as u64 > MAX_DOCX_DOCUMENT_XML_BYTES {
        return Err(AttachError::LimitExceeded(format!(
            "word/document.xml 크기가 허용 한도({MAX_DOCX_DOCUMENT_XML_BYTES} bytes)를 초과했습니다."
        )));
    }

    let xml = String::from_utf8(xml_buf)
        .map_err(|e| AttachError::ParseFailed(format!("word/document.xml UTF-8 오류: {e}")))?;

    extract_markdown_from_word_xml(&xml)
}

pub fn extract_markdown_from_word_xml(xml: &str) -> Result<String, AttachError> {
    let _ = WORDPROCESSINGML_NS; // kept for documentation parity
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);

    let mut in_body = false;
    let mut skip_depth: usize = 0;
    let mut in_text_elem = false;

    // Paragraph stack: each active paragraph buffers into `para_buf`
    let mut in_para = false;
    let mut para_buf = String::new();

    // Table state: supports top-level tables and cells (cells can have multiple paragraphs)
    let mut in_table = false;
    let mut table_rows: Vec<Vec<String>> = Vec::new();
    let mut current_row: Vec<String> = Vec::new();
    let mut in_cell = false;
    let mut cell_blocks: Vec<String> = Vec::new();

    let mut body_blocks: Vec<String> = Vec::new();
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let name = e.name();
                if skip_depth > 0 {
                    skip_depth += 1;
                } else if is_word_local(name, b"body") {
                    in_body = true;
                } else if !in_body {
                    // ignore outside body
                } else if is_word_local(name, b"del")
                    || is_word_local(name, b"delText")
                    || is_word_local(name, b"instrText")
                    || is_word_local(name, b"pPr")
                    || is_word_local(name, b"rPr")
                {
                    skip_depth = 1;
                } else if is_word_local(name, b"tbl") && !in_table {
                    in_table = true;
                    table_rows.clear();
                } else if is_word_local(name, b"tr") && in_table {
                    current_row.clear();
                } else if is_word_local(name, b"tc") && in_table {
                    in_cell = true;
                    cell_blocks.clear();
                } else if is_word_local(name, b"p") {
                    in_para = true;
                    para_buf.clear();
                } else if is_word_local(name, b"t") && in_para {
                    in_text_elem = true;
                } else if is_word_local(name, b"tab") && in_para {
                    para_buf.push('\t');
                } else if (is_word_local(name, b"br") || is_word_local(name, b"cr")) && in_para {
                    para_buf.push('\n');
                } else if is_word_local(name, b"noBreakHyphen") && in_para {
                    para_buf.push('\u{2011}');
                } else if is_word_local(name, b"softHyphen") && in_para {
                    para_buf.push('\u{00ad}');
                }
            }
            Ok(Event::Empty(e)) => {
                if in_body && skip_depth == 0 && in_para {
                    let name = e.name();
                    if is_word_local(name, b"tab") {
                        para_buf.push('\t');
                    } else if is_word_local(name, b"br") || is_word_local(name, b"cr") {
                        para_buf.push('\n');
                    } else if is_word_local(name, b"noBreakHyphen") {
                        para_buf.push('\u{2011}');
                    } else if is_word_local(name, b"softHyphen") {
                        para_buf.push('\u{00ad}');
                    }
                }
            }
            Ok(Event::Text(e)) => {
                if in_body && skip_depth == 0 && in_text_elem {
                    let text = quick_xml::escape::unescape(e.as_ref()).map_err(|err| {
                        AttachError::ParseFailed(format!("XML 디코딩 오류: {err}"))
                    })?;
                    para_buf.push_str(&text);
                }
            }
            Ok(Event::CData(e)) => {
                if in_body && skip_depth == 0 && in_text_elem {
                    let text = String::from_utf8_lossy(e.as_ref().as_bytes());
                    para_buf.push_str(&text);
                }
            }
            Ok(Event::End(e)) => {
                let name = e.name();
                if skip_depth > 0 {
                    skip_depth -= 1;
                } else if is_word_local(name, b"body") {
                    in_body = false;
                } else if !in_body {
                    // ignore
                } else if is_word_local(name, b"t") {
                    in_text_elem = false;
                } else if is_word_local(name, b"p") {
                    in_para = false;
                    if !para_buf.is_empty() {
                        if in_cell {
                            cell_blocks.push(std::mem::take(&mut para_buf));
                        } else if !in_table {
                            body_blocks.push(std::mem::take(&mut para_buf));
                        }
                    }
                    para_buf.clear();
                } else if is_word_local(name, b"tc") && in_cell {
                    in_cell = false;
                    current_row.push(cell_blocks.join("<br>"));
                    cell_blocks.clear();
                } else if is_word_local(name, b"tr") && in_table {
                    table_rows.push(std::mem::take(&mut current_row));
                } else if is_word_local(name, b"tbl") && in_table {
                    in_table = false;
                    let rendered = render_word_table(&table_rows);
                    table_rows.clear();
                    if !rendered.is_empty() {
                        body_blocks.push(rendered);
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(AttachError::ParseFailed(format!(
                    "word/document.xml의 XML 형식이 올바르지 않습니다: {e}"
                )));
            }
            _ => {}
        }
        buf.clear();
    }

    Ok(body_blocks.join("\n\n"))
}

fn render_word_table(rows: &[Vec<String>]) -> String {
    if rows.is_empty() {
        return String::new();
    }
    let col_count = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    if col_count == 0 {
        return String::new();
    }

    let render_row = |row: &[String]| -> String {
        let cells: Vec<String> = (0..col_count)
            .map(|idx| {
                row.get(idx)
                    .map(|s| {
                        s.replace('|', "\\|")
                            .replace("\r\n", "<br>")
                            .replace('\n', "<br>")
                    })
                    .unwrap_or_default()
            })
            .collect();
        format!("| {} |", cells.join(" | "))
    };

    let header = render_row(&rows[0]);
    let divider = format!(
        "| {} |",
        vec!["---"; col_count].join(" | ")
    );
    let mut lines = vec![header, divider];
    for row in &rows[1..] {
        lines.push(render_row(row));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    fn make_docx(document_xml: &str) -> Vec<u8> {
        let mut buf = Cursor::new(Vec::new());
        {
            let mut writer = ZipWriter::new(&mut buf);
            writer
                .start_file("word/document.xml", SimpleFileOptions::default())
                .unwrap();
            writer.write_all(document_xml.as_bytes()).unwrap();
            writer.finish().unwrap();
        }
        buf.into_inner()
    }

    #[test]
    fn parses_paragraphs_and_tables_and_skips_deleted_text() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
        <w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
          <w:body>
            <w:p>
              <w:pPr><w:jc w:val="center"/></w:pPr>
              <w:r><w:t>첫 번째 문단</w:t><w:tab/><w:t>끝</w:t></w:r>
              <w:del><w:r><w:delText>삭제됨</w:delText></w:r></w:del>
            </w:p>
            <w:tbl>
              <w:tr>
                <w:tc><w:p><w:r><w:t>항목</w:t></w:r></w:p></w:tc>
                <w:tc><w:p><w:r><w:t>금액</w:t></w:r></w:p></w:tc>
              </w:tr>
              <w:tr>
                <w:tc><w:p><w:r><w:t>인건비</w:t></w:r></w:p></w:tc>
                <w:tc><w:p><w:r><w:t>10,000</w:t></w:r></w:p></w:tc>
              </w:tr>
            </w:tbl>
          </w:body>
        </w:document>"#;

        let bytes = make_docx(xml);
        let md = parse_docx_bytes_to_markdown(&bytes).unwrap();
        assert!(md.contains("첫 번째 문단\t끝"));
        assert!(!md.contains("삭제됨"));
        assert!(md.contains("| 항목 | 금액 |"));
        assert!(md.contains("| 인건비 | 10,000 |"));
    }
}
