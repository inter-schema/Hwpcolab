use crate::error::{Hwp5Error, Result};

/// Canonical HWP 5.0 binary record tag IDs (10-bit `HWPTAG_*`).
pub mod tag {
    // Base offset: HWPTAG_BEGIN = 0x10
    pub const DOCUMENT_PROPERTIES: u16 = 0x10;
    pub const ID_MAPPINGS: u16 = 0x11;
    pub const BIN_DATA: u16 = 0x12;
    pub const FACE_NAME: u16 = 0x13;
    pub const BORDER_FILL: u16 = 0x14;
    pub const CHAR_SHAPE: u16 = 0x15;
    pub const TAB_DEF: u16 = 0x16;
    pub const NUMBERING: u16 = 0x17;
    pub const BULLET: u16 = 0x18;
    pub const PARA_SHAPE: u16 = 0x19;
    pub const STYLE: u16 = 0x1A;
    pub const DOC_DATA: u16 = 0x1B;
    pub const DISTRIBUTE_DOC_DATA: u16 = 0x1C;
    pub const COMPATIBLE_DOCUMENT: u16 = 0x1E;
    pub const LAYOUT_COMPATIBILITY: u16 = 0x1F;
    pub const TRACKCHANGE: u16 = 0x20;
    pub const MEMO_SHAPE_DOC: u16 = 0x5C;

    // BodyText tags
    pub const PARA_HEADER: u16 = 0x42;
    pub const PARA_TEXT: u16 = 0x43;
    pub const PARA_CHAR_SHAPE: u16 = 0x44;
    pub const PARA_LINE_SEG: u16 = 0x45;
    pub const PARA_RANGE_TAG: u16 = 0x46;
    pub const CTRL_HEADER: u16 = 0x47;
    pub const LIST_HEADER: u16 = 0x48;
    pub const PAGE_DEF: u16 = 0x49;
    pub const FOOTNOTE_SHAPE: u16 = 0x4A;
    pub const PAGE_BORDER_FILL: u16 = 0x4B;
    pub const SHAPE_COMPONENT: u16 = 0x4C;
    pub const TABLE: u16 = 0x4D;
    pub const SHAPE_COMPONENT_LINE: u16 = 0x4E;
    pub const SHAPE_COMPONENT_RECTANGLE: u16 = 0x4F;
    pub const SHAPE_COMPONENT_ELLIPSE: u16 = 0x50;
    pub const SHAPE_COMPONENT_ARC: u16 = 0x51;
    pub const SHAPE_COMPONENT_POLYGON: u16 = 0x52;
    pub const SHAPE_COMPONENT_CURVE: u16 = 0x53;
    pub const SHAPE_COMPONENT_OLE: u16 = 0x54;
    pub const SHAPE_COMPONENT_PICTURE: u16 = 0x55;
    pub const SHAPE_COMPONENT_CONTAINER: u16 = 0x56;
    pub const CTRL_DATA: u16 = 0x57;
    pub const EQEDIT: u16 = 0x58;
    pub const SHAPE_COMPONENT_TEXTART: u16 = 0x5A;
    pub const FORM_OBJECT: u16 = 0x5B;
    pub const MEMO_SHAPE: u16 = 0x5C;
    pub const MEMO_LIST: u16 = 0x5D;
    pub const FORBIDDEN_CHAR: u16 = 0x5E;
    pub const CHART_DATA: u16 = 0x5F;
    pub const VIDEO_DATA: u16 = 0x62;
    pub const SHAPE_COMPONENT_UNKNOWN: u16 = 0x63;
}

/// 12-bit record header size sentinel indicating a trailing 32-bit LE length field.
pub const EXTENDED_SIZE_SENTINEL: usize = 0xFFF;

/// Single decoded HWP 5.0 binary record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawRecord {
    /// 10-bit tag identifier (`0..=0x3FF`).
    pub tag: u16,
    /// 10-bit tree nesting level (`0..=0x3FF`).
    pub level: u16,
    /// Raw record payload bytes.
    pub body: Vec<u8>,
}

impl RawRecord {
    /// Construct a new binary record.
    pub fn new(tag: u16, level: u16, body: Vec<u8>) -> Self {
        Self { tag, level, body }
    }

    /// Encode this record (header + optional extended length + payload) into `out`.
    pub fn write_to(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.tag > 0x3FF {
            return Err(Hwp5Error::MalformedRecord(format!(
                "record tag 0x{:X} exceeds 10-bit limit",
                self.tag
            )));
        }
        if self.level > 0x3FF {
            return Err(Hwp5Error::MalformedRecord(format!(
                "record level {} exceeds 10-bit limit",
                self.level
            )));
        }
        let len = self.body.len();
        if len < EXTENDED_SIZE_SENTINEL {
            let hdr = (self.tag as u32) | ((self.level as u32) << 10) | ((len as u32) << 20);
            out.extend_from_slice(&hdr.to_le_bytes());
        } else {
            let len_u32 = u32::try_from(len).map_err(|_| {
                Hwp5Error::MalformedRecord(format!("record length {len} exceeds u32::MAX"))
            })?;
            let hdr = (self.tag as u32)
                | ((self.level as u32) << 10)
                | ((EXTENDED_SIZE_SENTINEL as u32) << 20);
            out.extend_from_slice(&hdr.to_le_bytes());
            out.extend_from_slice(&len_u32.to_le_bytes());
        }
        out.extend_from_slice(&self.body);
        Ok(())
    }

