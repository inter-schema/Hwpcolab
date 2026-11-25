use crate::error::{Hwp5Error, Result};
use crate::record::{parse_records, serialize_records, tag, RawRecord};

/// MSB flag in `PARA_HEADER.char_count_raw` marking the final paragraph in a list/section.
pub const PARA_LAST_FLAG: u32 = 0x8000_0000;

/// Helper constants for converting millimeters to `HWPUNIT` (`1 mm = 7200 / 25.4`).
pub const HWPUNIT_PER_MM: f64 = 7200.0 / 25.4;

/// Extended control codes in `PARA_TEXT` (`0x43`) occupying **8** UTF-16 code units (`16` bytes).
pub fn is_extended_ctrl_wchar(code: u16) -> bool {
    matches!(
        code,
        1 | 2 | 3 | 11 | 12 | 14 | 15 | 16 | 17 | 18 | 21 | 22 | 23
    )
}

/// View over a 22-byte or 24-byte `HWPTAG_PARA_HEADER` (`0x42`) record payload.
pub struct ParaHeaderMut<'a> {
    body: &'a mut [u8],
}

impl<'a> ParaHeaderMut<'a> {
    pub fn wrap(body: &'a mut [u8]) -> Result<Self> {
        if body.len() < 22 {
            return Err(Hwp5Error::MalformedRecord(format!(
                "PARA_HEADER body must be >= 22 bytes (got {})",
                body.len()
            )));
        }
        Ok(Self { body })
    }

    pub fn char_count(&self) -> u32 {
        u32::from_le_bytes(self.body[0..4].try_into().unwrap()) & 0x7FFF_FFFF
    }

    pub fn is_last_in_list(&self) -> bool {
        (u32::from_le_bytes(self.body[0..4].try_into().unwrap()) & PARA_LAST_FLAG) != 0
    }

    pub fn set_char_count(&mut self, count: u32, last_in_list: bool) {
        let flag = if last_in_list { PARA_LAST_FLAG } else { 0 };
        let val = flag | (count & 0x7FFF_FFFF);
        self.body[0..4].copy_from_slice(&val.to_le_bytes());
    }

    pub fn set_last_in_list(&mut self, last_in_list: bool) {
        let count = self.char_count();
        self.set_char_count(count, last_in_list);
    }

    pub fn control_mask(&self) -> u32 {
        u32::from_le_bytes(self.body[4..8].try_into().unwrap())
    }

    pub fn set_control_mask(&mut self, mask: u32) {
        self.body[4..8].copy_from_slice(&mask.to_le_bytes());
    }

    pub fn para_shape_id(&self) -> u16 {
        u16::from_le_bytes(self.body[8..10].try_into().unwrap())
    }

    pub fn set_para_shape_id(&mut self, id: u16) {
        self.body[8..10].copy_from_slice(&id.to_le_bytes());
    }

    pub fn style_id(&self) -> u8 {
        self.body[10]
    }

    pub fn set_style_id(&mut self, id: u8) {
        self.body[10] = id;
    }

    pub fn division_kind(&self) -> u8 {
        self.body[11]
    }

    pub fn set_division_kind(&mut self, val: u8) {
        self.body[11] = val;
    }

    pub fn char_shape_count(&self) -> u16 {
        u16::from_le_bytes(self.body[12..14].try_into().unwrap())
    }

    pub fn set_char_shape_count(&mut self, count: u16) {
        self.body[12..14].copy_from_slice(&count.to_le_bytes());
    }

    pub fn range_tag_count(&self) -> u16 {
        u16::from_le_bytes(self.body[14..16].try_into().unwrap())
    }

    pub fn set_range_tag_count(&mut self, count: u16) {
        self.body[14..16].copy_from_slice(&count.to_le_bytes());
    }

    pub fn line_seg_count(&self) -> u16 {
        u16::from_le_bytes(self.body[16..18].try_into().unwrap())
    }

    pub fn set_line_seg_count(&mut self, count: u16) {
        self.body[16..18].copy_from_slice(&count.to_le_bytes());
    }

