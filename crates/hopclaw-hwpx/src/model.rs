use std::collections::HashMap;

use hopclaw_types::{
    ErrorModel, ParentCellCoord, TableCellMeta, TableCoordinateMeta, TableWalker,
    HWPUNIT_PER_MM,
};

use crate::archive::HwpxArchive;
use crate::error::HwpxError;
use crate::xml_util::{
    get_attr, scan_top_level, set_or_add_attr, splice_span, sync_item_count, sync_sec_count,
    xml_escape_text, XmlElementSpan,
};

/// Embedded Hancom-native stub document used for web-safe list/paraPr injection.
pub const HANCOM_NATIVE_STUB: &[u8] =
    include_bytes!("../../../resources/claw-hwp/scripts/templates/hancom_native_stub.hwpx");

pub const PLAIN_SIDES: &str = r##"<hh:leftBorder type="SOLID" width="0.12 mm" color="#000000"/><hh:rightBorder type="SOLID" width="0.12 mm" color="#000000"/><hh:topBorder type="SOLID" width="0.12 mm" color="#000000"/><hh:bottomBorder type="SOLID" width="0.12 mm" color="#000000"/>"##;
pub const NONE_SIDES: &str = r##"<hh:leftBorder type="NONE" width="0.12 mm" color="#000000"/><hh:rightBorder type="NONE" width="0.12 mm" color="#000000"/><hh:topBorder type="NONE" width="0.12 mm" color="#000000"/><hh:bottomBorder type="NONE" width="0.12 mm" color="#000000"/>"##;
pub const DEFAULT_DIAG: &str = r##"<hh:diagonal type="SOLID" width="0.1 mm" color="#000000"/>"##;
pub const STD_SLASH_NONE: &str = r#"<hh:slash type="NONE" Crooked="0" isCounter="0"/><hh:backSlash type="NONE" Crooked="0" isCounter="0"/>"#;
pub const BF_ATTRS: &str =
    r#" threeD="0" shadow="0" centerLine="NONE" breakCellSeparateLine="0""#;

#[derive(Debug, Clone)]
pub struct ParagraphLocation {
    pub section: String,
    pub span: XmlElementSpan,
}

#[derive(Debug, Clone)]
pub struct TableLocation {
    pub section: String,
    pub section_index: usize,
    pub table_index: usize,
    pub depth: usize,
    pub parent_table_index: Option<usize>,
    pub parent_cell: Option<ParentCellCoord>,
    pub span: XmlElementSpan,
}

/// High-level OWPML document model wrapping [`HwpxArchive`].
pub struct HwpxDocument {
    pub archive: HwpxArchive,
}

impl HwpxDocument {
    pub fn open(bytes: Vec<u8>) -> Result<Self, HwpxError> {
        Ok(Self {
            archive: HwpxArchive::from_bytes(bytes)?,
        })
    }

    pub fn from_archive(archive: HwpxArchive) -> Self {
        Self { archive }
    }

    /// Top-level `<hp:p>` paragraphs across all sections, excluding paragraphs nested inside `<hp:tbl>`.
    pub fn paragraphs(&self) -> Result<Vec<ParagraphLocation>, HwpxError> {
        let mut list = Vec::new();
        for name in self.archive.section_names() {
            let xml = self.archive.read_text_ro(&name)?;
            let tbl_ranges: Vec<(usize, usize)> = scan_top_level(&xml, "hp:tbl")?
                .into_iter()
                .map(|t| (t.start, t.end))
                .collect();
            for span in scan_top_level(&xml, "hp:p")? {
                let inside_table = tbl_ranges
                    .iter()
                    .any(|(a, b)| *a < span.start && span.start < *b);
                if !inside_table {
                    list.push(ParagraphLocation {
                        section: name.clone(),
                        span,
                    });
                }
            }
        }
        Ok(list)
    }

    /// Top-level `<hp:tbl>` across all sections in document order.
    pub fn top_tables(&self) -> Result<Vec<TableLocation>, HwpxError> {
        let mut list = Vec::new();
        for (sec_idx, name) in self.archive.section_names().into_iter().enumerate() {
            let xml = self.archive.read_text_ro(&name)?;
            for span in scan_top_level(&xml, "hp:tbl")? {
                let idx = list.len();
                list.push(TableLocation {
                    section: name.clone(),
                    section_index: sec_idx,
                    table_index: idx,
                    depth: 0,
                    parent_table_index: None,
                    parent_cell: None,
                    span,
                });
            }
        }
        Ok(list)
    }

