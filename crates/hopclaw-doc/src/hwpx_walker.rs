//! Pure-Rust HWPX `TableWalker`, structure inspector, and markdown extractor
//! using `zip` + `quick-xml`, matching `src/core/hwpxTableIndex.ts` and
//! `resources/claw-hwp/scripts/extract_text.js`.

use std::io::Read;

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use zip::ZipArchive;

use hopclaw_types::{validate_preorder_coords, ErrorModel};

use crate::types::{
    InspectDocumentResult, ParentCellCoord, TableCellMeta, TableCoordinateMeta, TableWalker,
};
use crate::DocError;

/// Returns `true` when `bytes` begins with a ZIP local-file-header (`PK`).
pub fn is_hwpx_bytes(bytes: &[u8]) -> bool {
    bytes.len() >= 2 && bytes[0] == 0x50 && bytes[1] == 0x4b
}

/// Extract `Contents/section<N>.xml` entries sorted numerically by `N`.
pub fn extract_section_xmls(bytes: &[u8]) -> Result<Vec<(usize, String)>, DocError> {
    let cursor = std::io::Cursor::new(bytes);
    let mut archive = ZipArchive::new(cursor)?;
    let mut indexed: Vec<(usize, String)> = Vec::new();

    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let name = file.name().to_string();
        if let Some(sec_num) = parse_section_filename(&name) {
            let mut buf = Vec::with_capacity(file.size() as usize);
            file.read_to_end(&mut buf)?;
            let xml = String::from_utf8(buf)?;
            indexed.push((sec_num, xml));
        }
    }

    indexed.sort_by_key(|(idx, _)| *idx);
    Ok(indexed)
}

fn parse_section_filename(name: &str) -> Option<usize> {
    let lower = name.to_ascii_lowercase();
    let rest = lower.strip_prefix("contents/section")?;
    let digits = rest.strip_suffix(".xml")?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse::<usize>().ok()
}

/// Parsed cell returned by `parse_hwpx_section_tables`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HwpxParsedCell {
    pub idx: usize,
    pub row: usize,
    pub col: usize,
    pub row_span: usize,
    pub col_span: usize,
    pub text: String,
}

/// Parsed table in canonical document pre-order, mirroring `HwpxParsedTable` in `hwpxTableIndex.ts`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HwpxParsedTable {
    pub section_index: usize,
    pub table_index: usize,
    pub depth: usize,
    pub parent_table_index: Option<usize>,
    pub parent_cell: Option<ParentCellCoord>,
    pub row_count: usize,
    pub col_count: usize,
    pub cells: Vec<HwpxParsedCell>,
}

struct ActiveCell {
    idx: usize,
    row: usize,
    col: usize,
    row_span: usize,
    col_span: usize,
    text: String,
    sublist_depth: usize,
}

struct ActiveTable {
    slot_idx: usize,
    table_index: usize,
    depth: usize,
    parent_table_index: Option<usize>,
    parent_cell: Option<ParentCellCoord>,
    row_idx: usize,
    col_idx: usize,
    max_row: usize,
    max_col: usize,
    cells: Vec<HwpxParsedCell>,
    active_cell: Option<ActiveCell>,
}