    pub fn instance_id(&self) -> u32 {
        u32::from_le_bytes(self.body[18..22].try_into().unwrap())
    }

    pub fn set_instance_id(&mut self, id: u32) {
        self.body[18..22].copy_from_slice(&id.to_le_bytes());
    }
}

/// Read-only view over a `HWPTAG_PARA_HEADER` (`0x42`) record payload.
pub struct ParaHeaderRef<'a> {
    body: &'a [u8],
}

impl<'a> ParaHeaderRef<'a> {
    pub fn wrap(body: &'a [u8]) -> Result<Self> {
        if body.len() < 22 {
            return Err(Hwp5Error::MalformedRecord(format!(
                "PARA_HEADER body must be >= 22 bytes (got {})",
                body.len()
            )));
        }
        Ok(Self { body })
    }

    pub fn char_count(&self) -> u32 {
        u32::from_le_bytes(self.body[0..4].try_into().unwrap()) & 0x7FFF_FFFF
    }

    pub fn is_last_in_list(&self) -> bool {
        (u32::from_le_bytes(self.body[0..4].try_into().unwrap()) & PARA_LAST_FLAG) != 0
    }

    pub fn para_shape_id(&self) -> u16 {
        u16::from_le_bytes(self.body[8..10].try_into().unwrap())
    }

    pub fn instance_id(&self) -> u32 {
        u32::from_le_bytes(self.body[18..22].try_into().unwrap())
    }
}

/// Extracted plain text and wchar index map from a `HWPTAG_PARA_TEXT` (`0x43`) payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedParaText {
    /// Visible Unicode text (tabs normalized to `\t`, inline line breaks `0x000A` to `\n`).
    pub text: String,
    /// Total length in UTF-16 code units inside `PARA_TEXT` (matches `PARA_HEADER.char_count`).
    pub wchar_len: usize,
    /// For each scalar character in `text`, its starting UTF-16 wchar offset in `PARA_TEXT`.
    pub char_wchar_offsets: Vec<usize>,
}

/// Decode `HWPTAG_PARA_TEXT` (`0x43`) bytes, preserving UTF-16 offsets across 8-wchar HWP controls.
pub fn decode_para_text(body: &[u8]) -> Result<DecodedParaText> {
    if !body.len().is_multiple_of(2) {
        return Err(Hwp5Error::MalformedRecord(format!(
            "PARA_TEXT byte length {} is odd",
            body.len()
        )));
    }
    let (chunks, _) = body.as_chunks::<2>();
    let units: Vec<u16> = chunks.iter().map(|c| u16::from_le_bytes(*c)).collect();
    let wchar_len = units.len();

    let mut text = String::new();
    let mut char_wchar_offsets = Vec::new();
    let mut i = 0usize;

    while i < units.len() {
        let ch = units[i];
        if is_extended_ctrl_wchar(ch) {
            i = (i + 8).min(units.len());
            continue;
        }
        match ch {
            0 | 4 | 5 | 6 | 7 | 8 | 19 | 20 | 24..=31 => {
                i += 1;
            }
            9 => {
                char_wchar_offsets.push(i);
                text.push('\t');
                i += 1;
            }
            10 => {
                char_wchar_offsets.push(i);
                text.push('\n');
                i += 1;
            }
            13 => {
                // EOP carriage return ends logical paragraph text.
                i += 1;
            }
            _ => {
                if (0xD800..=0xDBFF).contains(&ch)
                    && i + 1 < units.len()
                    && (0xDC00..=0xDFFF).contains(&units[i + 1])
                {
                    if let Some(decoded) =
                        char::decode_utf16([ch, units[i + 1]]).next().and_then(|r| r.ok())
                    {
                        char_wchar_offsets.push(i);
                        text.push(decoded);
                    }
                    i += 2;
                } else {
                    if let Some(decoded) = char::from_u32(ch as u32) {
                        char_wchar_offsets.push(i);
                        text.push(decoded);
                    }
                    i += 1;
                }
            }
        }
    }

    Ok(DecodedParaText {
        text,
        wchar_len,
        char_wchar_offsets,
    })
}