    /// Canonical pre-order traversal of EVERY `<hp:tbl>` across all sections (nested included).
    /// Byte offsets in `span` are absolute offsets inside the section XML.
    pub fn tables_deep(&self) -> Result<Vec<TableLocation>, HwpxError> {
        let mut list = Vec::new();
        for (sec_idx, name) in self.archive.section_names().into_iter().enumerate() {
            let xml = self.archive.read_text_ro(&name)?;
            walk_tables_recursive(&name, sec_idx, &xml, 0, 0, None, None, &mut list)?;
        }
        Ok(list)
    }

    pub fn get_table(&self, table_index: usize) -> Result<TableLocation, HwpxError> {
        let tables = self.tables_deep()?;
        let count = tables.len();
        tables
            .into_iter()
            .nth(table_index)
            .ok_or(HwpxError::TableNotFound {
                index: table_index,
                count,
            })
    }

    pub fn require_header_name(&self) -> Result<String, HwpxError> {
        self.archive
            .header_name()
            .ok_or_else(|| HwpxError::MissingPart("Contents/header.xml".to_string()))
    }

    /// Synchronize `secCnt` and `itemCnt` attributes in `Contents/header.xml` if present.
    pub fn sync_header_counts(&mut self) -> Result<(), HwpxError> {
        let Some(header_name) = self.archive.header_name() else {
            return Ok(());
        };
        let sec_count = self.archive.section_names().len();
        let mut header = self.archive.read_text(&header_name)?;
        header = sync_sec_count(&header, sec_count)?;
        for (list_tag, item_tag) in [
            ("hh:borderFills", "hh:borderFill"),
            ("hh:charProperties", "hh:charPr"),
            ("hh:paraProperties", "hh:paraPr"),
            ("hh:bullets", "hh:bullet"),
            ("hh:numberings", "hh:numbering"),
        ] {
            header = sync_item_count(&header, list_tag, item_tag)?;
        }
        self.archive.write_text(header_name, header);
        Ok(())
    }