/// Parse all tables from an HWPX section XML string in canonical pre-order traversal order.
///
/// Uses a single streaming `quick-xml` pass with an explicit table stack so nested `<hp:tbl>`
/// elements receive `depth = parent.depth + 1`, `parent_table_index`, and `parent_cell` while
/// allocating `table_index` in exact pre-order (`start` event order).
pub fn parse_hwpx_section_tables(
    xml: &str,
    section_index: usize,
    start_index_offset: usize,
) -> Result<Vec<HwpxParsedTable>, DocError> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);

    let mut slots: Vec<Option<HwpxParsedTable>> = Vec::new();
    let mut stack: Vec<ActiveTable> = Vec::new();
    let mut in_text_elem = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                let name = e.name();
                match name.as_ref() {
                    "hp:tbl" => {
                        let depth = stack.len();
                        let (parent_table_index, parent_cell) = if let Some(parent) = stack.last() {
                            let pcell = parent
                                .active_cell
                                .as_ref()
                                .map(|c| ParentCellCoord { row: c.row, col: c.col });
                            (Some(parent.table_index), pcell)
                        } else {
                            (None, None)
                        };
                        let slot_idx = slots.len();
                        let table_index = start_index_offset + slot_idx;
                        slots.push(None);
                        stack.push(ActiveTable {
                            slot_idx,
                            table_index,
                            depth,
                            parent_table_index,
                            parent_cell,
                            row_idx: 0,
                            col_idx: 0,
                            max_row: 0,
                            max_col: 0,
                            cells: Vec::new(),
                            active_cell: None,
                        });
                    }
                    "hp:tr" => {
                        if let Some(tbl) = stack.last_mut() {
                            tbl.col_idx = 0;
                        }
                    }
                    "hp:tc" => {
                        if let Some(tbl) = stack.last_mut() {
                            let idx = tbl.cells.len();
                            tbl.active_cell = Some(ActiveCell {
                                idx,
                                row: tbl.row_idx,
                                col: tbl.col_idx,
                                row_span: 1,
                                col_span: 1,
                                text: String::new(),
                                sublist_depth: 0,
                            });
                        }
                    }
                    "hp:subList" => {
                        if let Some(tbl) = stack.last_mut() {
                            if let Some(cell) = tbl.active_cell.as_mut() {
                                cell.sublist_depth += 1;
                            }
                        }
                    }
                    "hp:cellAddr" => apply_cell_addr(&mut stack, e),
                    "hp:cellSpan" => apply_cell_span(&mut stack, e),
                    "hp:t" => {
                        in_text_elem = true;
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(ref e)) => {
                let name = e.name();
                match name.as_ref() {
                    "hp:cellAddr" => apply_cell_addr(&mut stack, e),
                    "hp:cellSpan" => apply_cell_span(&mut stack, e),
                    "hp:tbl" => {
                        let depth = stack.len();
                        let (parent_table_index, parent_cell) = if let Some(parent) = stack.last() {
                            let pcell = parent
                                .active_cell
                                .as_ref()
                                .map(|c| ParentCellCoord { row: c.row, col: c.col });
                            (Some(parent.table_index), pcell)
                        } else {
                            (None, None)
                        };
                        let slot_idx = slots.len();
                        let table_index = start_index_offset + slot_idx;
                        slots.push(Some(HwpxParsedTable {
                            section_index,
                            table_index,
                            depth,
                            parent_table_index,
                            parent_cell,
                            row_count: 0,
                            col_count: 0,
                            cells: Vec::new(),
                        }));
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(ref e)) => {
                if in_text_elem {
                    push_text_to_active_cells(&mut stack, e.as_ref());
                }
            }
            Ok(Event::GeneralRef(ref e)) => {
                if in_text_elem {
                    let decoded = decode_general_ref(e.as_ref());
                    push_text_to_active_cells(&mut stack, &decoded);
                }
            }
            Ok(Event::End(ref e)) => {
                let name = e.name();
                match name.as_ref() {
                    "hp:t" => {
                        in_text_elem = false;
                    }
                    "hp:subList" => {
                        if let Some(tbl) = stack.last_mut() {
                            if let Some(cell) = tbl.active_cell.as_mut() {
                                cell.sublist_depth = cell.sublist_depth.saturating_sub(1);
                            }
                        }
                    }
                    "hp:tc" => {
                        if let Some(tbl) = stack.last_mut() {
                            if let Some(cell) = tbl.active_cell.take() {
                                tbl.max_row = tbl.max_row.max(cell.row + cell.row_span);
                                tbl.max_col = tbl.max_col.max(cell.col + cell.col_span);
                                tbl.cells.push(HwpxParsedCell {
                                    idx: cell.idx,
                                    row: cell.row,
                                    col: cell.col,
                                    row_span: cell.row_span,
                                    col_span: cell.col_span,
                                    text: cell.text,
                                });
                                tbl.col_idx += 1;
                            }
                        }
                    }
                    "hp:tr" => {
                        if let Some(tbl) = stack.last_mut() {
                            tbl.row_idx += 1;
                        }
                    }
                    "hp:tbl" => {
                        if let Some(tbl) = stack.pop() {
                            let row_count = if tbl.max_row == 0 {
                                tbl.row_idx
                            } else {
                                tbl.max_row
                            };
                            slots[tbl.slot_idx] = Some(HwpxParsedTable {
                                section_index,
                                table_index: tbl.table_index,
                                depth: tbl.depth,
                                parent_table_index: tbl.parent_table_index,
                                parent_cell: tbl.parent_cell,
                                row_count,
                                col_count: tbl.max_col,
                                cells: tbl.cells,
                            });
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(err) => {
                return Err(DocError::XmlParse {
                    section: format!("section{section_index}"),
                    message: err.to_string(),
                });
            }
            _ => {}
        }
    }

    Ok(slots.into_iter().flatten().collect())
}

fn push_text_to_active_cells(stack: &mut [ActiveTable], txt: &str) {
    // Mirror `hwpxTableIndex.ts`: `<hp:tc>` text includes `<hp:t>` within that cell element.
    for tbl in stack.iter_mut() {
        if let Some(cell) = tbl.active_cell.as_mut() {
            cell.text.push_str(txt);
        }
    }
}

fn apply_cell_addr(stack: &mut [ActiveTable], e: &BytesStart<'_>) {
    let Some(tbl) = stack.last_mut() else {
        return;
    };
    let Some(cell) = tbl.active_cell.as_mut() else {
        return;
    };
    if cell.sublist_depth != 0 {
        return;
    }
    for attr in e.attributes().flatten() {
        let key = attr.key.as_ref();
        if key == "colAddr" {
            if let Ok(n) = attr.value.as_ref().parse::<usize>() {
                cell.col = n;
            }
        } else if key == "rowAddr" {
            if let Ok(n) = attr.value.as_ref().parse::<usize>() {
                cell.row = n;
            }
        }
    }
}

fn apply_cell_span(stack: &mut [ActiveTable], e: &BytesStart<'_>) {
    let Some(tbl) = stack.last_mut() else {
        return;
    };
    let Some(cell) = tbl.active_cell.as_mut() else {
        return;
    };
    if cell.sublist_depth != 0 {
        return;
    }
    for attr in e.attributes().flatten() {
        let key = attr.key.as_ref();
        if key == "colSpan" {
            if let Ok(n) = attr.value.as_ref().parse::<usize>() {
                cell.col_span = n;
            }
        } else if key == "rowSpan" {
            if let Ok(n) = attr.value.as_ref().parse::<usize>() {
                cell.row_span = n;
            }
        }
    }
}

fn decode_general_ref(name: &str) -> String {
    match name {
        "amp" => "&".to_string(),
        "lt" => "<".to_string(),
        "gt" => ">".to_string(),
        "quot" => "\"".to_string(),
        "apos" => "'".to_string(),
        _ if name.starts_with('#') => {
            let code = if let Some(hex) = name.strip_prefix("#x").or_else(|| name.strip_prefix("#X"))
            {
                u32::from_str_radix(hex, 16).ok()
            } else {
                name[1..].parse::<u32>().ok()
            };
            code.and_then(char::from_u32)
                .map(|c| c.to_string())
                .unwrap_or_else(|| format!("&{name};"))
        }
        _ => format!("&{name};"),
    }
}

/// Parse all tables across all `Contents/section*.xml` files from HWPX package bytes.
pub fn parse_hwpx_tables_from_bytes(bytes: &[u8]) -> Result<Vec<HwpxParsedTable>, DocError> {
    let sections = extract_section_xmls(bytes)?;
    let mut all_tables = Vec::new();
    for (sec_idx, (_, xml)) in sections.iter().enumerate() {
        let sec_tables = parse_hwpx_section_tables(xml, sec_idx, all_tables.len())?;
        all_tables.extend(sec_tables);
    }
    Ok(all_tables)
}

/// Merge pre-order HWPX parsed tables with rhwp top-level tables, preserving bboxes and
/// `(sec, para, ctrl)` coordinates while shifting nested tables into pre-order indices.
/// Falls back to `rhwp_top_tables` (with `depth = 0`) if counts or dimensions mismatch.
pub fn merge_hwpx_tables_with_rhwp(
    hwpx_tables: &[HwpxParsedTable],
    rhwp_top_tables: &[TableCoordinateMeta],
) -> Vec<TableCoordinateMeta> {
    let hwpx_top: Vec<&HwpxParsedTable> = hwpx_tables.iter().filter(|t| t.depth == 0).collect();

    let mismatch = if hwpx_top.len() != rhwp_top_tables.len() {
        true
    } else {
        hwpx_top.iter().zip(rhwp_top_tables.iter()).any(|(ht, rt)| {
            ht.row_count != rt.row_count || ht.col_count != rt.col_count
        })
    };

    if mismatch {
        return rhwp_top_tables
            .iter()
            .cloned()
            .map(|mut t| {
                t.depth = Some(0);
                t
            })
            .collect();
    }

    let mut merged: Vec<TableCoordinateMeta> = Vec::with_capacity(hwpx_tables.len());
    let mut top_idx = 0usize;

    for t in hwpx_tables {
        if t.depth == 0 {
            let top_meta = &rhwp_top_tables[top_idx];
            top_idx += 1;
            let cells = t
                .cells
                .iter()
                .map(|c| {
                    let matching_bbox = top_meta
                        .cells
                        .iter()
                        .find(|rc| rc.row == c.row && rc.col == c.col)
                        .and_then(|rc| rc.bbox.clone());
                    TableCellMeta {
                        idx: c.idx,
                        row: c.row,
                        col: c.col,
                        row_span: c.row_span,
                        col_span: c.col_span,
                        text: c.text.clone(),
                        bbox: matching_bbox,
                    }
                })
                .collect();

            merged.push(TableCoordinateMeta {
                table_index: t.table_index,
                sec: top_meta.sec,
                para: top_meta.para,
                ctrl: top_meta.ctrl,
                row_count: t.row_count,
                col_count: t.col_count,
                parent_table_index: None,
                parent_cell: None,
                depth: Some(0),
                cells,
            });
        } else {
            let parent_meta = t.parent_table_index.and_then(|pi| merged.get(pi)).cloned();
            let cells = t
                .cells
                .iter()
                .map(|c| TableCellMeta {
                    idx: c.idx,
                    row: c.row,
                    col: c.col,
                    row_span: c.row_span,
                    col_span: c.col_span,
                    text: c.text.clone(),
                    bbox: None,
                })
                .collect();

            merged.push(TableCoordinateMeta {
                table_index: t.table_index,
                sec: parent_meta
                    .as_ref()
                    .map(|p| p.sec)
                    .unwrap_or(t.section_index),
                para: parent_meta.as_ref().map(|p| p.para).unwrap_or(0),
                ctrl: parent_meta.as_ref().map(|p| p.ctrl).unwrap_or(0),
                row_count: t.row_count,
                col_count: t.col_count,
                parent_table_index: t.parent_table_index,
                parent_cell: t.parent_cell,
                depth: Some(t.depth),
                cells,
            });
        }
    }

    merged
}

/// Build full `TableCoordinateMeta` list for HWPX bytes, optionally merging `rhwp_tables`.
pub fn build_tables_meta_for_hwpx(
    bytes: &[u8],
    rhwp_tables: Option<&[TableCoordinateMeta]>,
) -> Vec<TableCoordinateMeta> {
    let hwpx_tables = match parse_hwpx_tables_from_bytes(bytes) {
        Ok(t) => t,
        Err(_) => {
            return rhwp_tables
                .unwrap_or(&[])
                .iter()
                .cloned()
                .map(|mut t| {
                    t.depth = Some(0);
                    t
                })
                .collect();
        }
    };

    if let Some(rt) = rhwp_tables {
        let rhwp_top: Vec<TableCoordinateMeta> = rt
            .iter()
            .filter(|t| t.depth.unwrap_or(0) == 0)
            .cloned()
            .collect();
        if !rhwp_top.is_empty() {
            return merge_hwpx_tables_with_rhwp(&hwpx_tables, &rhwp_top);
        }
    }

    let mut result: Vec<TableCoordinateMeta> = Vec::with_capacity(hwpx_tables.len());
    for t in &hwpx_tables {
        if t.depth == 0 {
            result.push(TableCoordinateMeta {
                table_index: t.table_index,
                sec: t.section_index,
                para: 0,
                ctrl: 0,
                row_count: t.row_count,
                col_count: t.col_count,
                parent_table_index: None,
                parent_cell: None,
                depth: Some(0),
                cells: t
                    .cells
                    .iter()
                    .map(|c| TableCellMeta {
                        idx: c.idx,
                        row: c.row,
                        col: c.col,
                        row_span: c.row_span,
                        col_span: c.col_span,
                        text: c.text.clone(),
                        bbox: None,
                    })
                    .collect(),
            });
        } else {
            let parent_meta = t.parent_table_index.and_then(|pi| result.get(pi)).cloned();
            result.push(TableCoordinateMeta {
                table_index: t.table_index,
                sec: parent_meta
                    .as_ref()
                    .map(|p| p.sec)
                    .unwrap_or(t.section_index),
                para: parent_meta.as_ref().map(|p| p.para).unwrap_or(0),
                ctrl: parent_meta.as_ref().map(|p| p.ctrl).unwrap_or(0),
                row_count: t.row_count,
                col_count: t.col_count,
                parent_table_index: t.parent_table_index,
                parent_cell: t.parent_cell,
                depth: Some(t.depth),
                cells: t
                    .cells
                    .iter()
                    .map(|c| TableCellMeta {
                        idx: c.idx,
                        row: c.row,
                        col: c.col,
                        row_span: c.row_span,
                        col_span: c.col_span,
                        text: c.text.clone(),
                        bbox: None,
                    })
                    .collect(),
            });
        }
    }
    result
}

/// Pure-Rust `TableWalker` implementation for HWPX documents.
#[derive(Debug, Clone, Copy)]
pub struct HwpxTableWalker<'a> {
    bytes: &'a [u8],
}

impl<'a> HwpxTableWalker<'a> {
    /// Wrap raw HWPX package bytes for canonical pre-order table traversal.
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes }
    }

    /// Convenience helper matching `walk_tables` with [`DocError`] error type.
    pub fn walk_bytes(bytes: &[u8]) -> Result<Vec<TableCoordinateMeta>, DocError> {
        if !is_hwpx_bytes(bytes) {
            return Err(DocError::InvalidDocument(
                "HwpxTableWalker requires ZIP/HWPX bytes".to_string(),
            ));
        }
        let hwpx_tables = parse_hwpx_tables_from_bytes(bytes)?;
        let mut out: Vec<TableCoordinateMeta> = Vec::with_capacity(hwpx_tables.len());
        for t in &hwpx_tables {
            let parent_sec = t
                .parent_table_index
                .and_then(|pi| out.get(pi))
                .map(|p| p.sec)
                .unwrap_or(t.section_index);
            out.push(TableCoordinateMeta {
                table_index: t.table_index,
                sec: parent_sec,
                para: 0,
                ctrl: 0,
                row_count: t.row_count,
                col_count: t.col_count,
                parent_table_index: t.parent_table_index,
                parent_cell: t.parent_cell,
                depth: Some(t.depth),
                cells: t
                    .cells
                    .iter()
                    .map(|c| TableCellMeta {
                        idx: c.idx,
                        row: c.row,
                        col: c.col,
                        row_span: c.row_span,
                        col_span: c.col_span,
                        text: c.text.clone(),
                        bbox: None,
                    })
                    .collect(),
            });
        }
        Ok(out)
    }
}

impl TableWalker for HwpxTableWalker<'_> {
    fn walk_tables(&self) -> Result<Vec<TableCoordinateMeta>, ErrorModel> {
        let tables = Self::walk_bytes(self.bytes).map_err(ErrorModel::from)?;
        validate_preorder_coords(&tables)?;
        Ok(tables)
    }
}