/// Encode plain paragraph text into `HWPTAG_PARA_TEXT` UTF-16LE bytes (appending `0x000D` EOP).
/// Returns `(encoded_bytes, wchar_count)`.
pub fn encode_para_text(text: &str) -> (Vec<u8>, u32) {
    let mut units: Vec<u16> = Vec::with_capacity(text.len() + 1);
    for ch in text.chars() {
        match ch {
            '\n' => units.push(10),
            '\r' => {} // trailing EOP added once below
            _ => {
                let mut buf = [0u16; 2];
                for u in ch.encode_utf16(&mut buf) {
                    units.push(*u);
                }
            }
        }
    }
    units.push(13); // EOP
    let mut bytes = Vec::with_capacity(units.len() * 2);
    for u in &units {
        bytes.extend_from_slice(&u.to_le_bytes());
    }
    (bytes, units.len() as u32)
}

/// Entry inside `HWPTAG_PARA_CHAR_SHAPE` (`0x44`): `(char_pos, char_shape_id)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharShapeRun {
    pub char_pos: u32,
    pub char_shape_id: u32,
}

/// Parse `HWPTAG_PARA_CHAR_SHAPE` (`0x44`) payload into `(char_pos, char_shape_id)` runs.
pub fn parse_para_char_shapes(body: &[u8]) -> Result<Vec<CharShapeRun>> {
    if !body.len().is_multiple_of(8) {
        return Err(Hwp5Error::MalformedRecord(format!(
            "PARA_CHAR_SHAPE length {} is not a multiple of 8",
            body.len()
        )));
    }
    let (chunks, _) = body.as_chunks::<8>();
    Ok(chunks
        .iter()
        .map(|c| CharShapeRun {
            char_pos: u32::from_le_bytes(c[0..4].try_into().unwrap()),
            char_shape_id: u32::from_le_bytes(c[4..8].try_into().unwrap()),
        })
        .collect())
}

/// Serialize `HWPTAG_PARA_CHAR_SHAPE` (`0x44`) runs into raw bytes.
pub fn serialize_para_char_shapes(runs: &[CharShapeRun]) -> Vec<u8> {
    let mut out = Vec::with_capacity(runs.len() * 8);
    for run in runs {
        out.extend_from_slice(&run.char_pos.to_le_bytes());
        out.extend_from_slice(&run.char_shape_id.to_le_bytes());
    }
    out
}

/// Parsed cell attributes from a level-2 table cell `HWPTAG_LIST_HEADER` (`0x48`) body (`>= 34` bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TableCellAttr {
    pub col: u16,
    pub row: u16,
    pub col_span: u16,
    pub row_span: u16,
    pub width: u32,
    pub height: u32,
    pub margin_left: u16,
    pub margin_right: u16,
    pub margin_top: u16,
    pub margin_bottom: u16,
    pub border_fill_id: u16,
}

impl TableCellAttr {
    pub const MIN_LEN: usize = 34;

    pub fn parse(body: &[u8]) -> Result<Self> {
        if body.len() < Self::MIN_LEN {
            return Err(Hwp5Error::MalformedRecord(format!(
                "cell LIST_HEADER too short ({} bytes, need >= {})",
                body.len(),
                Self::MIN_LEN
            )));
        }
        Ok(Self {
            col: u16::from_le_bytes(body[8..10].try_into().unwrap()),
            row: u16::from_le_bytes(body[10..12].try_into().unwrap()),
            col_span: u16::from_le_bytes(body[12..14].try_into().unwrap()),
            row_span: u16::from_le_bytes(body[14..16].try_into().unwrap()),
            width: u32::from_le_bytes(body[16..20].try_into().unwrap()),
            height: u32::from_le_bytes(body[20..24].try_into().unwrap()),
            margin_left: u16::from_le_bytes(body[24..26].try_into().unwrap()),
            margin_right: u16::from_le_bytes(body[26..28].try_into().unwrap()),
            margin_top: u16::from_le_bytes(body[28..30].try_into().unwrap()),
            margin_bottom: u16::from_le_bytes(body[30..32].try_into().unwrap()),
            border_fill_id: u16::from_le_bytes(body[32..34].try_into().unwrap()),
        })
    }
}

