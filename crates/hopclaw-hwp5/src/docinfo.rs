use crate::error::{Hwp5Error, Result};
use crate::record::{parse_records, serialize_records, tag, RawRecord};

/// Byte offsets of count slots inside `HWPTAG_ID_MAPPINGS` (`0x11`).
pub mod id_slot {
    pub const BIN_DATA: usize = 0;
    pub const FACE_NAME_START: usize = 1; // 7 slots: 1..=7
    pub const BORDER_FILL: usize = 8;
    pub const CHAR_SHAPE: usize = 9;
    pub const TAB_DEF: usize = 10;
    pub const NUMBERING: usize = 11;
    pub const BULLET: usize = 12;
    pub const PARA_SHAPE: usize = 13;
    pub const STYLE: usize = 14;
    pub const MEMO_SHAPE: usize = 15;
    pub const TRACK_CHANGE: usize = 16;
    pub const TRACK_CHANGE_AUTHOR: usize = 17;
}

/// Canonical HWP 5.0 native NUMBERING record payload (`^1.` multi-level decimal list).
pub const DEFAULT_NUMBERING_BYTES: &[u8] = &[
    0x0c, 0x00, 0x00, 0x00, 0x00, 0x00, 0x32, 0x00, 0xff, 0xff, 0xff, 0xff, 0x03, 0x00, 0x5e, 0x00,
    0x31, 0x00, 0x2e, 0x00, 0x0c, 0x01, 0x00, 0x00, 0x00, 0x00, 0x32, 0x00, 0xff, 0xff, 0xff, 0xff,
    0x03, 0x00, 0x5e, 0x00, 0x32, 0x00, 0x2e, 0x00, 0x0c, 0x00, 0x00, 0x00, 0x00, 0x00, 0x32, 0x00,
    0xff, 0xff, 0xff, 0xff, 0x03, 0x00, 0x5e, 0x00, 0x33, 0x00, 0x29, 0x00, 0x0c, 0x01, 0x00, 0x00,
    0x00, 0x00, 0x32, 0x00, 0xff, 0xff, 0xff, 0xff, 0x03, 0x00, 0x5e, 0x00, 0x34, 0x00, 0x29, 0x00,
    0x0c, 0x00, 0x00, 0x00, 0x00, 0x00, 0x32, 0x00, 0xff, 0xff, 0xff, 0xff, 0x04, 0x00, 0x28, 0x00,
    0x5e, 0x00, 0x35, 0x00, 0x29, 0x00, 0x0c, 0x01, 0x00, 0x00, 0x00, 0x00, 0x32, 0x00, 0xff, 0xff,
    0xff, 0xff, 0x04, 0x00, 0x28, 0x00, 0x5e, 0x00, 0x36, 0x00, 0x29, 0x00, 0x2c, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x32, 0x00, 0xff, 0xff, 0xff, 0xff, 0x02, 0x00, 0x5e, 0x00, 0x37, 0x00, 0x01, 0x00,
    0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00,
    0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x2c, 0x01, 0x00, 0x00,
    0x00, 0x00, 0x32, 0x00, 0xff, 0xff, 0xff, 0xff, 0x02, 0x00, 0x5e, 0x00, 0x38, 0x00, 0x0c, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x32, 0x00, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x0c, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x32, 0x00, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00,
    0x00, 0x00, 0x01, 0x00, 0x00, 0x00,
];

/// Canonical HWP 5.0 native BULLET record payload (`U+F06C` filled circle).
pub const DEFAULT_BULLET_BYTES: &[u8] = &[
    0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x32, 0x00, 0xff, 0xff, 0xff, 0xff, 0x6c, 0xf0, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0x00,
];

/// Convert `(r, g, b)` components into HWP 5.0 `COLORREF` (`0x00BBGGRR`).
pub const fn rgb_to_bgr(r: u8, g: u8, b: u8) -> u32 {
    (r as u32) | ((g as u32) << 8) | ((b as u32) << 16)
}