    /// Total serialized byte length of this record (header + payload).
    pub fn encoded_len(&self) -> usize {
        let hdr_len = if self.body.len() < EXTENDED_SIZE_SENTINEL { 4 } else { 8 };
        hdr_len + self.body.len()
    }
}

/// Parse an entire decompressed HWP 5.0 stream (`DocInfo` or `BodyText/SectionN`) into records.
pub fn parse_records(data: &[u8]) -> Result<Vec<RawRecord>> {
    let mut records = Vec::new();
    let mut offset = 0usize;

    while offset < data.len() {
        if offset + 4 > data.len() {
            return Err(Hwp5Error::MalformedRecord(format!(
                "truncated record header at offset {offset} ({} trailing bytes)",
                data.len() - offset
            )));
        }
        let hdr = u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap());
        let tag = (hdr & 0x3FF) as u16;
        let level = ((hdr >> 10) & 0x3FF) as u16;
        let inline_size = ((hdr >> 20) & 0xFFF) as usize;

        let (size, header_len) = if inline_size == EXTENDED_SIZE_SENTINEL {
            if offset + 8 > data.len() {
                return Err(Hwp5Error::MalformedRecord(format!(
                    "truncated extended record length at offset {offset}"
                )));
            }
            let ext = u32::from_le_bytes(data[offset + 4..offset + 8].try_into().unwrap()) as usize;
            (ext, 8usize)
        } else {
            (inline_size, 4usize)
        };

        let body_start = offset + header_len;
        let body_end = body_start.checked_add(size).ok_or_else(|| {
            Hwp5Error::MalformedRecord(format!("record size overflow at offset {offset}"))
        })?;
        if body_end > data.len() {
            return Err(Hwp5Error::MalformedRecord(format!(
                "record at offset {offset} (tag=0x{tag:02X}, level={level}, size={size}) overruns stream length {}",
                data.len()
            )));
        }

        records.push(RawRecord {
            tag,
            level,
            body: data[body_start..body_end].to_vec(),
        });
        offset = body_end;
    }

    Ok(records)
}

/// Serialize records back into raw HWP 5.0 stream bytes.
pub fn serialize_records(records: &[RawRecord]) -> Result<Vec<u8>> {
    let total: usize = records.iter().map(RawRecord::encoded_len).sum();
    let mut out = Vec::with_capacity(total);
    for rec in records {
        rec.write_to(&mut out)?;
    }
    Ok(out)
}

/// Find the end index (exclusive) of a record subtree rooted at `start` (any record with `level <= root.level` stops it).
pub fn subtree_end(records: &[RawRecord], start: usize) -> usize {
    let root_level = records[start].level;
    let mut idx = start + 1;
    while idx < records.len() && records[idx].level > root_level {
        idx += 1;
    }
    idx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_standard_and_extended_records() {
        let small = RawRecord::new(tag::PARA_HEADER, 0, vec![1, 2, 3, 4]);
        let boundary = RawRecord::new(tag::PARA_TEXT, 1, vec![0xAA; EXTENDED_SIZE_SENTINEL - 1]);
        let exact_sentinel = RawRecord::new(tag::PARA_TEXT, 1, vec![0xBB; EXTENDED_SIZE_SENTINEL]);
        let large = RawRecord::new(tag::CTRL_HEADER, 2, vec![0xCC; 5000]);

        let input = vec![small, boundary, exact_sentinel, large];
        let encoded = serialize_records(&input).expect("serialize");
        // Verify header sizes: 4, 4, 8, 8
        assert_eq!(input[0].encoded_len(), 4 + 4);
        assert_eq!(input[1].encoded_len(), 4 + 0xFFE);
        assert_eq!(input[2].encoded_len(), 8 + 0xFFF);
        assert_eq!(input[3].encoded_len(), 8 + 5000);

        let decoded = parse_records(&encoded).expect("parse");
        assert_eq!(decoded, input);
        let reencoded = serialize_records(&decoded).expect("re-serialize");
        assert_eq!(reencoded, encoded);
    }

    #[test]
    fn rejects_truncated_header_or_body() {
        assert!(parse_records(&[0x42, 0x00]).is_err());
        // Declares size 10 but only 2 payload bytes follow
        let bad = [0x42, 0x00, 0xA0, 0x00, 0x01, 0x02];
        assert!(parse_records(&bad).is_err());
    }
}