/// Summary metadata for a table discovered in a section stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionTableInfo {
    /// 0-based document-order table index in this section.
    pub table_index: usize,
    /// 0-based level-0 paragraph index containing the `'tbl '` control.
    pub para_index: usize,
    /// 0-based level-1 control index within that paragraph.
    pub ctrl_index: usize,
    /// Record index of the `CTRL_HEADER` (`0x47`).
    pub ctrl_rec_idx: usize,
    /// Record index of the `TABLE` (`0x4D`).
    pub table_rec_idx: usize,
    /// Declared row count from `HWPTAG_TABLE`.
    pub rows: u16,
    /// Declared column count from `HWPTAG_TABLE`.
    pub cols: u16,
    /// Ordered cell records (`LIST_HEADER` at level 2) within the table.
    pub cells: Vec<SectionTableCellInfo>,
}

/// Summary metadata for one cell inside a `SectionTableInfo`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionTableCellInfo {
    pub cell_index: usize,
    pub list_header_rec_idx: usize,
    pub end_rec_idx: usize,
    pub attr: TableCellAttr,
}

/// Parsed `HWPTAG_PAGE_DEF` (`0x49`, 40 bytes) geometry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageDef {
    pub width: u32,
    pub height: u32,
    pub margin_left: u32,
    pub margin_right: u32,
    pub margin_top: u32,
    pub margin_bottom: u32,
    pub header_margin: u32,
    pub footer_margin: u32,
    pub gutter_margin: u32,
    pub attr: u32,
}

impl PageDef {
    pub const LEN: usize = 40;

    pub fn parse(body: &[u8]) -> Result<Self> {
        if body.len() < Self::LEN {
            return Err(Hwp5Error::MalformedRecord(format!(
                "PAGE_DEF body must be >= 40 bytes (got {})",
                body.len()
            )));
        }
        Ok(Self {
            width: u32::from_le_bytes(body[0..4].try_into().unwrap()),
            height: u32::from_le_bytes(body[4..8].try_into().unwrap()),
            margin_left: u32::from_le_bytes(body[8..12].try_into().unwrap()),
            margin_right: u32::from_le_bytes(body[12..16].try_into().unwrap()),
            margin_top: u32::from_le_bytes(body[16..20].try_into().unwrap()),
            margin_bottom: u32::from_le_bytes(body[20..24].try_into().unwrap()),
            header_margin: u32::from_le_bytes(body[24..28].try_into().unwrap()),
            footer_margin: u32::from_le_bytes(body[28..32].try_into().unwrap()),
            gutter_margin: u32::from_le_bytes(body[32..36].try_into().unwrap()),
            attr: u32::from_le_bytes(body[36..40].try_into().unwrap()),
        })
    }

    pub fn write_to(&self, body: &mut [u8]) -> Result<()> {
        if body.len() < Self::LEN {
            return Err(Hwp5Error::MalformedRecord(format!(
                "PAGE_DEF body must be >= 40 bytes (got {})",
                body.len()
            )));
        }
        body[0..4].copy_from_slice(&self.width.to_le_bytes());
        body[4..8].copy_from_slice(&self.height.to_le_bytes());
        body[8..12].copy_from_slice(&self.margin_left.to_le_bytes());
        body[12..16].copy_from_slice(&self.margin_right.to_le_bytes());
        body[16..20].copy_from_slice(&self.margin_top.to_le_bytes());
        body[20..24].copy_from_slice(&self.margin_bottom.to_le_bytes());
        body[24..28].copy_from_slice(&self.header_margin.to_le_bytes());
        body[28..32].copy_from_slice(&self.footer_margin.to_le_bytes());
        body[32..36].copy_from_slice(&self.gutter_margin.to_le_bytes());
        body[36..40].copy_from_slice(&self.attr.to_le_bytes());
        Ok(())
    }