/// Parse `#RRGGBB` or `RRGGBB` CSS hex color into HWP 5.0 `COLORREF` (`0x00BBGGRR`).
pub fn parse_hex_color(hex: &str) -> Result<u32> {
    let trimmed = hex.trim().strip_prefix('#').unwrap_or(hex.trim());
    if trimmed.len() != 6 {
        return Err(Hwp5Error::InvalidTarget(format!(
            "invalid hex color '{hex}', expected #RRGGBB"
        )));
    }
    let r = u8::from_str_radix(&trimmed[0..2], 16)
        .map_err(|_| Hwp5Error::InvalidTarget(format!("invalid hex color '{hex}'")))?;
    let g = u8::from_str_radix(&trimmed[2..4], 16)
        .map_err(|_| Hwp5Error::InvalidTarget(format!("invalid hex color '{hex}'")))?;
    let b = u8::from_str_radix(&trimmed[4..6], 16)
        .map_err(|_| Hwp5Error::InvalidTarget(format!("invalid hex color '{hex}'")))?;
    Ok(rgb_to_bgr(r, g, b))
}

/// Border side inside a `BORDER_FILL` record (`0x14`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BorderSide {
    Left,
    Right,
    Top,
    Bottom,
}

impl BorderSide {
    /// Fixed byte offset of this side's 6-byte descriptor `(type:u8, width:u8, color:u32)`.
    pub const fn byte_offset(self) -> usize {
        match self {
            Self::Left => 2,
            Self::Right => 8,
            Self::Top => 14,
            Self::Bottom => 20,
        }
    }
}

/// HWP 5.0 border / diagonal line style enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum BorderLineType {
    None = 0,
    Solid = 1,
    Dash = 2,
    Dot = 3,
    Double = 4,
}

/// Diagonal glyph direction inside a `BORDER_FILL` record (`0x14`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagonalDirection {
    /// Bottom-left to top-right (`/`), attribute bit 3 (`0x08`).
    Slash,
    /// Top-left to bottom-right (`\`), attribute bit 6 (`0x40`).
    Backslash,
    /// Crossed (`X`), bits 3 & 6 (`0x48`).
    Cross,
}

impl DiagonalDirection {
    pub const fn attr_bits(self) -> u16 {
        match self {
            Self::Slash => 1 << 3,
            Self::Backslash => 1 << 6,
            Self::Cross => (1 << 3) | (1 << 6),
        }
    }
}

/// Helper for building and merging `HWPTAG_BORDER_FILL` (`0x14`) record payloads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BorderFillBuilder;

impl BorderFillBuilder {
    /// Merge a solid background color into an existing `BorderFill` body, preserving the
    /// 32-byte border + diagonal block and emitting a 53-byte solid-fill body.
    pub fn merge_solid_fill(base: &[u8], bgr_color: u32, with_size_marker: bool) -> Vec<u8> {
        let mut out = vec![0u8; 53];
        let copy_len = base.len().min(32);
        out[..copy_len].copy_from_slice(&base[..copy_len]);
        out[32..36].copy_from_slice(&1u32.to_le_bytes()); // fill_type = solid
        out[36..40].copy_from_slice(&bgr_color.to_le_bytes());
        out[40..44].copy_from_slice(&0x0099_9999u32.to_le_bytes()); // pattern color
        out[44..48].copy_from_slice(&(-1i32).to_le_bytes()); // pattern_type = -1 (solid)
        if with_size_marker {
            out[48..52].copy_from_slice(&1u32.to_le_bytes());
        }
        out
    }

    /// Update one or more border sides while preserving existing diagonal, fill, and untouched sides.
    pub fn merge_borders(
        base: &[u8],
        sides: &[BorderSide],
        line_type: u8,
        width: u8,
        bgr_color: u32,
    ) -> Vec<u8> {
        let mut out = vec![0u8; base.len().max(32)];
        out[..base.len()].copy_from_slice(base);
        for side in sides {
            let off = side.byte_offset();
            out[off] = line_type;
            out[off + 1] = width;
            out[off + 2..off + 6].copy_from_slice(&bgr_color.to_le_bytes());
        }
        out
    }