    pub fn ensure_border_fill(&mut self, want_inner: &str) -> Result<String, HwpxError> {
        let header_name = self.require_header_name()?;
        let header = self.archive.read_text(&header_name)?;
        let lists = scan_top_level(&header, "hh:borderFills")?;
        let list = lists
            .first()
            .ok_or_else(|| HwpxError::MissingPart("<hh:borderFills> in header.xml".to_string()))?;
        let existing = scan_top_level(list.inner(&header), "hh:borderFill")?;
        let mut max_id = 0usize;
        for bf in &existing {
            if bf.inner(list.inner(&header)) == want_inner {
                if let Some(id) = get_attr(bf.attrs(list.inner(&header), "hh:borderFill"), "id") {
                    return Ok(id);
                }
            }
            if let Some(id) = get_attr(bf.attrs(list.inner(&header), "hh:borderFill"), "id") {
                if let Ok(n) = id.parse::<usize>() {
                    max_id = max_id.max(n);
                }
            }
        }
        let new_id = (max_id + 1).to_string();
        let new_bf = format!(r#"<hh:borderFill id="{new_id}"{BF_ATTRS}>{want_inner}</hh:borderFill>"#);
        let list_attrs = list.attrs(&header, "hh:borderFills");
        let new_list = format!(
            "<hh:borderFills{list_attrs}>{}{new_bf}</hh:borderFills>",
            list.inner(&header)
        );
        let new_header = splice_span(&header, list, &new_list);
        let new_header = sync_item_count(&new_header, "hh:borderFills", "hh:borderFill")?;
        self.archive.write_text(header_name, new_header);
        Ok(new_id)
    }

    pub fn build_attr_ref_counts(&self, attr_name: &str) -> Result<HashMap<String, usize>, HwpxError> {
        let mut counts = HashMap::new();
        let needle = format!(r#"{attr_name}=""#);
        for name in self.archive.section_names() {
            let xml = self.archive.read_text_ro(&name)?;
            let mut rest = xml.as_str();
            while let Some(idx) = rest.find(&needle) {
                let after = &rest[idx + needle.len()..];
                if let Some(end_q) = after.find('"') {
                    let val = &after[..end_q];
                    if !val.is_empty() && val.bytes().all(|b| b.is_ascii_digit()) {
                        *counts.entry(val.to_string()).or_insert(0) += 1;
                    }
                    rest = &after[end_q + 1..];
                } else {
                    break;
                }
            }
        }
        Ok(counts)
    }

    pub fn find_font_id_by_face(&self, face: &str) -> Result<Option<String>, HwpxError> {
        let Some(header_name) = self.archive.header_name() else {
            return Ok(None);
        };
        let header = self.archive.read_text_ro(&header_name)?;
        for ff in scan_top_level(&header, "hh:fontface")? {
            if get_attr(ff.attrs(&header, "hh:fontface"), "lang").as_deref() == Some("HANGUL") {
                let ff_inner = ff.inner(&header);
                for f in scan_top_level(ff_inner, "hh:font")? {
                    let attrs = f.attrs(ff_inner, "hh:font");
                    if get_attr(attrs, "face").as_deref() == Some(face) {
                        return Ok(get_attr(attrs, "id"));
                    }
                }
            }
        }
        Ok(None)
    }

    pub fn char_pr_font_pt(&self, char_pr_id: Option<&str>) -> f64 {
        let Some(id) = char_pr_id else {
            return 10.0;
        };
        let Ok(Some(header_name)) = Ok::<_, HwpxError>(self.archive.header_name()) else {
            return 10.0;
        };
        let Ok(header) = self.archive.read_text_ro(&header_name) else {
            return 10.0;
        };
        let Ok(cps) = scan_top_level(&header, "hh:charPr") else {
            return 10.0;
        };
        for cp in cps {
            let attrs = cp.attrs(&header, "hh:charPr");
            if get_attr(attrs, "id").as_deref() == Some(id) {
                if let Some(h) = get_attr(attrs, "height").and_then(|s| s.parse::<f64>().ok()) {
                    return (h / 100.0).max(6.0);
                }
            }
        }
        10.0
    }

    pub fn page_text_width(&self, section: &str) -> i32 {
        let Ok(xml) = self.archive.read_text_ro(section) else {
            return 48188;
        };
        let w = scan_top_level(&xml, "hp:pagePr")
            .ok()
            .and_then(|v| v.into_iter().next())
            .and_then(|s| get_attr(s.attrs(&xml, "hp:pagePr"), "width"))
            .and_then(|s| s.parse::<i32>().ok())
            .unwrap_or(59528);
        let (l, r) = scan_top_level(&xml, "hp:margin")
            .ok()
            .and_then(|v| v.into_iter().next())
            .map(|s| {
                let attrs = s.attrs(&xml, "hp:margin");
                let left = get_attr(attrs, "left")
                    .and_then(|v| v.parse::<i32>().ok())
                    .unwrap_or(8504);
                let right = get_attr(attrs, "right")
                    .and_then(|v| v.parse::<i32>().ok())
                    .unwrap_or(8504);
                (left, right)
            })
            .unwrap_or((8504, 8504));
        (w - l - r).max(10_000)
    }

    /// Ensure `xmlns:hwpunitchar` and native stub `paraPr` infrastructure for web-safe
    /// paragraph styles and lists.
    pub fn inject_hancom_list_infra(&mut self) -> Result<bool, HwpxError> {
        let Some(header_name) = self.archive.header_name() else {
            return Ok(false);
        };
        let Ok(stub) = HwpxArchive::from_bytes(HANCOM_NATIVE_STUB.to_vec()) else {
            return Ok(false);
        };
        let Ok(stub_header) = stub.read_text_ro("Contents/header.xml") else {
            return Ok(false);
        };
        let stub_pps = scan_top_level(&stub_header, "hh:paraPr")?;
        let stub_bullet_pp = stub_pps
            .iter()
            .find(|p| get_attr(p.attrs(&stub_header, "hh:paraPr"), "id").as_deref() == Some("2"))
            .map(|p| p.full(&stub_header).to_string());
        let stub_number_pp = stub_pps
            .iter()
            .find(|p| get_attr(p.attrs(&stub_header, "hh:paraPr"), "id").as_deref() == Some("3"))
            .map(|p| p.full(&stub_header).to_string());
        if stub_bullet_pp.is_none() && stub_number_pp.is_none() {
            return Ok(false);
        }

        let mut header = self.archive.read_text(&header_name)?;
        let cur_pps = scan_top_level(&header, "hh:paraPr")?;
        let has_bullet = cur_pps
            .iter()
            .any(|p| p.inner(&header).contains(r#"<hh:heading type="BULLET""#));
        let has_number = cur_pps
            .iter()
            .any(|p| p.inner(&header).contains(r#"<hh:heading type="NUMBER""#));
        let mut max_id = cur_pps
            .iter()
            .filter_map(|p| {
                get_attr(p.attrs(&header, "hh:paraPr"), "id").and_then(|s| s.parse::<usize>().ok())
            })
            .max()
            .unwrap_or(0);

        let mut added = String::new();
        let mut injected = 0usize;
        if !has_bullet {
            if let Some(tmpl) = &stub_bullet_pp {
                max_id += 1;
                added.push_str(&tmpl.replacen(
                    r#"<hh:paraPr id="2""#,
                    &format!(r#"<hh:paraPr id="{max_id}""#),
                    1,
                ));
                injected += 1;
            }
        }
        if !has_number {
            if let Some(tmpl) = &stub_number_pp {
                max_id += 1;
                added.push_str(&tmpl.replacen(
                    r#"<hh:paraPr id="3""#,
                    &format!(r#"<hh:paraPr id="{max_id}""#),
                    1,
                ));
                injected += 1;
            }
        }
        if injected > 0 {
            header = header.replace("</hh:paraProperties>", &format!("{added}</hh:paraProperties>"));
            header = sync_item_count(&header, "hh:paraProperties", "hh:paraPr")?;
        }

        if !header.contains("xmlns:hwpunitchar=") {
            header = header.replacen(
                r#"xmlns:ooxmlchart="http://www.hancom.co.kr/hwpml/2016/ooxmlchart""#,
                r#"xmlns:ooxmlchart="http://www.hancom.co.kr/hwpml/2016/ooxmlchart" xmlns:hwpunitchar="http://www.hancom.co.kr/hwpml/2016/HwpUnitChar""#,
                1,
            );
        }

        if scan_top_level(&header, "hh:bullets")?.is_empty() {
            if let Some(sb) = scan_top_level(&stub_header, "hh:bullets")?.first() {
                let block = sb.full(&stub_header);
                if header.contains("</hh:numberings>") {
                    header = header.replacen("</hh:numberings>", &format!("</hh:numberings>{block}"), 1);
                } else {
                    header = header.replacen("</hh:head>", &format!("{block}</hh:head>"), 1);
                }
            }
        }

        if let Some(sn) = scan_top_level(&stub_header, "hh:numbering")?
            .into_iter()
            .find(|n| get_attr(n.attrs(&stub_header, "hh:numbering"), "id").as_deref() == Some("1"))
        {
            if let Some(cur1) = scan_top_level(&header, "hh:numbering")?
                .into_iter()
                .find(|n| get_attr(n.attrs(&header, "hh:numbering"), "id").as_deref() == Some("1"))
            {
                let cur1_inner = cur1.inner(&header);
                let has_non_empty = scan_top_level(cur1_inner, "hh:paraHead")?
                    .into_iter()
                    .any(|ph| !ph.inner(cur1_inner).is_empty());
                if !has_non_empty {
                    header = splice_span(&header, &cur1, sn.full(&stub_header));
                }
            }
        }

        self.archive.write_text(header_name, header);

        if let Some(hpf_name) = self.archive.hpf_name() {
            let mut hpf = self.archive.read_text(&hpf_name)?;
            if !hpf.contains("xmlns:hwpunitchar=") {
                hpf = hpf.replacen(
                    r#"xmlns:ooxmlchart="http://www.hancom.co.kr/hwpml/2016/ooxmlchart""#,
                    r#"xmlns:ooxmlchart="http://www.hancom.co.kr/hwpml/2016/ooxmlchart" xmlns:hwpunitchar="http://www.hancom.co.kr/hwpml/2016/HwpUnitChar""#,
                    1,
                );
            }
            if !hpf.contains("headersc") {
                let script_items = concat!(
                    r#"<opf:item id="headersc" href="Scripts/headerScripts" media-type="application/x-javascript ;charset=utf-16"/>"#,
                    r#"<opf:item id="sourcesc" href="Scripts/sourceScripts" media-type="application/x-javascript ;charset=utf-16"/>"#
                );
                let script_refs =
                    r#"<opf:itemref idref="headersc" linear="yes"/><opf:itemref idref="sourcesc" linear="yes"/>"#;
                if hpf.contains(r#"<opf:item id="settings""#) {
                    hpf = hpf.replacen(
                        r#"<opf:item id="settings""#,
                        &format!(r#"{script_items}<opf:item id="settings""#),
                        1,
                    );
                } else {
                    hpf = hpf.replacen("</opf:manifest>", &format!("{script_items}</opf:manifest>"), 1);
                }
                hpf = hpf.replacen("</opf:spine>", &format!("{script_refs}</opf:spine>"), 1);
            }
            self.archive.write_text(hpf_name, hpf);
        }

        for entry in stub.entry_names() {
            if entry.starts_with("Scripts/") && !self.archive.contains(&entry) {
                let raw = stub.read_raw_bytes(&entry)?;
                self.archive.write_bytes(entry, raw);
            }
        }

        Ok(injected > 0)
    }

    pub fn reuse_existing_list_para_pr(&self, kind: &str, lvl: usize) -> Result<Option<String>, HwpxError> {
        let Some(header_name) = self.archive.header_name() else {
            return Ok(None);
        };
        let header = self.archive.read_text_ro(&header_name)?;
        let lvl_str = lvl.to_string();
        for pp in scan_top_level(&header, "hh:paraPr")? {
            let pp_inner = pp.inner(&header);
            for h in scan_top_level(pp_inner, "hh:heading")? {
                let hattrs = h.attrs(pp_inner, "hh:heading");
                if get_attr(hattrs, "type").as_deref() == Some(kind)
                    && get_attr(hattrs, "level").as_deref() == Some(lvl_str.as_str())
                {
                    return Ok(get_attr(pp.attrs(&header, "hh:paraPr"), "id"));
                }
            }
        }
        Ok(None)
    }
}

impl TableWalker for HwpxDocument {
    fn walk_tables(&self) -> Result<Vec<TableCoordinateMeta>, ErrorModel> {
        let deep = self.tables_deep().map_err(ErrorModel::from)?;
        let mut out = Vec::with_capacity(deep.len());
        for loc in deep {
            let sec_xml = self
                .archive
                .read_text_ro(&loc.section)
                .map_err(ErrorModel::from)?;
            let tbl_inner = loc.span.inner(&sec_xml);
            let trs = scan_top_level(tbl_inner, "hp:tr").map_err(ErrorModel::from)?;
            let mut cells = Vec::new();
            let mut max_row = 0usize;
            let mut max_col = 0usize;
            let mut cell_idx = 0usize;
            for (ri, tr) in trs.iter().enumerate() {
                let tr_inner = tr.inner(tbl_inner);
                let tcs = scan_top_level(tr_inner, "hp:tc").map_err(ErrorModel::from)?;
                for (ci, tc) in tcs.iter().enumerate() {
                    let tc_inner = tc.inner(tr_inner);
                    let tail = cell_tail(tc_inner);
                    let (col, row) = read_cell_addr(tail, ci, ri);
                    let (col_span, row_span) = read_cell_span(tail);
                    let text = crate::xml_util::extract_t_text(tc_inner).map_err(ErrorModel::from)?;
                    max_row = max_row.max(row + row_span);
                    max_col = max_col.max(col + col_span);
                    cells.push(TableCellMeta {
                        idx: cell_idx,
                        row,
                        col,
                        row_span,
                        col_span,
                        text,
                        bbox: None,
                    });
                    cell_idx += 1;
                }
            }
            if max_row == 0 {
                max_row = trs.len();
            }
            out.push(TableCoordinateMeta {
                table_index: loc.table_index,
                sec: loc.section_index,
                para: 0,
                ctrl: 0,
                row_count: max_row,
                col_count: max_col,
                parent_table_index: loc.parent_table_index,
                parent_cell: loc.parent_cell,
                depth: Some(loc.depth),
                cells,
            });
        }
        Ok(out)
    }
}

#[allow(clippy::too_many_arguments)]
fn walk_tables_recursive(
    section: &str,
    section_index: usize,
    fragment: &str,
    base_offset: usize,
    depth: usize,
    parent_table_index: Option<usize>,
    parent_cell: Option<ParentCellCoord>,
    out: &mut Vec<TableLocation>,
) -> Result<(), HwpxError> {
    for span in scan_top_level(fragment, "hp:tbl")? {
        let abs_span = XmlElementSpan {
            start: base_offset + span.start,
            open_end: base_offset + span.open_end,
            close_start: base_offset + span.close_start,
            end: base_offset + span.end,
            self_closing: span.self_closing,
        };
        let cur_idx = out.len();
        out.push(TableLocation {
            section: section.to_string(),
            section_index,
            table_index: cur_idx,
            depth,
            parent_table_index,
            parent_cell,
            span: abs_span,
        });

        let tbl_inner = span.inner(fragment);
        let trs = scan_top_level(tbl_inner, "hp:tr")?;
        for (ri, tr) in trs.iter().enumerate() {
            let tr_inner = tr.inner(tbl_inner);
            let tcs = scan_top_level(tr_inner, "hp:tc")?;
            for (ci, tc) in tcs.iter().enumerate() {
                let tc_inner = tc.inner(tr_inner);
                let tail = cell_tail(tc_inner);
                let (col, row) = read_cell_addr(tail, ci, ri);
                if !scan_top_level(tc_inner, "hp:tbl")?.is_empty() {
                    let tc_base = base_offset + span.open_end + tr.open_end + tc.open_end;
                    walk_tables_recursive(
                        section,
                        section_index,
                        tc_inner,
                        tc_base,
                        depth + 1,
                        Some(cur_idx),
                        Some(ParentCellCoord { row, col }),
                        out,
                    )?;
                }
            }
        }
    }
    Ok(())
}

/// Return the cell metadata slice after the last `</hp:subList>` closing tag
/// so nested table metadata inside `subList` is ignored and no unmatched end tag remains.
pub fn cell_tail(tc_inner: &str) -> &str {
    const CLOSE: &str = "</hp:subList>";
    tc_inner
        .rfind(CLOSE)
        .map(|p| &tc_inner[p + CLOSE.len()..])
        .unwrap_or(tc_inner)
}

pub fn read_cell_addr(tail: &str, fallback_col: usize, fallback_row: usize) -> (usize, usize) {
    if let Ok(addrs) = scan_top_level(tail, "hp:cellAddr") {
        if let Some(first) = addrs.first() {
            let attrs = first.attrs(tail, "hp:cellAddr");
            let col = get_attr(attrs, "colAddr")
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(fallback_col);
            let row = get_attr(attrs, "rowAddr")
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(fallback_row);
            return (col, row);
        }
    }
    (fallback_col, fallback_row)
}

pub fn read_cell_span(tail: &str) -> (usize, usize) {
    if let Ok(spans) = scan_top_level(tail, "hp:cellSpan") {
        if let Some(first) = spans.first() {
            let attrs = first.attrs(tail, "hp:cellSpan");
            let col_span = get_attr(attrs, "colSpan")
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(1);
            let row_span = get_attr(attrs, "rowSpan")
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(1);
            return (col_span.max(1), row_span.max(1));
        }
    }
    (1, 1)
}

pub fn read_cell_size(tail: &str) -> Option<(i32, i32)> {
    let szs = scan_top_level(tail, "hp:cellSz").ok()?;
    let first = szs.first()?;
    let attrs = first.attrs(tail, "hp:cellSz");
    let w = get_attr(attrs, "width")?.parse::<i32>().ok()?;
    let h = get_attr(attrs, "height")?.parse::<i32>().ok()?;
    Some((w, h))
}

pub fn norm_hex(c: &str) -> String {
    format!("#{}", c.trim().trim_start_matches('#').to_ascii_uppercase())
}

pub fn format_hu_as_mm(hu: i32) -> String {
    let mm = (hu as f64) / HWPUNIT_PER_MM;
    let rounded = (mm * 100.0).round() / 100.0;
    let mut s = format!("{rounded:.2}");
    while s.contains('.') && s.ends_with('0') {
        s.pop();
    }
    if s.ends_with('.') {
        s.pop();
    }
    format!("{s} mm")
}

pub fn run_with_text(char_pr_id: &str, text: &str) -> String {
    format!(
        r#"<hp:run charPrIDRef="{char_pr_id}"><hp:t>{}</hp:t></hp:run>"#,
        xml_escape_text(text)
    )
}

pub fn first_char_pr_id(xml: &str) -> String {
    if let Ok(runs) = scan_top_level(xml, "hp:run") {
        for r in runs {
            if let Some(id) = get_attr(r.attrs(xml, "hp:run"), "charPrIDRef") {
                return id;
            }
        }
    }
    if let Some(idx) = xml.find(r#"charPrIDRef=""#) {
        let rest = &xml[idx + r#"charPrIDRef=""#.len()..];
        if let Some(end) = rest.find('"') {
            return rest[..end].to_string();
        }
    }
    "0".to_string()
}

pub fn sanitize_attr_replace(attrs: &str, name: &str, value: &str) -> String {
    set_or_add_attr(attrs, name, value)
}