    pub fn is_landscape(&self) -> bool {
        (self.attr & 0x01) != 0
    }

    /// Usable printable page width in `HWPUNIT` (width - left - right - gutter).
    pub fn usable_width(&self) -> u32 {
        self.width
            .saturating_sub(self.margin_left)
            .saturating_sub(self.margin_right)
            .saturating_sub(self.gutter_margin)
    }
}

/// Decoded `BodyText/SectionN` record tree with table and paragraph traversal helpers.
#[derive(Debug, Clone)]
pub struct SectionDoc {
    pub records: Vec<RawRecord>,
}

impl SectionDoc {
    pub fn parse(plain: &[u8]) -> Result<Self> {
        Ok(Self {
            records: parse_records(plain)?,
        })
    }

    pub fn serialize(&self) -> Result<Vec<u8>> {
        serialize_records(&self.records)
    }

    /// Return record indices of all level-0 `PARA_HEADER` (`0x42`) records in document order.
    pub fn top_level_para_indices(&self) -> Vec<usize> {
        self.records
            .iter()
            .enumerate()
            .filter_map(|(idx, r)| (r.tag == tag::PARA_HEADER && r.level == 0).then_some(idx))
            .collect()
    }

    /// Return the first `PAGE_DEF` (`0x49`) record found in this section, if any.
    pub fn page_def(&self) -> Result<Option<PageDef>> {
        match self.records.iter().find(|r| r.tag == tag::PAGE_DEF) {
            Some(rec) => PageDef::parse(&rec.body).map(Some),
            None => Ok(None),
        }
    }

    /// Pick a fresh paragraph `instance_id` strictly greater than every `PARA_HEADER.instance_id` in this section.
    pub fn pick_fresh_instance_id(&self) -> u32 {
        let mut max_id = 0u32;
        for rec in &self.records {
            if rec.tag == tag::PARA_HEADER {
                if let Ok(hdr) = ParaHeaderRef::wrap(&rec.body) {
                    max_id = max_id.max(hdr.instance_id());
                }
            }
        }
        max_id.wrapping_add(1)
    }

    /// Normalize the `PARA_LAST_FLAG` bit across all level-0 paragraphs AND across every
    /// `LIST_HEADER` container (table cells, footnotes, headers/footers) so only the true
    /// final paragraph of each list carries `0x8000_0000`.
    pub fn normalize_last_para_flags(&mut self) -> Result<()> {
        let top_paras = self.top_level_para_indices();
        if let Some((&last_idx, earlier)) = top_paras.split_last() {
            for &idx in earlier {
                if let Ok(mut hdr) = ParaHeaderMut::wrap(&mut self.records[idx].body) {
                    hdr.set_last_in_list(false);
                }
            }
            if let Ok(mut hdr) = ParaHeaderMut::wrap(&mut self.records[last_idx].body) {
                hdr.set_last_in_list(true);
            }
        }

        // Also normalize child paragraphs under each LIST_HEADER at `list_level`
        // (direct child PARA_HEADERs sit at level == list_level).
        let len = self.records.len();
        let mut i = 0usize;
        while i < len {
            if self.records[i].tag == tag::LIST_HEADER {
                let list_level = self.records[i].level;
                let mut child_paras = Vec::new();
                let mut j = i + 1;
                while j < len {
                    let r = &self.records[j];
                    if r.level < list_level || (r.tag == tag::LIST_HEADER && r.level == list_level)
                    {
                        break;
                    }
                    if r.tag == tag::PARA_HEADER && r.level == list_level {
                        child_paras.push(j);
                    }
                    j += 1;
                }
                if let Some((&last_idx, earlier)) = child_paras.split_last() {
                    for &idx in earlier {
                        if let Ok(mut hdr) = ParaHeaderMut::wrap(&mut self.records[idx].body) {
                            hdr.set_last_in_list(false);
                        }
                    }
                    if let Ok(mut hdr) = ParaHeaderMut::wrap(&mut self.records[last_idx].body) {
                        hdr.set_last_in_list(true);
                    }
                }
            }
            i += 1;
        }
        Ok(())
    }