    /// Set a cell diagonal line while preserving existing borders and fill.
    pub fn merge_diagonal(
        base: &[u8],
        direction: DiagonalDirection,
        line_type: u8,
        width: u8,
        bgr_color: u32,
    ) -> Vec<u8> {
        let mut out = vec![0u8; base.len().max(32)];
        out[..base.len()].copy_from_slice(base);
        let attr = u16::from_le_bytes(out[0..2].try_into().unwrap()) | direction.attr_bits();
        out[0..2].copy_from_slice(&attr.to_le_bytes());
        out[26] = line_type;
        out[27] = width;
        out[28..32].copy_from_slice(&bgr_color.to_le_bytes());
        out
    }
}

/// Overlay patch applied onto an existing `HWPTAG_CHAR_SHAPE` (`0x15`) body (70 or 74 bytes).
#[derive(Debug, Default, Clone, PartialEq)]
pub struct CharShapePatch {
    pub face_ids: Option<[u16; 7]>,
    pub size_pt: Option<f64>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
    pub strikethrough: Option<bool>,
    pub superscript: Option<bool>,
    pub subscript: Option<bool>,
    pub text_color_bgr: Option<u32>,
    pub underline_color_bgr: Option<u32>,
    pub strikethrough_color_bgr: Option<u32>,
    pub shade_color_bgr: Option<u32>,
    pub char_ratio: Option<u8>,
    pub letter_spacing: Option<i8>,
}

impl CharShapePatch {
    /// Clone `base` (`70` or `74` bytes) preserving its exact byte length and unmanaged bits.
    pub fn apply_to(&self, base: &[u8]) -> Result<Vec<u8>> {
        if base.len() != 70 && base.len() != 74 {
            return Err(Hwp5Error::DocInfoCorrupt(format!(
                "CharShape body must be 70 or 74 bytes (got {})",
                base.len()
            )));
        }
        let mut out = base.to_vec();

        if let Some(ids) = self.face_ids {
            for (i, id) in ids.iter().enumerate() {
                out[i * 2..i * 2 + 2].copy_from_slice(&id.to_le_bytes());
            }
        }
        if let Some(ratio) = self.char_ratio {
            let v = ratio.max(1);
            out[14..21].fill(v);
        }
        if let Some(sp) = self.letter_spacing {
            for byte in &mut out[21..28] {
                *byte = sp as u8;
            }
        }
        if let Some(pt) = self.size_pt {
            let hwp_units = (pt * 100.0).round() as i32;
            out[42..46].copy_from_slice(&hwp_units.to_le_bytes());
        }

        let mut attr = u32::from_le_bytes(out[46..50].try_into().unwrap());
        let set_flag = |attr: &mut u32, mask: u32, val: Option<bool>| {
            if let Some(on) = val {
                if on {
                    *attr |= mask;
                } else {
                    *attr &= !mask;
                }
            }
        };
        set_flag(&mut attr, 0x0000_0001, self.italic);
        set_flag(&mut attr, 0x0000_0002, self.bold);
        if let Some(u) = self.underline {
            attr &= !0x0000_000C;
            if u {
                attr |= 0x0000_0004;
            }
        }
        if let Some(s) = self.strikethrough {
            attr &= !0x001C_0000;
            if s {
                // Bit 3 + bit 18 matches Hancom Office's strike pattern.
                attr |= 0x0004_0008;
            } else {
                // Only clear bit 3 if underline was not explicitly enabled
                if !matches!(self.underline, Some(true)) {
                    attr &= !0x0000_0008;
                }
            }
        }
        set_flag(&mut attr, 0x0000_8000, self.superscript);
        set_flag(&mut attr, 0x0001_0000, self.subscript);
        out[46..50].copy_from_slice(&attr.to_le_bytes());

        if let Some(c) = self.text_color_bgr {
            out[52..56].copy_from_slice(&c.to_le_bytes());
        }
        if let Some(c) = self.underline_color_bgr {
            out[56..60].copy_from_slice(&c.to_le_bytes());
        }
        if let Some(c) = self.shade_color_bgr {
            out[60..64].copy_from_slice(&c.to_le_bytes());
        }
        if let Some(c) = self.strikethrough_color_bgr {
            if out.len() >= 74 {
                out[70..74].copy_from_slice(&c.to_le_bytes());
            }
        }

        Ok(out)
    }
}

