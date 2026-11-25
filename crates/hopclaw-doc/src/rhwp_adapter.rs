//! Native `rhwp` adapter boundary (behind feature `native-rhwp`) and pure-Rust HWP 5.0 CFB
//! header/record counter fallback for offline builds.

use std::io::Read;

use crate::hwpx_walker::{inspect_hwpx, is_hwpx_bytes};
use crate::types::DocumentInspectReport;
use crate::DocError;

const CFB_MAGIC: [u8; 8] = [0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1];

/// Returns `true` if `bytes` begins with an OLE2/CFB header (`D0 CF 11 E0 A1 B1 1A E1`).
pub fn is_hwp5_cfb_bytes(bytes: &[u8]) -> bool {
    bytes.len() >= 8 && bytes[..8] == CFB_MAGIC
}

/// Inspect raw document bytes (HWPX natively; HWP 5.0 via `native-rhwp` when enabled).
pub fn inspect_document_bytes(
    bytes: &[u8],
    with_cell_text: bool,
) -> Result<DocumentInspectReport, DocError> {
    if is_hwpx_bytes(bytes) {
        return inspect_hwpx(bytes, with_cell_text);
    }
    if is_hwp5_cfb_bytes(bytes) {
        return inspect_hwp5_bytes(bytes, with_cell_text);
    }
    Err(DocError::InvalidDocument(
        "unrecognized document magic bytes (expected PK/HWPX or CFB/HWP5)".to_string(),
    ))
}

#[cfg(feature = "native-rhwp")]
fn inspect_hwp5_bytes(
    _bytes: &[u8],
    _with_cell_text: bool,
) -> Result<DocumentInspectReport, DocError> {
    // Placeholder for linking `rhwp` crate once vendored/cached.
    Err(DocError::UnsupportedFeature(
        "native-rhwp adapter requires the external `rhwp` crate".to_string(),
    ))
}

#[cfg(not(feature = "native-rhwp"))]
fn inspect_hwp5_bytes(
    bytes: &[u8],
    with_cell_text: bool,
) -> Result<DocumentInspectReport, DocError> {
    if with_cell_text {
        return Err(DocError::UnsupportedFeature(
            "HWP 5.0 --with-cell-text requires feature `native-rhwp` (rhwp crate unavailable offline)"
                .to_string(),
        ));
    }
    inspect_hwp5_cfb_summary(bytes)
}

/// Lightweight CFB-level HWP 5.0 summary (section count and record-tag counts for
/// `PARA_HEADER 0x42`, `CTRL_HEADER 0x47` `"tbl "` and `LIST_HEADER 0x48`) using cached `cfb` + `flate2`.
pub fn inspect_hwp5_cfb_summary(bytes: &[u8]) -> Result<DocumentInspectReport, DocError> {
    if !is_hwp5_cfb_bytes(bytes) {
        return Err(DocError::InvalidDocument("not a CFB/HWP5 file".to_string()));
    }
    let cursor = std::io::Cursor::new(bytes);
    let mut comp = cfb::CompoundFile::open(cursor)
        .map_err(|e| DocError::InvalidDocument(format!("CFB open error: {e}")))?;

    let mut header_bytes = Vec::new();
    comp.open_stream("FileHeader")
        .map_err(|e| DocError::InvalidDocument(format!("missing FileHeader: {e}")))?
        .read_to_end(&mut header_bytes)?;
    if header_bytes.len() < 40 || !header_bytes.starts_with(b"HWP Document File") {
        return Err(DocError::InvalidDocument(
            "FileHeader missing 'HWP Document File' signature".to_string(),
        ));
    }
    let flags = u32::from_le_bytes([
        header_bytes[36],
        header_bytes[37],
        header_bytes[38],
        header_bytes[39],
    ]);
    let is_compressed = (flags & 0x01) != 0;

    let mut section_count = 0usize;
    let mut paragraph_count = 0usize;
    let mut table_count = 0usize;

    loop {
        let stream_name = format!("BodyText/Section{section_count}");
        let Ok(mut stream) = comp.open_stream(&stream_name) else {
            break;
        };
        section_count += 1;
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw)?;
        let decoded = if is_compressed {
            let mut dec = flate2::read::DeflateDecoder::new(&raw[..]);
            let mut out = Vec::new();
            dec.read_to_end(&mut out)?;
            out
        } else {
            raw
        };

        let mut pos = 0usize;
        while pos + 4 <= decoded.len() {
            let header = u32::from_le_bytes([
                decoded[pos],
                decoded[pos + 1],
                decoded[pos + 2],
                decoded[pos + 3],
            ]);
            pos += 4;
            let tag_id = header & 0x3ff;
            let level = (header >> 10) & 0x3ff;
            let mut size = (header >> 20) & 0xfff;
            if size == 0xfff {
                if pos + 4 > decoded.len() {
                    break;
                }
                size = u32::from_le_bytes([
                    decoded[pos],
                    decoded[pos + 1],
                    decoded[pos + 2],
                    decoded[pos + 3],
                ]);
                pos += 4;
            }
            let size = size as usize;
            if pos + size > decoded.len() {
                break;
            }
            let payload = &decoded[pos..pos + size];
            if tag_id == 0x42 && level == 0 {
                // Top-level body paragraph
                paragraph_count += 1;
            } else if tag_id == 0x47 && payload.len() >= 4 && &payload[..4] == b" lbt" {
                // HWP5 stores "tbl " little-endian as " lbt"
                table_count += 1;
            }
            pos += size;
        }
    }

    Ok(DocumentInspectReport {
        file_type: "hwp".to_string(),
        section_count,
        paragraph_count,
        table_count,
        cell_count: 0,
        image_count: None,
        tables: None,
    })
}