/// Inspect an HWPX package, matching `extract_text.js --inspect` counts (`hp:p`, `hp:tbl`, `hp:tc`, `hp:pic`).
pub fn inspect_hwpx(bytes: &[u8], with_cell_text: bool) -> Result<InspectDocumentResult, DocError> {
    if !is_hwpx_bytes(bytes) {
        return Err(DocError::InvalidDocument(
            "expected HWPX ZIP container".to_string(),
        ));
    }
    let sections = extract_section_xmls(bytes)?;
    let mut paragraph_count = 0usize;
    let mut table_count = 0usize;
    let mut cell_count = 0usize;
    let mut image_count = 0usize;

    for (sec_idx, (_, xml)) in sections.iter().enumerate() {
        let mut reader = Reader::from_str(xml);
        loop {
            match reader.read_event() {
                Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                    let name = e.name();
                    match name.as_ref() {
                        "hp:p" => paragraph_count += 1,
                        "hp:tbl" => table_count += 1,
                        "hp:tc" => cell_count += 1,
                        "hp:pic" => image_count += 1,
                        _ => {}
                    }
                }
                Ok(Event::Eof) => break,
                Err(err) => {
                    return Err(DocError::XmlParse {
                        section: format!("section{sec_idx}"),
                        message: err.to_string(),
                    });
                }
                _ => {}
            }
        }
    }

    let tables = if with_cell_text {
        Some(HwpxTableWalker::walk_bytes(bytes)?)
    } else {
        None
    };

    Ok(InspectDocumentResult {
        file_type: "hwpx".to_string(),
        section_count: sections.len(),
        paragraph_count,
        table_count,
        cell_count,
        image_count: Some(image_count),
        tables,
    })
}