/// Paragraph horizontal alignment (`attr1` bits 2..4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum ParaAlignment {
    Justify = 0,
    Left = 1,
    Right = 2,
    Center = 3,
    Distribute = 4,
    Split = 5,
}

/// Line spacing mode (`attr1` bits 0..1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum LineSpacingType {
    Percent = 0,
    Fixed = 1,
    SpaceOnly = 2,
    Minimum = 3,
}

/// Overlay patch applied onto an existing `HWPTAG_PARA_SHAPE` (`0x19`) body (>= 46 bytes -> 58 bytes).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ParaShapePatch {
    pub alignment: Option<ParaAlignment>,
    pub line_spacing_type: Option<LineSpacingType>,
    pub heading_kind: Option<u8>,
    pub heading_id: Option<u16>,
    pub margin_left: Option<i32>,
    pub margin_right: Option<i32>,
    pub indent: Option<i32>,
    pub spacing_before: Option<i32>,
    pub spacing_after: Option<i32>,
    pub line_spacing: Option<i32>,
    pub border_fill_id: Option<u16>,
}

impl ParaShapePatch {
    /// Clone `base` (`>= 46` bytes) into a normalized 58-byte `PARA_SHAPE` payload.
    pub fn apply_to(&self, base: &[u8]) -> Result<Vec<u8>> {
        if base.len() < 46 {
            return Err(Hwp5Error::DocInfoCorrupt(format!(
                "ParaShape base must be >= 46 bytes (got {})",
                base.len()
            )));
        }
        let mut out = vec![0u8; 58];
        let copy_len = base.len().min(58);
        out[..copy_len].copy_from_slice(&base[..copy_len]);

        let mut attr1 = u32::from_le_bytes(out[0..4].try_into().unwrap());
        if let Some(a) = self.alignment {
            attr1 = (attr1 & !(0x07 << 2)) | ((a as u32) << 2);
        }
        if let Some(ls) = self.line_spacing_type {
            attr1 = (attr1 & !0x03) | (ls as u32);
        }
        if let Some(hk) = self.heading_kind {
            attr1 = (attr1 & !(0x03 << 23)) | (((hk as u32) & 0x03) << 23);
        }
        out[0..4].copy_from_slice(&attr1.to_le_bytes());

        if let Some(v) = self.margin_left {
            out[4..8].copy_from_slice(&v.to_le_bytes());
        }
        if let Some(v) = self.margin_right {
            out[8..12].copy_from_slice(&v.to_le_bytes());
        }
        if let Some(v) = self.indent {
            out[12..16].copy_from_slice(&v.to_le_bytes());
        }
        if let Some(v) = self.spacing_before {
            out[16..20].copy_from_slice(&v.to_le_bytes());
        }
        if let Some(v) = self.spacing_after {
            out[20..24].copy_from_slice(&v.to_le_bytes());
        }
        if let Some(v) = self.line_spacing {
            out[24..28].copy_from_slice(&v.to_le_bytes());
            out[50..54].copy_from_slice(&(v as u32).to_le_bytes());
        }
        if let Some(id) = self.heading_id {
            out[30..32].copy_from_slice(&id.to_le_bytes());
        }
        if let Some(id) = self.border_fill_id {
            out[32..34].copy_from_slice(&id.to_le_bytes());
        }

        Ok(out)
    }
}

/// Structured view and mutator over a decompressed HWP 5.0 `DocInfo` stream.
#[derive(Debug, Clone)]
pub struct DocInfoDoc {
    pub records: Vec<RawRecord>,
}