    /// Enumerate every top-level table in this section (`CTRL_HEADER` at level 1 owning a `TABLE` at level 2)
    /// along with its grid metadata and level-2 `LIST_HEADER` cells.
    pub fn enumerate_tables(&self) -> Result<Vec<SectionTableInfo>> {
        let mut tables = Vec::new();
        let top_paras = self.top_level_para_indices();

        for (para_index, &para_rec_idx) in top_paras.iter().enumerate() {
            let para_end = top_paras
                .get(para_index + 1)
                .copied()
                .unwrap_or(self.records.len());

            let mut ctrl_index = 0usize;
            let mut j = para_rec_idx + 1;
            while j < para_end {
                if self.records[j].tag == tag::CTRL_HEADER && self.records[j].level == 1 {
                    let ctrl_end = ((j + 1)..para_end)
                        .find(|&k| {
                            self.records[k].tag == tag::CTRL_HEADER && self.records[k].level == 1
                        })
                        .unwrap_or(para_end);

                    if let Some(table_rec_idx) = ((j + 1)..ctrl_end)
                        .find(|&k| self.records[k].tag == tag::TABLE && self.records[k].level == 2)
                    {
                        let t_body = &self.records[table_rec_idx].body;
                        if t_body.len() < 8 {
                            return Err(Hwp5Error::MalformedRecord(format!(
                                "TABLE record too short ({} bytes)",
                                t_body.len()
                            )));
                        }
                        let rows = u16::from_le_bytes(t_body[4..6].try_into().unwrap());
                        let cols = u16::from_le_bytes(t_body[6..8].try_into().unwrap());

                        let lh_indices: Vec<usize> = ((j + 1)..ctrl_end)
                            .filter(|&k| {
                                self.records[k].tag == tag::LIST_HEADER
                                    && self.records[k].level == 2
                            })
                            .collect();

                        let mut cells = Vec::with_capacity(lh_indices.len());
                        for (cell_index, &lh_idx) in lh_indices.iter().enumerate() {
                            let end_rec_idx = lh_indices
                                .get(cell_index + 1)
                                .copied()
                                .unwrap_or(ctrl_end);
                            let attr = TableCellAttr::parse(&self.records[lh_idx].body)?;
                            cells.push(SectionTableCellInfo {
                                cell_index,
                                list_header_rec_idx: lh_idx,
                                end_rec_idx,
                                attr,
                            });
                        }

                        tables.push(SectionTableInfo {
                            table_index: tables.len(),
                            para_index,
                            ctrl_index,
                            ctrl_rec_idx: j,
                            table_rec_idx,
                            rows,
                            cols,
                            cells,
                        });
                    }
                    ctrl_index += 1;
                }
                j += 1;
            }
        }

        Ok(tables)
    }

    /// Locate a table cell by `(table_index, row, col)` — pure Rust replacement for `rhwp` coordinate resolution.
    pub fn locate_cell_by_grid(
        &self,
        table_index: usize,
        row: u16,
        col: u16,
    ) -> Result<SectionTableCellInfo> {
        let tables = self.enumerate_tables()?;
        let table = tables.get(table_index).ok_or_else(|| {
            Hwp5Error::TargetNotFound(format!(
                "table index {table_index} out of bounds (section has {} table(s))",
                tables.len()
            ))
        })?;
        table
            .cells
            .iter()
            .find(|c| {
                row >= c.attr.row
                    && row < c.attr.row.saturating_add(c.attr.row_span.max(1))
                    && col >= c.attr.col
                    && col < c.attr.col.saturating_add(c.attr.col_span.max(1))
            })
            .cloned()
            .ok_or_else(|| {
                Hwp5Error::TargetNotFound(format!(
                    "cell ({row}, {col}) not found in table {table_index} ({}x{})",
                    table.rows, table.cols
                ))
            })
    }
}