/// Extract structured markdown from HWPX bytes, matching `extract_text.js --format markdown`.
pub fn extract_hwpx_markdown(bytes: &[u8]) -> Result<String, DocError> {
    if !is_hwpx_bytes(bytes) {
        return Err(DocError::InvalidDocument(
            "expected HWPX ZIP container".to_string(),
        ));
    }
    let sections = extract_section_xmls(bytes)?;
    let mut lines: Vec<String> = Vec::new();

    for (i, (_, xml)) in sections.iter().enumerate() {
        if i > 0 {
            lines.push(String::new());
        }
        extract_section_markdown(xml, i, &mut lines)?;
    }

    let mut out = lines.join("\n");
    out.push('\n');
    Ok(out)
}

fn extract_section_markdown(
    xml: &str,
    section_index: usize,
    lines: &mut Vec<String>,
) -> Result<(), DocError> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);

    let mut tbl_depth = 0usize;
    let mut current_rows: Vec<Vec<String>> = Vec::new();
    let mut current_row: Vec<String> = Vec::new();
    let mut in_cell = false;
    let mut cell_buf = String::new();
    let mut para_buf = String::new();
    let mut in_t = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                let name = e.name();
                match name.as_ref() {
                    "hp:tbl" => {
                        if tbl_depth == 0 {
                            // Flush any non-table paragraph text accumulated before this table
                            // (mirrors extractParagraphs(xml.slice(lastEnd, m.index))).
                            if !para_buf.trim().is_empty() {
                                lines.push(std::mem::take(&mut para_buf));
                            } else {
                                para_buf.clear();
                            }
                            current_rows.clear();
                        }
                        tbl_depth += 1;
                    }
                    "hp:tr" if tbl_depth == 1 => {
                        current_row.clear();
                    }
                    "hp:tc" if tbl_depth == 1 => {
                        in_cell = true;
                        cell_buf.clear();
                    }
                    "hp:t" => {
                        in_t = true;
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(ref e)) => {
                if in_t {
                    let txt = e.as_ref();
                    if tbl_depth > 0 && in_cell {
                        cell_buf.push_str(txt);
                    } else if tbl_depth == 0 {
                        para_buf.push_str(txt);
                    }
                }
            }
            Ok(Event::GeneralRef(ref e)) => {
                if in_t {
                    let txt = decode_general_ref(e.as_ref());
                    if tbl_depth > 0 && in_cell {
                        cell_buf.push_str(&txt);
                    } else if tbl_depth == 0 {
                        para_buf.push_str(&txt);
                    }
                }
            }
            Ok(Event::End(ref e)) => {
                let name = e.name();
                match name.as_ref() {
                    "hp:t" => {
                        in_t = false;
                    }
                    "hp:p" if tbl_depth == 0 => {
                        if !para_buf.trim().is_empty() {
                            lines.push(std::mem::take(&mut para_buf));
                        } else {
                            para_buf.clear();
                        }
                    }
                    "hp:tc" if tbl_depth == 1 => {
                        if in_cell {
                            let cleaned = cell_buf
                                .replace('|', "\\|")
                                .replace('\n', " ")
                                .trim()
                                .to_string();
                            current_row.push(if cleaned.is_empty() {
                                " ".to_string()
                            } else {
                                cleaned
                            });
                            cell_buf.clear();
                            in_cell = false;
                        }
                    }
                    "hp:tr" if tbl_depth == 1 => {
                        if !current_row.is_empty() {
                            current_rows.push(std::mem::take(&mut current_row));
                        }
                    }
                    "hp:tbl" if tbl_depth > 0 => {
                        tbl_depth -= 1;
                        if tbl_depth == 0 {
                            emit_markdown_table(&current_rows, lines);
                            current_rows.clear();
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(err) => {
                return Err(DocError::XmlParse {
                    section: format!("section{section_index}"),
                    message: err.to_string(),
                });
            }
            _ => {}
        }
    }

    Ok(())
}

fn emit_markdown_table(rows: &[Vec<String>], lines: &mut Vec<String>) {
    if rows.is_empty() {
        return;
    }
    let has_content = rows.iter().any(|r| r.iter().any(|c| !c.trim().is_empty()));
    if !has_content {
        return;
    }
    let col_count = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    if col_count == 0 {
        return;
    }

    let pad_row = |r: &[String]| -> Vec<String> {
        let mut out = r.to_vec();
        while out.len() < col_count {
            out.push(" ".to_string());
        }
        out
    };

    if let Some(last) = lines.last() {
        if !last.is_empty() {
            lines.push(String::new());
        }
    }
    let header = pad_row(&rows[0]);
    lines.push(format!("| {} |", header.join(" | ")));
    let sep = vec!["---"; col_count];
    lines.push(format!("| {} |", sep.join(" | ")));
    for row in &rows[1..] {
        let padded = pad_row(row);
        lines.push(format!("| {} |", padded.join(" | ")));
    }
    lines.push(String::new());
}