impl DocInfoDoc {
    /// Parse a decompressed `DocInfo` stream into structured records and verify `HWPTAG_ID_MAPPINGS`.
    pub fn parse(plain: &[u8]) -> Result<Self> {
        let records = parse_records(plain)?;
        let has_id_map = records
            .iter()
            .any(|r| r.tag == tag::ID_MAPPINGS && r.body.len() >= 60);
        if !has_id_map {
            return Err(Hwp5Error::DocInfoCorrupt(
                "HWPTAG_ID_MAPPINGS missing or shorter than 60 bytes".to_string(),
            ));
        }
        Ok(Self { records })
    }

    /// Serialize records back to raw decompressed `DocInfo` bytes.
    pub fn serialize(&self) -> Result<Vec<u8>> {
        serialize_records(&self.records)
    }

    fn id_mappings_index(&self) -> Result<usize> {
        self.records
            .iter()
            .position(|r| r.tag == tag::ID_MAPPINGS)
            .ok_or_else(|| Hwp5Error::DocInfoCorrupt("missing HWPTAG_ID_MAPPINGS".to_string()))
    }

    /// Read count from `HWPTAG_ID_MAPPINGS` at `slot_index` (`id_slot::*`).
    pub fn id_mapping_count(&self, slot_index: usize) -> Result<u32> {
        let idx = self.id_mappings_index()?;
        let body = &self.records[idx].body;
        let off = slot_index * 4;
        if off + 4 > body.len() {
            return Err(Hwp5Error::DocInfoCorrupt(format!(
                "ID_MAPPINGS ({} bytes) too short for slot {slot_index}",
                body.len()
            )));
        }
        Ok(u32::from_le_bytes(body[off..off + 4].try_into().unwrap()))
    }

    fn set_id_mapping_count(&mut self, slot_index: usize, count: u32) -> Result<()> {
        let idx = self.id_mappings_index()?;
        let body = &mut self.records[idx].body;
        let off = slot_index * 4;
        if off + 4 > body.len() {
            return Err(Hwp5Error::DocInfoCorrupt(format!(
                "ID_MAPPINGS ({} bytes) too short for slot {slot_index}",
                body.len()
            )));
        }
        body[off..off + 4].copy_from_slice(&count.to_le_bytes());
        Ok(())
    }

    /// Re-synchronize every standard `ID_MAPPINGS` table count (`BIN_DATA`, `BORDER_FILL`,
    /// `CHAR_SHAPE`, `TAB_DEF`, `NUMBERING`, `BULLET`, `PARA_SHAPE`, `STYLE`) to match actual records.
    /// `FACE_NAME` per-script counts are left untouched unless updated through `find_or_add_face_name`.
    pub fn sync_id_mappings(&mut self) -> Result<()> {
        let pairs = [
            (id_slot::BIN_DATA, tag::BIN_DATA),
            (id_slot::BORDER_FILL, tag::BORDER_FILL),
            (id_slot::CHAR_SHAPE, tag::CHAR_SHAPE),
            (id_slot::TAB_DEF, tag::TAB_DEF),
            (id_slot::NUMBERING, tag::NUMBERING),
            (id_slot::BULLET, tag::BULLET),
            (id_slot::PARA_SHAPE, tag::PARA_SHAPE),
            (id_slot::STYLE, tag::STYLE),
        ];
        let counts: [(usize, u32); 8] = pairs.map(|(slot, rec_tag)| {
            let count = self.records.iter().filter(|r| r.tag == rec_tag).count() as u32;
            (slot, count)
        });
        for (slot, count) in counts {
            self.set_id_mapping_count(slot, count)?;
        }
        Ok(())
    }

    /// Return all record bodies matching `rec_tag` in 0-based order.
    pub fn bodies_by_tag(&self, rec_tag: u16) -> Vec<&[u8]> {
        self.records
            .iter()
            .filter(|r| r.tag == rec_tag)
            .map(|r| r.body.as_slice())
            .collect()
    }

    /// Return all `BORDER_FILL` (`0x14`) record bodies in document order (index `id - 1`).
    pub fn border_fills(&self) -> Vec<&[u8]> {
        self.bodies_by_tag(tag::BORDER_FILL)
    }

    /// Get `BORDER_FILL` body by 1-based `border_fill_id` (`0` = no fill -> `None`).
    pub fn get_border_fill(&self, border_fill_id: u16) -> Option<&[u8]> {
        if border_fill_id == 0 {
            return None;
        }
        self.border_fills()
            .get((border_fill_id - 1) as usize)
            .copied()
    }

    /// Find an identical `BORDER_FILL` body or append a new record; returns **1-based** ID.
    pub fn find_or_add_border_fill(&mut self, body: Vec<u8>) -> Result<u16> {
        if let Some(pos) = self
            .border_fills()
            .iter()
            .position(|existing| *existing == body.as_slice())
        {
            return u16::try_from(pos + 1).map_err(|_| {
                Hwp5Error::DocInfoCorrupt("BorderFill ID overflows u16".to_string())
            });
        }
        self.append_table_record(tag::BORDER_FILL, body, id_slot::BORDER_FILL, true)
    }

    /// Return all `CHAR_SHAPE` (`0x15`) record bodies in 0-based ID order.
    pub fn char_shapes(&self) -> Vec<&[u8]> {
        self.bodies_by_tag(tag::CHAR_SHAPE)
    }

    /// Get `CHAR_SHAPE` body by 0-based ID.
    pub fn get_char_shape(&self, char_shape_id: u16) -> Option<&[u8]> {
        self.char_shapes().get(char_shape_id as usize).copied()
    }

    /// Find an identical `CHAR_SHAPE` body or append a new record (validating 70/74 bytes); returns **0-based** ID.
    pub fn find_or_add_char_shape(&mut self, body: Vec<u8>) -> Result<u16> {
        if body.len() != 70 && body.len() != 74 {
            return Err(Hwp5Error::DocInfoCorrupt(format!(
                "CharShape body must be 70 or 74 bytes (got {})",
                body.len()
            )));
        }
        if let Some(pos) = self
            .char_shapes()
            .iter()
            .position(|existing| *existing == body.as_slice())
        {
            return u16::try_from(pos).map_err(|_| {
                Hwp5Error::DocInfoCorrupt("CharShape ID overflows u16".to_string())
            });
        }
        self.append_table_record(tag::CHAR_SHAPE, body, id_slot::CHAR_SHAPE, false)
    }

    /// Return all `PARA_SHAPE` (`0x19`) record bodies in 0-based ID order.
    pub fn para_shapes(&self) -> Vec<&[u8]> {
        self.bodies_by_tag(tag::PARA_SHAPE)
    }

    /// Get `PARA_SHAPE` body by 0-based ID.
    pub fn get_para_shape(&self, para_shape_id: u16) -> Option<&[u8]> {
        self.para_shapes().get(para_shape_id as usize).copied()
    }

    /// Find an identical `PARA_SHAPE` body or append a new record (validating `>= 46` bytes); returns **0-based** ID.
    pub fn find_or_add_para_shape(&mut self, body: Vec<u8>) -> Result<u16> {
        if body.len() < 46 {
            return Err(Hwp5Error::DocInfoCorrupt(format!(
                "ParaShape body must be >= 46 bytes (got {})",
                body.len()
            )));
        }
        if let Some(pos) = self
            .para_shapes()
            .iter()
            .position(|existing| *existing == body.as_slice())
        {
            return u16::try_from(pos).map_err(|_| {
                Hwp5Error::DocInfoCorrupt("ParaShape ID overflows u16".to_string())
            });
        }
        self.append_table_record(tag::PARA_SHAPE, body, id_slot::PARA_SHAPE, false)
    }

    /// Append a canonical `NUMBERING` (`0x17`) list template; returns **1-based** ID.
    pub fn append_default_numbering(&mut self) -> Result<u16> {
        self.append_table_record(
            tag::NUMBERING,
            DEFAULT_NUMBERING_BYTES.to_vec(),
            id_slot::NUMBERING,
            true,
        )
    }

    /// Append a canonical `BULLET` (`0x18`) list template; returns **1-based** ID.
    pub fn append_default_bullet(&mut self) -> Result<u16> {
        self.append_table_record(
            tag::BULLET,
            DEFAULT_BULLET_BYTES.to_vec(),
            id_slot::BULLET,
            true,
        )
    }

    /// Append an `EMBEDDING` `BIN_DATA` (`0x12`) record for `/BinData/BIN{storage_id:04X}.{ext}`; returns **1-based** binary data ID.
    pub fn append_embedded_bin_data(&mut self, storage_id: u16, ext: &str) -> Result<u16> {
        let utf16: Vec<u16> = ext.encode_utf16().collect();
        let ext_len = u16::try_from(utf16.len()).map_err(|_| {
            Hwp5Error::DocInfoCorrupt("BinData extension length overflow".to_string())
        })?;
        let mut body = Vec::with_capacity(6 + utf16.len() * 2);
        body.extend_from_slice(&0x0001u16.to_le_bytes()); // properties: type = EMBEDDING (1)
        body.extend_from_slice(&storage_id.to_le_bytes());
        body.extend_from_slice(&ext_len.to_le_bytes());
        for ch in utf16 {
            body.extend_from_slice(&ch.to_le_bytes());
        }
        self.append_table_record(tag::BIN_DATA, body, id_slot::BIN_DATA, true)
    }

    /// Ensure `font_name` exists in all 7 `FACE_NAME` language slots, inserting missing entries
    /// and updating `ID_MAPPINGS[1..=7]`. Returns `[u16; 7]` face IDs for `CHAR_SHAPE`.
    pub fn find_or_add_face_name(&mut self, font_name: &str) -> Result<[u16; 7]> {
        let mut counts = [0u32; 7];
        for (slot, c) in counts.iter_mut().enumerate() {
            *c = self.id_mapping_count(id_slot::FACE_NAME_START + slot)?;
        }

        let face_indices: Vec<usize> = self
            .records
            .iter()
            .enumerate()
            .filter_map(|(idx, r)| (r.tag == tag::FACE_NAME).then_some(idx))
            .collect();

        let total_declared: usize = counts.iter().map(|&c| c as usize).sum();
        if face_indices.len() < total_declared {
            return Err(Hwp5Error::DocInfoCorrupt(format!(
                "DocInfo has {} FACE_NAME records but ID_MAPPINGS declares {total_declared}",
                face_indices.len()
            )));
        }

        let level = face_indices
            .first()
            .map(|&idx| self.records[idx].level)
            .unwrap_or(1);

        let fallback_insert_after = self
            .records
            .iter()
            .rposition(|r| r.tag == tag::BIN_DATA || r.tag == tag::ID_MAPPINGS)
            .map(|idx| idx + 1)
            .unwrap_or(self.records.len());

        let mut cursor = 0usize;
        let mut face_ids = [0u16; 7];
        let mut pending_inserts: Vec<(usize, usize)> = Vec::new(); // (insert_rec_idx, slot)
        let mut prev_slot_end = face_indices
            .first()
            .copied()
            .unwrap_or(fallback_insert_after);

        for slot in 0..7 {
            let cnt = counts[slot] as usize;
            let slot_indices = &face_indices[cursor..cursor + cnt];
            let insert_at = slot_indices
                .last()
                .map(|&idx| idx + 1)
                .unwrap_or(prev_slot_end);

            let mut found = None;
            for (local_id, &rec_idx) in slot_indices.iter().enumerate() {
                if decode_face_name(&self.records[rec_idx].body).as_deref() == Some(font_name) {
                    found = Some(local_id);
                    break;
                }
            }
            if let Some(id) = found {
                face_ids[slot] = u16::try_from(id).map_err(|_| {
                    Hwp5Error::DocInfoCorrupt("FaceName ID overflows u16".to_string())
                })?;
            } else {
                face_ids[slot] = u16::try_from(cnt).map_err(|_| {
                    Hwp5Error::DocInfoCorrupt("FaceName ID overflows u16".to_string())
                })?;
                pending_inserts.push((insert_at, slot));
            }
            if let Some(&last_idx) = slot_indices.last() {
                prev_slot_end = last_idx + 1;
            }
            cursor += cnt;
        }

        if pending_inserts.is_empty() {
            return Ok(face_ids);
        }

        let body = build_face_name_body(font_name)?;
        // Insert in descending index order so earlier record indices remain valid.
        for &(insert_at, slot) in pending_inserts.iter().rev() {
            self.records
                .insert(insert_at, RawRecord::new(tag::FACE_NAME, level, body.clone()));
            let cur = self.id_mapping_count(id_slot::FACE_NAME_START + slot)?;
            self.set_id_mapping_count(id_slot::FACE_NAME_START + slot, cur + 1)?;
        }

        Ok(face_ids)
    }

    fn append_table_record(
        &mut self,
        rec_tag: u16,
        body: Vec<u8>,
        slot_index: usize,
        one_based: bool,
    ) -> Result<u16> {
        let mut count = 0usize;
        let mut last_same_tag = None;
        let mut last_lower_tag = None;
        let mut child_level = None;

        for (idx, r) in self.records.iter().enumerate() {
            if r.tag == rec_tag {
                count += 1;
                last_same_tag = Some(idx);
                if child_level.is_none() {
                    child_level = Some(r.level);
                }
            } else if r.tag >= tag::ID_MAPPINGS && r.tag < rec_tag && r.tag < 0x30 {
                last_lower_tag = Some(idx);
            }
        }

        // Preserve sibling DocInfo table level (1 in standard HWP 5.0 files, or matching existing).
        let level = child_level.unwrap_or_else(|| {
            self.records
                .iter()
                .find(|r| {
                    matches!(
                        r.tag,
                        tag::FACE_NAME
                            | tag::BORDER_FILL
                            | tag::CHAR_SHAPE
                            | tag::TAB_DEF
                            | tag::PARA_SHAPE
                            | tag::STYLE
                    )
                })
                .map(|r| r.level)
                .unwrap_or(1)
        });

        let insert_idx = last_same_tag
            .or(last_lower_tag)
            .map(|i| i + 1)
            .unwrap_or(self.records.len());

        self.records
            .insert(insert_idx, RawRecord::new(rec_tag, level, body));

        let new_count = (count + 1) as u32;
        self.set_id_mapping_count(slot_index, new_count)?;

        let id = if one_based { count + 1 } else { count };
        u16::try_from(id).map_err(|_| {
            Hwp5Error::DocInfoCorrupt(format!("table ID overflow for tag 0x{rec_tag:02X}"))
        })
    }
}

fn decode_face_name(body: &[u8]) -> Option<String> {
    if body.len() < 3 {
        return None;
    }
    let wchar_len = u16::from_le_bytes(body[1..3].try_into().ok()?) as usize;
    let byte_end = 3usize.checked_add(wchar_len.checked_mul(2)?)?;
    if byte_end > body.len() {
        return None;
    }
    let (chunks, _) = body[3..byte_end].as_chunks::<2>();
    let units: Vec<u16> = chunks.iter().map(|c| u16::from_le_bytes(*c)).collect();
    Some(String::from_utf16_lossy(&units))
}

fn build_face_name_body(font_name: &str) -> Result<Vec<u8>> {
    let units: Vec<u16> = font_name.encode_utf16().collect();
    let len = u16::try_from(units.len()).map_err(|_| {
        Hwp5Error::DocInfoCorrupt(format!("font name too long: '{font_name}'"))
    })?;
    let mut body = Vec::with_capacity(3 + units.len() * 2);
    body.push(0); // flags: no alt/type/default font blocks
    body.extend_from_slice(&len.to_le_bytes());
    for u in units {
        body.extend_from_slice(&u.to_le_bytes());
    }
    Ok(body)
}
