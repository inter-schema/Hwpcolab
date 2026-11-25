use hopclaw_types::{
    BorderLineType as TypesBorderLine, BorderSide as TypesBorderSide, CellHAlign, CellVAlign,
    ColInsertPosition, EqualizeMode, Hwp5TableTarget, ParentCellCoord, RowInsertPosition,
    TableCellMeta, TableCoordinateMeta, HWPUNIT_PER_MM,
};

use crate::docinfo::{
    parse_hex_color, BorderFillBuilder, BorderLineType, BorderSide, CharShapePatch, DocInfoDoc,
    ParaAlignment,
};
use crate::error::{Hwp5Error, Result};
use crate::record::{tag, RawRecord};
use crate::section::{
    decode_para_text, encode_para_text, parse_para_char_shapes, serialize_para_char_shapes,
    CharShapeRun, ParaHeaderMut, ParaHeaderRef, SectionDoc, TableCellAttr,
};
use crate::text::ensure_aligned_para_shape;

const CTRL_TBL_LE: &[u8; 4] = b" lbt"; // 'tbl ' in little-endian u32

/// Walk all tables across a single section in canonical pre-order (supporting pre-existing nested tables).
pub fn walk_section_tables(
    sec: &SectionDoc,
    sec_idx: usize,
    out: &mut Vec<TableCoordinateMeta>,
) -> Result<()> {
    let top_paras = sec.top_level_para_indices();
    for (para_idx, &ph_rec_idx) in top_paras.iter().enumerate() {
        let para_end = top_paras
            .get(para_idx + 1)
            .copied()
            .unwrap_or(sec.records.len());
        walk_container_tables(
            &sec.records,
            ph_rec_idx + 1,
            para_end,
            1,
            sec_idx,
            para_idx,
            0,
            None,
            None,
            out,
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn walk_container_tables(
    records: &[RawRecord],
    start: usize,
    end: usize,
    ctrl_level: u16,
    sec_idx: usize,
    para_idx: usize,
    depth: usize,
    parent_table_index: Option<usize>,
    parent_cell: Option<ParentCellCoord>,
    out: &mut Vec<TableCoordinateMeta>,
) -> Result<()> {
    let mut ctrl_idx = 0usize;
    let mut i = start;
    while i < end {
        if records[i].tag == tag::CTRL_HEADER && records[i].level == ctrl_level {
            let ctrl_end = ((i + 1)..end)
                .find(|&k| records[k].tag == tag::CTRL_HEADER && records[k].level <= ctrl_level)
                .unwrap_or(end);

            let table_level = ctrl_level + 1;
            if let Some(t_rec_idx) = ((i + 1)..ctrl_end)
                .find(|&k| records[k].tag == tag::TABLE && records[k].level == table_level)
            {
                let t_body = &records[t_rec_idx].body;
                if t_body.len() < 8 {
                    return Err(Hwp5Error::MalformedRecord(format!(
                        "TABLE record too short ({} bytes)",
                        t_body.len()
                    )));
                }
                let row_count = u16::from_le_bytes(t_body[4..6].try_into().unwrap()) as usize;
                let col_count = u16::from_le_bytes(t_body[6..8].try_into().unwrap()) as usize;

                let lh_indices: Vec<usize> = ((i + 1)..ctrl_end)
                    .filter(|&k| records[k].tag == tag::LIST_HEADER && records[k].level == table_level)
                    .collect();

                let this_table_index = out.len();
                let mut cells_meta = Vec::with_capacity(lh_indices.len());
                let mut nested_spawns: Vec<(usize, usize, ParentCellCoord)> = Vec::new();

                for (cell_idx, &lh_idx) in lh_indices.iter().enumerate() {
                    let cell_end = lh_indices.get(cell_idx + 1).copied().unwrap_or(ctrl_end);
                    let attr = TableCellAttr::parse(&records[lh_idx].body)?;
                    let text = extract_cell_plain_text(records, lh_idx, cell_end, table_level)?;
                    cells_meta.push(TableCellMeta {
                        idx: cell_idx,
                        row: attr.row as usize,
                        col: attr.col as usize,
                        row_span: attr.row_span.max(1) as usize,
                        col_span: attr.col_span.max(1) as usize,
                        text,
                        bbox: None,
                    });
                    nested_spawns.push((
                        lh_idx + 1,
                        cell_end,
                        ParentCellCoord {
                            row: attr.row as usize,
                            col: attr.col as usize,
                        },
                    ));
                }

                out.push(TableCoordinateMeta {
                    table_index: this_table_index,
                    sec: sec_idx,
                    para: para_idx,
                    ctrl: ctrl_idx,
                    row_count,
                    col_count,
                    parent_table_index,
                    parent_cell,
                    depth: Some(depth),
                    cells: cells_meta,
                });

                // Pre-order recurse into nested tables inside this table's cells.
                for (c_start, c_end, p_coord) in nested_spawns {
                    walk_container_tables(
                        records,
                        c_start,
                        c_end,
                        table_level + 1,
                        sec_idx,
                        para_idx,
                        depth + 1,
                        Some(this_table_index),
                        Some(p_coord),
                        out,
                    )?;
                }
            }
            ctrl_idx += 1;
            i = ctrl_end;
        } else {
            i += 1;
        }
    }
    Ok(())
}

fn extract_cell_plain_text(
    records: &[RawRecord],
    lh_idx: usize,
    cell_end: usize,
    cell_level: u16,
) -> Result<String> {
    let text_level = cell_level + 1;
    let mut paras = Vec::new();
    let mut k = lh_idx + 1;
    while k < cell_end {
        if records[k].tag == tag::PARA_HEADER && records[k].level == cell_level {
            let p_end = ((k + 1)..cell_end)
                .find(|&m| records[m].tag == tag::PARA_HEADER && records[m].level == cell_level)
                .unwrap_or(cell_end);
            let mut para_text = String::new();
            for rec in &records[(k + 1)..p_end] {
                if rec.tag == tag::PARA_TEXT && rec.level == text_level {
                    para_text = decode_para_text(&rec.body)?.text;
                    break;
                }
            }
            paras.push(para_text);
            k = p_end;
        } else {
            k += 1;
        }
    }
    Ok(paras.join("\n").trim_end_matches('\n').to_string())
}

/// Resolve an [`Hwp5TableTarget`] into `(section_index, table_index_in_section)`.
pub fn resolve_target_table_in_section(
    sec: &SectionDoc,
    target: &Hwp5TableTarget,
    op_name: &str,
) -> Result<crate::section::SectionTableInfo> {
    let tables = sec.enumerate_tables()?;
    if let (Some(para), Some(ctrl)) = (target.para, target.ctrl) {
        if let Some(found) = tables
            .iter()
            .find(|t| t.para_index == para && t.ctrl_index == ctrl)
        {
            return Ok(found.clone());
        }
        return Err(Hwp5Error::TargetNotFound(format!(
            "{op_name}: table at sec={} para={para} ctrl={ctrl} not found (section has {} tables)",
            target.sec.unwrap_or(0),
            tables.len()
        )));
    }
    tables.get(target.table_index).cloned().ok_or_else(|| {
        Hwp5Error::TargetNotFound(format!(
            "{op_name}: tableIndex {} out of range (section has {} tables)",
            target.table_index,
            tables.len()
        ))
    })
}

fn find_cell_by_grid<'a>(
    tbl: &'a crate::section::SectionTableInfo,
    row: usize,
    col: usize,
    op_name: &str,
) -> Result<&'a crate::section::SectionTableCellInfo> {
    let r = row as u16;
    let c = col as u16;
    tbl.cells
        .iter()
        .find(|cell| cell.attr.row == r && cell.attr.col == c)
        .or_else(|| {
            tbl.cells.iter().find(|cell| {
                r >= cell.attr.row
                    && r < cell.attr.row.saturating_add(cell.attr.row_span.max(1))
                    && c >= cell.attr.col
                    && c < cell.attr.col.saturating_add(cell.attr.col_span.max(1))
            })
        })
        .ok_or_else(|| {
            Hwp5Error::TargetNotFound(format!(
                "{op_name}: cell ({row}, {col}) not found in table {} ({}x{})",
                tbl.table_index, tbl.rows, tbl.cols
            ))
        })
}

/// Patch text in a cell (`set_cell_text`). Collapses trailing empty paragraphs and optionally fits text.
pub fn set_cell_text(
    docinfo: &mut DocInfoDoc,
    sec: &mut SectionDoc,
    target: &Hwp5TableTarget,
    row: usize,
    col: usize,
    text: &str,
    fit: bool,
) -> Result<()> {
    let clean = text
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .trim_matches('\n')
        .to_string();
    let tbl = resolve_target_table_in_section(sec, target, "set_cell_text")?;
    let cell = find_cell_by_grid(&tbl, row, col, "set_cell_text")?.clone();

    let cell_level = sec.records[cell.list_header_rec_idx].level;
    let ph_idx = ((cell.list_header_rec_idx + 1)..cell.end_rec_idx)
        .find(|&k| sec.records[k].tag == tag::PARA_HEADER && sec.records[k].level == cell_level)
        .ok_or_else(|| {
            Hwp5Error::MalformedRecord(format!("cell ({row}, {col}) has no PARA_HEADER"))
        })?;
    let p_end = ((ph_idx + 1)..cell.end_rec_idx)
        .find(|&k| sec.records[k].level <= cell_level)
        .unwrap_or(cell.end_rec_idx);

    let pt_idx = ((ph_idx + 1)..p_end)
        .find(|&k| sec.records[k].tag == tag::PARA_TEXT && sec.records[k].level == cell_level + 1);
    let cs_idx = ((ph_idx + 1)..p_end).find(|&k| {
        sec.records[k].tag == tag::PARA_CHAR_SHAPE && sec.records[k].level == cell_level + 1
    });
    let ls_idx = ((ph_idx + 1)..p_end).find(|&k| {
        sec.records[k].tag == tag::PARA_LINE_SEG && sec.records[k].level == cell_level + 1
    });
    let has_inline_obj = ((ph_idx + 1)..p_end).any(|k| sec.records[k].tag == tag::CTRL_HEADER);
    if has_inline_obj && !clean.is_empty() {
        return Err(Hwp5Error::InvalidTarget(format!(
            "set_cell_text: cell ({row}, {col}) paragraph hosts an inline object"
        )));
    }

    let final_text = if fit && !clean.is_empty() {
        if let Some(pti) = pt_idx {
            let decoded = decode_para_text(&sec.records[pti].body)?;
            fit_value_into_layout(&decoded.text, &clean)
        } else {
            clean.clone()
        }
    } else {
        clean.clone()
    };

    let last_flag = ParaHeaderRef::wrap(&sec.records[ph_idx].body)?.is_last_in_list();

    if final_text.is_empty() {
        ParaHeaderMut::wrap(&mut sec.records[ph_idx].body)?.set_char_count(1, last_flag);
        ParaHeaderMut::wrap(&mut sec.records[ph_idx].body)?.set_char_shape_count(1);
        ParaHeaderMut::wrap(&mut sec.records[ph_idx].body)?.set_line_seg_count(1);
        if let Some(csi) = cs_idx {
            if sec.records[csi].body.len() > 8 {
                sec.records[csi].body.truncate(8);
            }
        }
        if let Some(lsi) = ls_idx {
            if sec.records[lsi].body.len() > 36 {
                sec.records[lsi].body.truncate(36);
            }
        }
        if let Some(pti) = pt_idx {
            sec.records.remove(pti);
        }
    } else {
        let (pt_bytes, wchar_len) = encode_para_text(&final_text);
        ParaHeaderMut::wrap(&mut sec.records[ph_idx].body)?.set_char_count(wchar_len, last_flag);
        if let Some(csi) = cs_idx {
            let runs = parse_para_char_shapes(&sec.records[csi].body)?;
            let mut kept: Vec<CharShapeRun> = runs
                .iter()
                .copied()
                .filter(|r| r.char_pos < wchar_len)
                .collect();
            if kept.is_empty() || kept[0].char_pos != 0 {
                let first_cs = runs.first().map(|r| r.char_shape_id).unwrap_or(0);
                kept = vec![CharShapeRun {
                    char_pos: 0,
                    char_shape_id: first_cs,
                }];
            }
            let cs_count = u16::try_from(kept.len()).unwrap_or(u16::MAX);
            sec.records[csi].body = serialize_para_char_shapes(&kept);
            ParaHeaderMut::wrap(&mut sec.records[ph_idx].body)?.set_char_shape_count(cs_count);
        }
        // Keep any PARA_RANGE_TAG highlight spans in bounds when text shrinks.
        let mut drop_range_tags = Vec::new();
        let mut kept_rt_count = 0u16;
        for rt_i in (ph_idx + 1)..p_end {
            let rec = &mut sec.records[rt_i];
            if rec.tag != tag::PARA_RANGE_TAG || rec.level != cell_level + 1 || rec.body.len() < 8 {
                continue;
            }
            let start = u32::from_le_bytes(rec.body[0..4].try_into().unwrap());
            let end = u32::from_le_bytes(rec.body[4..8].try_into().unwrap());
            let text_max = wchar_len.saturating_sub(1);
            if start >= text_max {
                drop_range_tags.push(rt_i);
            } else {
                let clamped_end = end.min(text_max);
                if clamped_end <= start {
                    drop_range_tags.push(rt_i);
                } else {
                    rec.body[4..8].copy_from_slice(&clamped_end.to_le_bytes());
                    kept_rt_count = kept_rt_count.saturating_add(1);
                }
            }
        }
        if !drop_range_tags.is_empty() {
            ParaHeaderMut::wrap(&mut sec.records[ph_idx].body)?.set_range_tag_count(kept_rt_count);
            for rt_i in drop_range_tags.into_iter().rev() {
                sec.records.remove(rt_i);
            }
        }
        // Note: `pt_idx` sits right after `PARA_HEADER` (before `PARA_CHAR_SHAPE` / `PARA_RANGE_TAG`),
        // so removing higher-indexed stale range tags leaves `pt_idx` valid.
        if let Some(pti) = pt_idx {
            sec.records[pti].body = pt_bytes;
        } else {
            sec.records.insert(
                ph_idx + 1,
                RawRecord::new(tag::PARA_TEXT, cell_level + 1, pt_bytes),
            );
        }

        // If `fit` requested and cell still looks likely to wrap tightly, apply compact 9pt fallback if larger
        if fit {
            let est_width_mm = (cell.attr.width.saturating_sub(
                (cell.attr.margin_left + cell.attr.margin_right) as u32,
            )) as f64
                / HWPUNIT_PER_MM;
            let char_units: f64 = final_text
                .chars()
                .map(|c| if c.is_ascii() { 0.5 } else { 1.0 })
                .sum();
            if est_width_mm > 2.0 && char_units * 3.5 > est_width_mm {
                let _ = shrink_cell_font(docinfo, sec, target, row, col, 850, 700);
            }
        }
    }

    collapse_trailing_empty_cell_paras(sec, target, row, col)?;
    sec.normalize_last_para_flags()?;
    Ok(())
}

fn fit_value_into_layout(orig: &str, value: &str) -> String {
    let mut best_range: Option<(usize, usize)> = None;
    let mut best_len = 0usize;
    let mut cur_start = None;
    for (idx, ch) in orig.char_indices() {
        if ch == ' ' {
            if cur_start.is_none() {
                cur_start = Some(idx);
            }
        } else if let Some(s) = cur_start.take() {
            let len = orig[s..idx].chars().count();
            if len >= 2 && len > best_len {
                best_len = len;
                best_range = Some((s, idx));
            }
        }
    }
    if let Some(s) = cur_start {
        let len = orig[s..].chars().count();
        if len >= 2 && len > best_len {
            best_len = len;
            best_range = Some((s, orig.len()));
        }
    }

    let val_chars = value.chars().count();
    let Some((start, _)) = best_range else {
        return value.to_string();
    };
    if best_len < val_chars {
        return value.to_string();
    }
    let keep = usize::from(best_len > val_chars);
    let prefix: String = orig[..start].to_string();
    let pad_start = " ".repeat(keep);
    let rest_spaces = " ".repeat(best_len - keep - val_chars);
    let suffix = &orig[start + best_len..];
    format!("{prefix}{pad_start}{value}{rest_spaces}{suffix}")
}

fn collapse_trailing_empty_cell_paras(
    sec: &mut SectionDoc,
    target: &Hwp5TableTarget,
    row: usize,
    col: usize,
) -> Result<()> {
    let tbl = resolve_target_table_in_section(sec, target, "collapse_cell")?;
    let cell = find_cell_by_grid(&tbl, row, col, "collapse_cell")?.clone();
    let cell_level = sec.records[cell.list_header_rec_idx].level;

    let para_starts: Vec<usize> = ((cell.list_header_rec_idx + 1)..cell.end_rec_idx)
        .filter(|&k| sec.records[k].tag == tag::PARA_HEADER && sec.records[k].level == cell_level)
        .collect();
    if para_starts.len() <= 1 {
        return Ok(());
    }

    let mut drop_from_rec: Option<usize> = None;
    let mut dropped_count = 0u16;
    for k in (1..para_starts.len()).rev() {
        let p_start = para_starts[k];
        let p_end = para_starts.get(k + 1).copied().unwrap_or(cell.end_rec_idx);
        let has_content = ((p_start + 1)..p_end).any(|m| {
            sec.records[m].tag == tag::PARA_TEXT || sec.records[m].tag == tag::CTRL_HEADER
        });
        if has_content {
            break;
        }
        drop_from_rec = Some(p_start);
        dropped_count += 1;
    }

    if let Some(from_rec) = drop_from_rec {
        sec.records.drain(from_rec..cell.end_rec_idx);
        let lh = &mut sec.records[cell.list_header_rec_idx].body;
        if lh.len() >= 2 {
            let prev = u16::from_le_bytes(lh[0..2].try_into().unwrap());
            let next = prev.saturating_sub(dropped_count).max(1);
            lh[0..2].copy_from_slice(&next.to_le_bytes());
        }
    }
    Ok(())
}

/// Append a clean `rows x cols` table in pure Rust (zero WASM runtime).
pub fn append_table(
    docinfo: &mut DocInfoDoc,
    sec: &mut SectionDoc,
    headers: &[String],
    rows: &[Vec<String>],
) -> Result<usize> {
    let has_header = !headers.is_empty();
    let data_rows = rows.len();
    let total_rows = data_rows + usize::from(has_header);
    let cols = headers
        .len()
        .max(rows.iter().map(Vec::len).max().unwrap_or(0))
        .max(1);
    let total_rows = total_rows.max(1);

    // Allocate clean 1-solid-black-border BorderFill (+ optional pale header fill).
    let black = 0x0000_0000u32;
    let zero_bf = vec![0u8; 40];
    let grid_bf_body = BorderFillBuilder::merge_borders(
        &zero_bf,
        &[
            BorderSide::Left,
            BorderSide::Right,
            BorderSide::Top,
            BorderSide::Bottom,
        ],
        BorderLineType::Solid as u8,
        1,
        black,
    );
    let grid_bf_id = docinfo.find_or_add_border_fill(grid_bf_body.clone())?;

    let header_bf_id = if has_header {
        let tint = parse_hex_color("#E8EEF5")?;
        let hdr_body = BorderFillBuilder::merge_solid_fill(&grid_bf_body, tint, true);
        docinfo.find_or_add_border_fill(hdr_body)?
    } else {
        grid_bf_id
    };

    let bold_cs_id = if has_header {
        if let Some(base_cs) = docinfo.get_char_shape(0).map(|b| b.to_vec()) {
            let patched = CharShapePatch {
                bold: Some(true),
                ..Default::default()
            }
            .apply_to(&base_cs)?;
            docinfo.find_or_add_char_shape(patched)? as u32
        } else {
            0
        }
    } else {
        0
    };

    let printable_w = sec
        .page_def()?
        .map(|pd| pd.usable_width())
        .filter(|&w| w > 2000)
        .unwrap_or(42520);
    let out_lr = 283u16; // 1mm
    let table_w = printable_w.saturating_sub((out_lr as u32) * 2).max(cols as u32 * 500);
    let base_col_w = table_w / (cols as u32);
    let col_widths: Vec<u32> = (0..cols)
        .map(|c| {
            if c + 1 == cols {
                table_w - base_col_w * ((cols - 1) as u32)
            } else {
                base_col_w
            }
        })
        .collect();
    let row_h = 2835u32; // ~10mm row height
    let table_h = row_h * (total_rows as u32);

    let para_inst = sec.pick_fresh_instance_id();
    let ctrl_inst = para_inst.wrapping_add(1);
    let hdr_len = sec
        .records
        .iter()
        .find(|r| r.tag == tag::PARA_HEADER)
        .map(|r| if r.body.len() >= 24 { 24 } else { 22 })
        .unwrap_or(24);
    let default_ls = sec
        .records
        .iter()
        .find(|r| r.tag == tag::PARA_LINE_SEG && r.body.len() >= 36)
        .map(|r| r.body[..36].to_vec())
        .unwrap_or_else(|| vec![0u8; 36]);

    let mut recs: Vec<RawRecord> = Vec::new();

    // 1. Table-container paragraph (PARA_HEADER L0 + PARA_TEXT L1 + CS L1 + LS L1)
    let mut ph_body = vec![0u8; hdr_len];
    {
        let mut ph = ParaHeaderMut::wrap(&mut ph_body)?;
        ph.set_char_count(9, false); // 8 wchar 'tbl ' ctrl + EOP
        ph.set_control_mask(0x800);
        ph.set_para_shape_id(0);
        ph.set_char_shape_count(1);
        ph.set_line_seg_count(1);
        ph.set_instance_id(para_inst);
    }
    recs.push(RawRecord::new(tag::PARA_HEADER, 0, ph_body));

    let mut pt_body = Vec::with_capacity(18);
    pt_body.extend_from_slice(&0x000Bu16.to_le_bytes());
    pt_body.extend_from_slice(CTRL_TBL_LE);
    pt_body.extend_from_slice(&[0u8; 8]);
    pt_body.extend_from_slice(&0x000Bu16.to_le_bytes());
    pt_body.extend_from_slice(&0x000Du16.to_le_bytes());
    recs.push(RawRecord::new(tag::PARA_TEXT, 1, pt_body));

    recs.push(RawRecord::new(
        tag::PARA_CHAR_SHAPE,
        1,
        serialize_para_char_shapes(&[CharShapeRun {
            char_pos: 0,
            char_shape_id: 0,
        }]),
    ));
    recs.push(RawRecord::new(tag::PARA_LINE_SEG, 1, default_ls.clone()));

    // 2. CTRL_HEADER 'tbl ' (42 bytes)
    let mut ctrl_body = vec![0u8; 42];
    ctrl_body[0..4].copy_from_slice(CTRL_TBL_LE);
    ctrl_body[4..8].copy_from_slice(&0x002A_0310u32.to_le_bytes()); // standard treat-as-char table attr
    ctrl_body[16..20].copy_from_slice(&table_w.to_le_bytes());
    ctrl_body[20..24].copy_from_slice(&table_h.to_le_bytes());
    for m_i in 0..4 {
        ctrl_body[28 + m_i * 2..30 + m_i * 2].copy_from_slice(&out_lr.to_le_bytes());
    }
    ctrl_body[36..40].copy_from_slice(&ctrl_inst.to_le_bytes());
    recs.push(RawRecord::new(tag::CTRL_HEADER, 1, ctrl_body));

    // 3. HWPTAG_TABLE (18 + rows*2 + 4 bytes)
    let mut tbl_body = vec![0u8; 18 + total_rows * 2 + 4];
    tbl_body[0..4].copy_from_slice(&0x0000_0006u32.to_le_bytes()); // pageBreak cell + repeatHeader
    tbl_body[4..6].copy_from_slice(&(total_rows as u16).to_le_bytes());
    tbl_body[6..8].copy_from_slice(&(cols as u16).to_le_bytes());
    // Default inner cell margins: 510 L/R (~1.8mm), 141 T/B (~0.5mm)
    tbl_body[10..12].copy_from_slice(&510u16.to_le_bytes());
    tbl_body[12..14].copy_from_slice(&510u16.to_le_bytes());
    tbl_body[14..16].copy_from_slice(&141u16.to_le_bytes());
    tbl_body[16..18].copy_from_slice(&141u16.to_le_bytes());
    for r in 0..total_rows {
        let off = 18 + r * 2;
        tbl_body[off..off + 2].copy_from_slice(&(cols as u16).to_le_bytes());
    }
    let bf_off = 18 + total_rows * 2;
    tbl_body[bf_off..bf_off + 2].copy_from_slice(&grid_bf_id.to_le_bytes());
    recs.push(RawRecord::new(tag::TABLE, 2, tbl_body));

    // 4. Cells (row-major)
    let mut next_inst = ctrl_inst.wrapping_add(1);
    for r in 0..total_rows {
        let is_hdr = has_header && r == 0;
        let bf_id = if is_hdr { header_bf_id } else { grid_bf_id };
        let cs_id = if is_hdr { bold_cs_id } else { 0 };

        for (c, &col_w) in col_widths.iter().enumerate() {
            let cell_text = if is_hdr {
                headers.get(c).map(String::as_str).unwrap_or("")
            } else {
                let dr = r - usize::from(has_header);
                rows.get(dr)
                    .and_then(|row_vec| row_vec.get(c))
                    .map(String::as_str)
                    .unwrap_or("")
            };

            let mut lh_body = vec![0u8; 34];
            lh_body[0..2].copy_from_slice(&1u16.to_le_bytes()); // nParagraphs = 1
            lh_body[2..6].copy_from_slice(&0x0020_0000u32.to_le_bytes()); // cell attr: valign center (bits 21-22)
            if is_hdr {
                lh_body[6..8].copy_from_slice(&4u16.to_le_bytes()); // header/title cell flag
            }
            lh_body[8..10].copy_from_slice(&(c as u16).to_le_bytes());
            lh_body[10..12].copy_from_slice(&(r as u16).to_le_bytes());
            lh_body[12..14].copy_from_slice(&1u16.to_le_bytes()); // colSpan
            lh_body[14..16].copy_from_slice(&1u16.to_le_bytes()); // rowSpan
            lh_body[16..20].copy_from_slice(&col_w.to_le_bytes());
            lh_body[20..24].copy_from_slice(&row_h.to_le_bytes());
            lh_body[24..26].copy_from_slice(&510u16.to_le_bytes());
            lh_body[26..28].copy_from_slice(&510u16.to_le_bytes());
            lh_body[28..30].copy_from_slice(&141u16.to_le_bytes());
            lh_body[30..32].copy_from_slice(&141u16.to_le_bytes());
            lh_body[32..34].copy_from_slice(&bf_id.to_le_bytes());
            recs.push(RawRecord::new(tag::LIST_HEADER, 2, lh_body));

            let (cell_pt, char_count) = if cell_text.is_empty() {
                (None, 1u32)
            } else {
                let (b, n) = encode_para_text(cell_text);
                (Some(b), n)
            };
            let mut cph_body = vec![0u8; hdr_len];
            {
                let mut cph = ParaHeaderMut::wrap(&mut cph_body)?;
                cph.set_char_count(char_count, true);
                cph.set_para_shape_id(0);
                cph.set_char_shape_count(1);
                cph.set_line_seg_count(1);
                cph.set_instance_id(next_inst);
                next_inst = next_inst.wrapping_add(1);
            }
            recs.push(RawRecord::new(tag::PARA_HEADER, 2, cph_body));
            if let Some(pt) = cell_pt {
                recs.push(RawRecord::new(tag::PARA_TEXT, 3, pt));
            }
            recs.push(RawRecord::new(
                tag::PARA_CHAR_SHAPE,
                3,
                serialize_para_char_shapes(&[CharShapeRun {
                    char_pos: 0,
                    char_shape_id: cs_id,
                }]),
            ));
            recs.push(RawRecord::new(tag::PARA_LINE_SEG, 3, default_ls.clone()));
        }
    }

    let new_table_idx = sec.enumerate_tables()?.len();
    sec.records.extend(recs);
    sec.normalize_last_para_flags()?;
    Ok(new_table_idx)
}

/// Insert a row above or below `row`, optionally filling cells (`cells: Option<&[String]>`).
pub fn insert_table_row(
    docinfo: &mut DocInfoDoc,
    sec: &mut SectionDoc,
    target: &Hwp5TableTarget,
    row: usize,
    position: RowInsertPosition,
    cells_text: Option<&[String]>,
) -> Result<()> {
    let tbl = resolve_target_table_in_section(sec, target, "insert_table_row")?;
    if row >= tbl.rows as usize {
        return Err(Hwp5Error::TargetNotFound(format!(
            "insert_table_row: row {row} out of range (table has {} rows)",
            tbl.rows
        )));
    }
    let insert_row = match position {
        RowInsertPosition::Below => (row + 1) as u16,
        RowInsertPosition::Above => row as u16,
    };

    let mut covered_cols = vec![false; tbl.cols as usize];
    for c in &tbl.cells {
        if c.attr.row < insert_row && c.attr.row + c.attr.row_span > insert_row {
            let lh = &mut sec.records[c.list_header_rec_idx].body;
            lh[14..16].copy_from_slice(&(c.attr.row_span + 1).to_le_bytes());
            for cc in c.attr.col..(c.attr.col + c.attr.col_span).min(tbl.cols) {
                covered_cols[cc as usize] = true;
            }
        }
    }

    let mut new_clusters: Vec<Vec<RawRecord>> = Vec::new();
    for (col_idx, &covered) in covered_cols.iter().enumerate() {
        if covered {
            continue;
        }
        let cloned = clone_blank_cell_cluster(&sec.records, &tbl.cells, insert_row, col_idx as u16)?;
        new_clusters.push(cloned);
    }
    let added_in_row = new_clusters.len() as u16;
    let new_row_height = new_clusters
        .first()
        .and_then(|cl| cl.first())
        .and_then(|lh| TableCellAttr::parse(&lh.body).ok())
        .map(|a| a.height)
        .unwrap_or(0);

    // Grow CTRL_HEADER box height when table dimensions match
    let row0_width: u32 = tbl
        .cells
        .iter()
        .filter(|c| c.attr.row == 0)
        .map(|c| c.attr.width)
        .sum();
    if row0_width > 0 && new_row_height > 0 {
        let cb = &mut sec.records[tbl.ctrl_rec_idx].body;
        if cb.len() >= 24 {
            for off in (8..=cb.len() - 8).step_by(4) {
                if u32::from_le_bytes(cb[off..off + 4].try_into().unwrap()) == row0_width {
                    let h = u32::from_le_bytes(cb[off + 4..off + 8].try_into().unwrap());
                    cb[off + 4..off + 8].copy_from_slice(&(h + new_row_height).to_le_bytes());
                    break;
                }
            }
        }
    }

    // Shift existing rows >= insert_row down by 1
    for c in &tbl.cells {
        if c.attr.row >= insert_row {
            sec.records[c.list_header_rec_idx].body[10..12]
                .copy_from_slice(&(c.attr.row + 1).to_le_bytes());
        }
    }

    // Determine insertion record index before modifying sec.records indices
    let ins_rec_idx = tbl
        .cells
        .iter()
        .find(|c| c.attr.row >= insert_row)
        .map(|c| c.list_header_rec_idx)
        .unwrap_or_else(|| {
            tbl.cells
                .last()
                .map(|c| c.end_rec_idx)
                .unwrap_or(tbl.table_rec_idx + 1)
        });

    // Update HWPTAG_TABLE row count & per-row cell count array
    {
        let tbody = &mut sec.records[tbl.table_rec_idx].body;
        let cut = 18 + (insert_row as usize) * 2;
        tbody.splice(cut..cut, added_in_row.to_le_bytes());
        tbody[4..6].copy_from_slice(&(tbl.rows + 1).to_le_bytes());
    }

    let flat_new: Vec<RawRecord> = new_clusters.into_iter().flatten().collect();
    sec.records.splice(ins_rec_idx..ins_rec_idx, flat_new);
    sec.normalize_last_para_flags()?;

    if let Some(values) = cells_text {
        for (col_idx, text) in values.iter().enumerate() {
            if col_idx < tbl.cols as usize && !covered_cols[col_idx] && !text.is_empty() {
                set_cell_text(docinfo, sec, target, insert_row as usize, col_idx, text, false)?;
            }
        }
    }
    Ok(())
}

/// Delete a row cleanly (`delete_table_row`). Rejects if a rowSpan > 1 cell originates on that row.
pub fn delete_table_row(
    sec: &mut SectionDoc,
    target: &Hwp5TableTarget,
    row: usize,
) -> Result<()> {
    let tbl = resolve_target_table_in_section(sec, target, "delete_table_row")?;
    if row >= tbl.rows as usize {
        return Err(Hwp5Error::TargetNotFound(format!(
            "delete_table_row: row {row} out of range (table has {} rows)",
            tbl.rows
        )));
    }
    if tbl.rows <= 1 {
        return Err(Hwp5Error::InvalidTarget(
            "delete_table_row: cannot delete the last remaining row".to_string(),
        ));
    }
    let del_row = row as u16;
    let mut remove_ranges = Vec::new();
    for c in &tbl.cells {
        if c.attr.row == del_row {
            if c.attr.row_span > 1 {
                return Err(Hwp5Error::InvalidTarget(format!(
                    "delete_table_row: cell ({},{}) has rowSpan {}; unmerge first",
                    c.attr.row, c.attr.col, c.attr.row_span
                )));
            }
            remove_ranges.push((c.list_header_rec_idx, c.end_rec_idx));
        } else if c.attr.row < del_row && c.attr.row + c.attr.row_span > del_row {
            sec.records[c.list_header_rec_idx].body[14..16]
                .copy_from_slice(&(c.attr.row_span - 1).to_le_bytes());
        } else if c.attr.row > del_row {
            sec.records[c.list_header_rec_idx].body[10..12]
                .copy_from_slice(&(c.attr.row - 1).to_le_bytes());
        }
    }

    {
        let tbody = &mut sec.records[tbl.table_rec_idx].body;
        let cut = 18 + (del_row as usize) * 2;
        if cut + 2 <= tbody.len() {
            tbody.drain(cut..cut + 2);
        }
        tbody[4..6].copy_from_slice(&(tbl.rows - 1).to_le_bytes());
    }

    remove_ranges.sort_by_key(|a| std::cmp::Reverse(a.0));
    for (s, e) in remove_ranges {
        sec.records.drain(s..e);
    }
    sec.normalize_last_para_flags()?;
    Ok(())
}

/// Insert a column left or right of `col`, optionally filling cells.
pub fn insert_table_col(
    docinfo: &mut DocInfoDoc,
    sec: &mut SectionDoc,
    target: &Hwp5TableTarget,
    col: usize,
    position: ColInsertPosition,
    cells_text: Option<&[String]>,
) -> Result<()> {
    let tbl = resolve_target_table_in_section(sec, target, "insert_table_col")?;
    if col >= tbl.cols as usize {
        return Err(Hwp5Error::TargetNotFound(format!(
            "insert_table_col: col {col} out of range (table has {} cols)",
            tbl.cols
        )));
    }
    let new_col = match position {
        ColInsertPosition::Right => (col + 1) as u16,
        ColInsertPosition::Left => col as u16,
    };

    let mut covered_rows = vec![false; tbl.rows as usize];
    for c in &tbl.cells {
        if c.attr.col < new_col && c.attr.col + c.attr.col_span > new_col {
            sec.records[c.list_header_rec_idx].body[12..14]
                .copy_from_slice(&(c.attr.col_span + 1).to_le_bytes());
            for rr in c.attr.row..(c.attr.row + c.attr.row_span).min(tbl.rows) {
                covered_rows[rr as usize] = true;
            }
        }
    }
    for c in &tbl.cells {
        if c.attr.col >= new_col {
            sec.records[c.list_header_rec_idx].body[8..10]
                .copy_from_slice(&(c.attr.col + 1).to_le_bytes());
        }
    }

    let mut inserts: Vec<(usize, Vec<RawRecord>, usize)> = Vec::new();
    for (r, &cov) in covered_rows.iter().enumerate() {
        if cov {
            continue;
        }
        let cluster = clone_blank_cell_cluster(&sec.records, &tbl.cells, r as u16, new_col)?;
        let ins_idx = tbl
            .cells
            .iter()
            .find(|c| c.attr.row == r as u16 && c.attr.col >= new_col)
            .map(|c| c.list_header_rec_idx)
            .unwrap_or_else(|| {
                tbl.cells
                    .iter()
                    .filter(|c| c.attr.row == r as u16)
                    .map(|c| c.end_rec_idx)
                    .max()
                    .unwrap_or(tbl.table_rec_idx + 1)
            });
        inserts.push((ins_idx, cluster, r));
    }

    {
        let tbody = &mut sec.records[tbl.table_rec_idx].body;
        tbody[6..8].copy_from_slice(&(tbl.cols + 1).to_le_bytes());
        for &(_, _, r) in &inserts {
            let off = 18 + r * 2;
            let cur = u16::from_le_bytes(tbody[off..off + 2].try_into().unwrap());
            tbody[off..off + 2].copy_from_slice(&(cur + 1).to_le_bytes());
        }
    }

    inserts.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.2.cmp(&a.2)));
    for (idx, cluster, _) in inserts {
        sec.records.splice(idx..idx, cluster);
    }
    sec.normalize_last_para_flags()?;

    if let Some(values) = cells_text {
        for (r, text) in values.iter().enumerate() {
            if r < tbl.rows as usize && !covered_rows[r] && !text.is_empty() {
                set_cell_text(docinfo, sec, target, r, new_col as usize, text, false)?;
            }
        }
    }
    Ok(())
}

/// Delete a column cleanly (`delete_table_col`). Rejects if a colSpan > 1 cell originates on that column.
pub fn delete_table_col(
    sec: &mut SectionDoc,
    target: &Hwp5TableTarget,
    col: usize,
) -> Result<()> {
    let tbl = resolve_target_table_in_section(sec, target, "delete_table_col")?;
    if col >= tbl.cols as usize {
        return Err(Hwp5Error::TargetNotFound(format!(
            "delete_table_col: col {col} out of range (table has {} cols)",
            tbl.cols
        )));
    }
    if tbl.cols <= 1 {
        return Err(Hwp5Error::InvalidTarget(
            "delete_table_col: cannot delete the last remaining column".to_string(),
        ));
    }
    let del_col = col as u16;
    let mut remove_ranges = Vec::new();
    for c in &tbl.cells {
        if c.attr.col == del_col {
            if c.attr.col_span > 1 {
                return Err(Hwp5Error::InvalidTarget(format!(
                    "delete_table_col: cell ({},{}) has colSpan {}; unmerge first",
                    c.attr.row, c.attr.col, c.attr.col_span
                )));
            }
            remove_ranges.push((c.list_header_rec_idx, c.end_rec_idx));
            let rs_off = 18 + (c.attr.row as usize) * 2;
            let tbody = &mut sec.records[tbl.table_rec_idx].body;
            let cur = u16::from_le_bytes(tbody[rs_off..rs_off + 2].try_into().unwrap());
            tbody[rs_off..rs_off + 2].copy_from_slice(&cur.saturating_sub(1).to_le_bytes());
        } else if c.attr.col < del_col && c.attr.col + c.attr.col_span > del_col {
            sec.records[c.list_header_rec_idx].body[12..14]
                .copy_from_slice(&(c.attr.col_span - 1).to_le_bytes());
        } else if c.attr.col > del_col {
            sec.records[c.list_header_rec_idx].body[8..10]
                .copy_from_slice(&(c.attr.col - 1).to_le_bytes());
        }
    }
    sec.records[tbl.table_rec_idx].body[6..8].copy_from_slice(&(tbl.cols - 1).to_le_bytes());

    remove_ranges.sort_by_key(|a| std::cmp::Reverse(a.0));
    for (s, e) in remove_ranges {
        sec.records.drain(s..e);
    }
    sec.normalize_last_para_flags()?;
    Ok(())
}

/// Merge a rectangular range of cells `[from_row..=to_row, from_col..=to_col]`.
pub fn merge_cells(
    sec: &mut SectionDoc,
    target: &Hwp5TableTarget,
    from_row: usize,
    from_col: usize,
    to_row: usize,
    to_col: usize,
) -> Result<()> {
    let r0 = from_row.min(to_row) as u16;
    let r1 = from_row.max(to_row) as u16;
    let c0 = from_col.min(to_col) as u16;
    let c1 = from_col.max(to_col) as u16;
    if r0 == r1 && c0 == c1 {
        return Err(Hwp5Error::InvalidTarget(
            "merge_cells: region must span more than one cell".to_string(),
        ));
    }

    let tbl = resolve_target_table_in_section(sec, target, "merge_cells")?;
    let tl = tbl
        .cells
        .iter()
        .find(|c| c.attr.row == r0 && c.attr.col == c0)
        .ok_or_else(|| {
            Hwp5Error::TargetNotFound(format!("merge_cells: top-left cell ({r0},{c0}) not found"))
        })?;

    sec.records[tl.list_header_rec_idx].body[12..14].copy_from_slice(&(c1 - c0 + 1).to_le_bytes());
    sec.records[tl.list_header_rec_idx].body[14..16].copy_from_slice(&(r1 - r0 + 1).to_le_bytes());

    let mut remove_ranges = Vec::new();
    for cell in &tbl.cells {
        if cell.attr.row >= r0
            && cell.attr.row <= r1
            && cell.attr.col >= c0
            && cell.attr.col <= c1
            && !(cell.attr.row == r0 && cell.attr.col == c0)
        {
            remove_ranges.push((cell.list_header_rec_idx, cell.end_rec_idx));
            let rs_off = 18 + (cell.attr.row as usize) * 2;
            let tbody = &mut sec.records[tbl.table_rec_idx].body;
            let cur = u16::from_le_bytes(tbody[rs_off..rs_off + 2].try_into().unwrap());
            tbody[rs_off..rs_off + 2].copy_from_slice(&cur.saturating_sub(1).to_le_bytes());
        }
    }

    remove_ranges.sort_by_key(|a| std::cmp::Reverse(a.0));
    for (s, e) in remove_ranges {
        sec.records.drain(s..e);
    }
    sec.normalize_last_para_flags()?;
    Ok(())
}

/// Unmerge a merged cell at `(row, col)` back into 1x1 blank cells.
pub fn unmerge_cells(
    sec: &mut SectionDoc,
    target: &Hwp5TableTarget,
    row: usize,
    col: usize,
) -> Result<()> {
    let tbl = resolve_target_table_in_section(sec, target, "unmerge_cells")?;
    let cell = find_cell_by_grid(&tbl, row, col, "unmerge_cells")?.clone();
    let rs = cell.attr.row_span.max(1);
    let cs = cell.attr.col_span.max(1);
    if rs == 1 && cs == 1 {
        return Ok(());
    }
    let base_r = cell.attr.row;
    let base_c = cell.attr.col;

    // Reset top-left span to 1x1
    sec.records[cell.list_header_rec_idx].body[12..14].copy_from_slice(&1u16.to_le_bytes());
    sec.records[cell.list_header_rec_idx].body[14..16].copy_from_slice(&1u16.to_le_bytes());

    let col_widths = compute_col_widths(&tbl);
    let cell_w = col_widths.get(base_c as usize).copied().unwrap_or(cell.attr.width / (cs as u32));
    sec.records[cell.list_header_rec_idx].body[16..20].copy_from_slice(&cell_w.to_le_bytes());

    // Add freed row-counts in TABLE record
    for rr in base_r..(base_r + rs) {
        let added = if rr == base_r { cs - 1 } else { cs };
        let rs_off = 18 + (rr as usize) * 2;
        let tbody = &mut sec.records[tbl.table_rec_idx].body;
        let cur = u16::from_le_bytes(tbody[rs_off..rs_off + 2].try_into().unwrap());
        tbody[rs_off..rs_off + 2].copy_from_slice(&(cur + added).to_le_bytes());
    }

    let template_cluster: Vec<RawRecord> = sec.records[cell.list_header_rec_idx..cell.end_rec_idx].to_vec();
    let mut inserts: Vec<(usize, Vec<RawRecord>, u16, u16)> = Vec::new();
    for rr in base_r..(base_r + rs) {
        for cc in base_c..(base_c + cs) {
            if rr == base_r && cc == base_c {
                continue;
            }
            let mut blank = empty_cell_cluster_from_records(&template_cluster, rr, cc)?;
            if let Some(lh) = blank.first_mut() {
                lh.body[12..14].copy_from_slice(&1u16.to_le_bytes());
                lh.body[14..16].copy_from_slice(&1u16.to_le_bytes());
                if let Some(&w) = col_widths.get(cc as usize) {
                    lh.body[16..20].copy_from_slice(&w.to_le_bytes());
                }
            }
            let ins_idx = tbl
                .cells
                .iter()
                .find(|x| x.attr.row > rr || (x.attr.row == rr && x.attr.col > cc))
                .map(|x| x.list_header_rec_idx)
                .unwrap_or_else(|| {
                    tbl.cells
                        .last()
                        .map(|x| x.end_rec_idx)
                        .unwrap_or(tbl.table_rec_idx + 1)
                });
            inserts.push((ins_idx, blank, rr, cc));
        }
    }

    inserts.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.2.cmp(&a.2)).then_with(|| b.3.cmp(&a.3)));
    for (idx, cluster, _, _) in inserts {
        sec.records.splice(idx..idx, cluster);
    }
    sec.normalize_last_para_flags()?;
    Ok(())
}

/// Split a plain 1x1 cell into an `into_rows x into_cols` sub-grid.
pub fn split_cell(
    sec: &mut SectionDoc,
    target: &Hwp5TableTarget,
    row: usize,
    col: usize,
    into_rows: usize,
    into_cols: usize,
) -> Result<()> {
    let n_rows = into_rows.max(1);
    let n_cols = into_cols.max(1);
    if n_rows < 2 && n_cols < 2 {
        return Err(Hwp5Error::InvalidTarget(
            "split_cell: into_rows or into_cols must be >= 2".to_string(),
        ));
    }
    let tbl = resolve_target_table_in_section(sec, target, "split_cell")?;
    let cell = find_cell_by_grid(&tbl, row, col, "split_cell")?.clone();
    if cell.attr.col_span > 1 || cell.attr.row_span > 1 {
        return Err(Hwp5Error::InvalidTarget(format!(
            "split_cell: cell ({row},{col}) is merged ({}x{}); unmerge first",
            cell.attr.col_span, cell.attr.row_span
        )));
    }
    let r0 = cell.attr.row;
    let c0 = cell.attr.col;
    let extra_rows = (n_rows - 1) as u16;
    let extra_cols = (n_cols - 1) as u16;

    let template: Vec<RawRecord> = sec.records[cell.list_header_rec_idx..cell.end_rec_idx].to_vec();

    // Divide original cell width and height cleanly across the sub-grid.
    let base_w = (cell.attr.width / (n_cols as u32)).max(1);
    let mut sub_w = vec![base_w; n_cols];
    if let Some(last) = sub_w.last_mut() {
        *last = cell
            .attr
            .width
            .saturating_sub(base_w * (n_cols as u32 - 1))
            .max(1);
    }
    let base_h = (cell.attr.height / (n_rows as u32)).max(1);
    let mut sub_h = vec![base_h; n_rows];
    if let Some(last) = sub_h.last_mut() {
        *last = cell
            .attr
            .height
            .saturating_sub(base_h * (n_rows as u32 - 1))
            .max(1);
    }

    // Determine insertion points BEFORE mutating grid rows in place.
    let ins_same_row_idx = cell.end_rec_idx;
    let ins_below_rows_idx = tbl
        .cells
        .iter()
        .find(|c| c.attr.row > r0)
        .map(|c| c.list_header_rec_idx)
        .unwrap_or_else(|| {
            tbl.cells
                .last()
                .map(|c| c.end_rec_idx)
                .unwrap_or(tbl.table_rec_idx + 1)
        });

    // Update target cell dimensions and shift/expand all sibling and spanning cells.
    for c in &tbl.cells {
        let lh = &mut sec.records[c.list_header_rec_idx].body;
        if c.attr.row == r0 && c.attr.col == c0 {
            lh[16..20].copy_from_slice(&sub_w[0].to_le_bytes());
            lh[20..24].copy_from_slice(&sub_h[0].to_le_bytes());
            continue;
        }
        if extra_rows > 0 {
            if c.attr.row > r0 {
                lh[10..12].copy_from_slice(&(c.attr.row + extra_rows).to_le_bytes());
            } else if c.attr.row == r0
                || (c.attr.row < r0 && c.attr.row + c.attr.row_span > r0)
            {
                lh[14..16].copy_from_slice(&(c.attr.row_span + extra_rows).to_le_bytes());
            }
        }
        if extra_cols > 0 {
            if c.attr.col > c0 {
                lh[8..10].copy_from_slice(&(c.attr.col + extra_cols).to_le_bytes());
            } else if c.attr.col == c0
                || (c.attr.col < c0 && c.attr.col + c.attr.col_span > c0)
            {
                lh[12..14].copy_from_slice(&(c.attr.col_span + extra_cols).to_le_bytes());
            }
        }
    }

    // Update HWPTAG_TABLE dimensions and row-cell-count array.
    {
        let tbody = &mut sec.records[tbl.table_rec_idx].body;
        if extra_cols > 0 {
            tbody[6..8].copy_from_slice(&(tbl.cols + extra_cols).to_le_bytes());
            let r0_off = 18 + (r0 as usize) * 2;
            let cur = u16::from_le_bytes(tbody[r0_off..r0_off + 2].try_into().unwrap());
            tbody[r0_off..r0_off + 2].copy_from_slice(&(cur + extra_cols).to_le_bytes());
        }
        if extra_rows > 0 {
            let ins_at = 18 + (r0 as usize + 1) * 2;
            let mut entries = Vec::with_capacity((extra_rows as usize) * 2);
            for _ in 0..extra_rows {
                entries.extend_from_slice(&(n_cols as u16).to_le_bytes());
            }
            tbody.splice(ins_at..ins_at, entries);
            tbody[4..6].copy_from_slice(&(tbl.rows + extra_rows).to_le_bytes());
        }
    }

    // Build blank sub-cells for dr == 0 (right of target cell) and dr in 1..n_rows.
    let mut same_row_cells = Vec::new();
    for (dc, &w) in sub_w.iter().enumerate().skip(1) {
        let mut blank = empty_cell_cluster_from_records(&template, r0, c0 + dc as u16)?;
        if let Some(lh) = blank.first_mut() {
            lh.body[16..20].copy_from_slice(&w.to_le_bytes());
            lh.body[20..24].copy_from_slice(&sub_h[0].to_le_bytes());
        }
        same_row_cells.extend(blank);
    }

    let mut below_rows_cells = Vec::new();
    for (dr, &h) in sub_h.iter().enumerate().skip(1) {
        for (dc, &w) in sub_w.iter().enumerate() {
            let mut blank =
                empty_cell_cluster_from_records(&template, r0 + dr as u16, c0 + dc as u16)?;
            if let Some(lh) = blank.first_mut() {
                lh.body[16..20].copy_from_slice(&w.to_le_bytes());
                lh.body[20..24].copy_from_slice(&h.to_le_bytes());
            }
            below_rows_cells.extend(blank);
        }
    }

    // Apply higher index first (`ins_below_rows_idx >= ins_same_row_idx`) so `ins_same_row_idx`
    // stays valid and row-0 extra cols precede row-(r0+1..) sub-cells even when equal.
    if !below_rows_cells.is_empty() {
        sec.records
            .splice(ins_below_rows_idx..ins_below_rows_idx, below_rows_cells);
    }
    if !same_row_cells.is_empty() {
        sec.records
            .splice(ins_same_row_idx..ins_same_row_idx, same_row_cells);
    }
    sec.normalize_last_para_flags()?;
    Ok(())
}

/// Equalize column widths and/or row heights evenly.
pub fn equalize_table(
    sec: &mut SectionDoc,
    target: &Hwp5TableTarget,
    mode: EqualizeMode,
) -> Result<()> {
    let tbl = resolve_target_table_in_section(sec, target, "equalize_table")?;
    let do_w = matches!(mode, EqualizeMode::Width | EqualizeMode::Both);
    let do_h = matches!(mode, EqualizeMode::Height | EqualizeMode::Both);

    if do_w && tbl.cols > 0 {
        let col_widths = compute_col_widths(&tbl);
        let total_w: u32 = col_widths.iter().sum();
        let avg_w = (total_w / tbl.cols as u32).max(1);
        for c in &tbl.cells {
            let new_w = avg_w * (c.attr.col_span.max(1) as u32);
            sec.records[c.list_header_rec_idx].body[16..20].copy_from_slice(&new_w.to_le_bytes());
        }
    }
    if do_h && tbl.rows > 0 {
        let mut row_heights = vec![0u32; tbl.rows as usize];
        for c in &tbl.cells {
            if c.attr.row_span == 1 && (c.attr.row as usize) < row_heights.len() && row_heights[c.attr.row as usize] == 0 {
                row_heights[c.attr.row as usize] = c.attr.height;
            }
        }
        let non_zero: Vec<u32> = row_heights.into_iter().filter(|&h| h > 0).collect();
        let avg_h = if non_zero.is_empty() {
            2835
        } else {
            (non_zero.iter().sum::<u32>() / non_zero.len() as u32).max(1)
        };
        for c in &tbl.cells {
            let new_h = avg_h * (c.attr.row_span.max(1) as u32);
            sec.records[c.list_header_rec_idx].body[20..24].copy_from_slice(&new_h.to_le_bytes());
        }
    }
    Ok(())
}

/// Set column `col` width to `width_hu` (optionally preserving total table width across an adjacent column).
pub fn set_col_width(
    sec: &mut SectionDoc,
    target: &Hwp5TableTarget,
    col: usize,
    width_hu: i32,
    keep_table_width: bool,
) -> Result<()> {
    let tbl = resolve_target_table_in_section(sec, target, "set_col_width")?;
    let cols = tbl.cols as usize;
    if col >= cols {
        return Err(Hwp5Error::TargetNotFound(format!(
            "set_col_width: col {col} out of range (total cols: {cols})"
        )));
    }
    let mut col_widths = compute_col_widths(&tbl);
    let new_w = width_hu.max(500) as u32;
    let delta = new_w as i64 - col_widths[col] as i64;
    col_widths[col] = new_w;
    if keep_table_width && cols > 1 {
        let adj = if col + 1 < cols { col + 1 } else { col - 1 };
        let min_w = (5.0 * HWPUNIT_PER_MM).round() as i64;
        col_widths[adj] = ((col_widths[adj] as i64 - delta).max(min_w)) as u32;
    }

    for c in &tbl.cells {
        let end = (c.attr.col + c.attr.col_span).min(tbl.cols) as usize;
        let cell_w: u32 = col_widths[c.attr.col as usize..end].iter().sum();
        sec.records[c.list_header_rec_idx].body[16..20].copy_from_slice(&cell_w.to_le_bytes());
    }
    let total_w: u32 = col_widths.iter().sum();
    if sec.records[tbl.ctrl_rec_idx].body.len() >= 20 {
        sec.records[tbl.ctrl_rec_idx].body[16..20].copy_from_slice(&total_w.to_le_bytes());
    }
    Ok(())
}

/// Set row `row` height to `height_hu`.
pub fn set_row_height(
    sec: &mut SectionDoc,
    target: &Hwp5TableTarget,
    row: usize,
    height_hu: i32,
) -> Result<()> {
    let tbl = resolve_target_table_in_section(sec, target, "set_row_height")?;
    if row >= tbl.rows as usize {
        return Err(Hwp5Error::TargetNotFound(format!(
            "set_row_height: row {row} out of range (total rows: {})",
            tbl.rows
        )));
    }
    let r = row as u16;
    let new_h = height_hu.max(100) as u32;
    let old_h = tbl
        .cells
        .iter()
        .find(|c| c.attr.row == r && c.attr.row_span == 1)
        .map(|c| c.attr.height)
        .or_else(|| {
            tbl.cells
                .iter()
                .find(|c| c.attr.row <= r && c.attr.row + c.attr.row_span > r)
                .map(|c| c.attr.height / c.attr.row_span.max(1) as u32)
        })
        .unwrap_or(new_h);
    let delta_h = new_h as i64 - old_h as i64;

    for c in &tbl.cells {
        if c.attr.row == r && c.attr.row_span == 1 {
            sec.records[c.list_header_rec_idx].body[20..24].copy_from_slice(&new_h.to_le_bytes());
        } else if c.attr.row <= r && c.attr.row + c.attr.row_span > r && c.attr.row_span > 1 {
            let cur_h = c.attr.height as i64;
            let updated = (cur_h + delta_h).max(1) as u32;
            sec.records[c.list_header_rec_idx].body[20..24].copy_from_slice(&updated.to_le_bytes());
        }
    }
    if sec.records[tbl.ctrl_rec_idx].body.len() >= 24 {
        let cur_tbl_h =
            u32::from_le_bytes(sec.records[tbl.ctrl_rec_idx].body[20..24].try_into().unwrap()) as i64;
        let updated = (cur_tbl_h + delta_h).max(1) as u32;
        sec.records[tbl.ctrl_rec_idx].body[20..24].copy_from_slice(&updated.to_le_bytes());
    }
    Ok(())
}

/// Proportionally scale all columns of a table to fit the usable page width (or `target_width_hu`).
pub fn fit_table_to_page(
    sec: &mut SectionDoc,
    target: &Hwp5TableTarget,
    target_width_hu: Option<i32>,
) -> Result<()> {
    let printable_w = sec
        .page_def()?
        .ok_or_else(|| Hwp5Error::MalformedRecord("fit_table_to_page: PAGE_DEF missing".to_string()))?
        .usable_width();
    let tbl = resolve_target_table_in_section(sec, target, "fit_table_to_page")?;
    let cols = tbl.cols as usize;
    if cols == 0 {
        return Ok(());
    }

    let ctrl_body = &sec.records[tbl.ctrl_rec_idx].body;
    let outer_lr = if ctrl_body.len() >= 32 {
        u16::from_le_bytes(ctrl_body[28..30].try_into().unwrap()) as u32
            + u16::from_le_bytes(ctrl_body[30..32].try_into().unwrap()) as u32
    } else {
        0
    };
    let target_w = target_width_hu
        .map(|v| v.max(cols as i32 * 200) as u32)
        .unwrap_or_else(|| printable_w.saturating_sub(outer_lr).max(cols as u32 * 200));

    let old_widths = compute_col_widths(&tbl);
    let current_total: u32 = old_widths.iter().sum();
    let scale = if current_total > 0 {
        target_w as f64 / current_total as f64
    } else {
        1.0
    };
    let mut new_widths: Vec<u32> = old_widths
        .iter()
        .map(|&w| ((w as f64) * scale).floor().max(1.0) as u32)
        .collect();
    let sum_so_far: u32 = new_widths.iter().sum();
    if let Some(last) = new_widths.last_mut() {
        *last = (*last as i64 + target_w as i64 - sum_so_far as i64).max(1) as u32;
    }

    for c in &tbl.cells {
        let end = (c.attr.col + c.attr.col_span).min(tbl.cols) as usize;
        let cell_w: u32 = new_widths[c.attr.col as usize..end].iter().sum();
        sec.records[c.list_header_rec_idx].body[16..20].copy_from_slice(&cell_w.to_le_bytes());
    }
    if sec.records[tbl.ctrl_rec_idx].body.len() >= 20 {
        sec.records[tbl.ctrl_rec_idx].body[16..20].copy_from_slice(&target_w.to_le_bytes());
    }
    Ok(())
}

/// Set cell inner padding (`margin_left/right/top/bottom` at `LIST_HEADER` offsets 24..31).
#[allow(clippy::too_many_arguments)]
pub fn set_cell_padding(
    sec: &mut SectionDoc,
    target: &Hwp5TableTarget,
    row: usize,
    col: usize,
    left_hu: Option<i32>,
    right_hu: Option<i32>,
    top_hu: Option<i32>,
    bottom_hu: Option<i32>,
) -> Result<()> {
    let tbl = resolve_target_table_in_section(sec, target, "set_cell_padding")?;
    let cell = find_cell_by_grid(&tbl, row, col, "set_cell_padding")?;
    let lh = &mut sec.records[cell.list_header_rec_idx].body;
    let default_m = (1.41 * HWPUNIT_PER_MM).round() as u16;
    let vals = [
        left_hu.map(|v| v.clamp(0, u16::MAX as i32) as u16).unwrap_or(default_m),
        right_hu.map(|v| v.clamp(0, u16::MAX as i32) as u16).unwrap_or(default_m),
        top_hu.map(|v| v.clamp(0, u16::MAX as i32) as u16).unwrap_or(default_m),
        bottom_hu.map(|v| v.clamp(0, u16::MAX as i32) as u16).unwrap_or(default_m),
    ];
    for (i, val) in vals.iter().enumerate() {
        lh[24 + i * 2..26 + i * 2].copy_from_slice(&val.to_le_bytes());
    }
    Ok(())
}

/// Set cell background color while preserving borders and diagonal in `BorderFill`.
pub fn set_cell_background(
    docinfo: &mut DocInfoDoc,
    sec: &mut SectionDoc,
    target: &Hwp5TableTarget,
    row: usize,
    col: usize,
    color_hex: &str,
) -> Result<()> {
    let tbl = resolve_target_table_in_section(sec, target, "set_cell_background")?;
    let cell = find_cell_by_grid(&tbl, row, col, "set_cell_background")?;
    let base_bf = docinfo
        .get_border_fill(cell.attr.border_fill_id)
        .unwrap_or(&[])
        .to_vec();
    let bgr = parse_hex_color(color_hex)?;
    let new_body = BorderFillBuilder::merge_solid_fill(&base_bf, bgr, true);
    let new_bf_id = docinfo.find_or_add_border_fill(new_body)?;
    sec.records[cell.list_header_rec_idx].body[32..34].copy_from_slice(&new_bf_id.to_le_bytes());
    Ok(())
}

/// Set horizontal and/or vertical alignment on a cell or `(row, col)..=(end_row, end_col)` range.
#[allow(clippy::too_many_arguments)]
pub fn set_cell_align(
    docinfo: &mut DocInfoDoc,
    sec: &mut SectionDoc,
    target: &Hwp5TableTarget,
    row: usize,
    col: usize,
    end_row: Option<usize>,
    end_col: Option<usize>,
    horizontal: Option<CellHAlign>,
    vertical: Option<CellVAlign>,
) -> Result<()> {
    let tbl = resolve_target_table_in_section(sec, target, "set_cell_align")?;
    let r1 = end_row.unwrap_or(row);
    let c1 = end_col.unwrap_or(col);
    let r_lo = row.min(r1) as u16;
    let r_hi = row.max(r1) as u16;
    let c_lo = col.min(c1) as u16;
    let c_hi = col.max(c1) as u16;

    let ps_id = match horizontal {
        Some(h) => {
            let align = match h {
                CellHAlign::Left => ParaAlignment::Left,
                CellHAlign::Center => ParaAlignment::Center,
                CellHAlign::Right => ParaAlignment::Right,
                CellHAlign::Justify => ParaAlignment::Justify,
            };
            Some(ensure_aligned_para_shape(docinfo, align)?)
        }
        None => None,
    };

    let valign_bits = vertical.map(|v| match v {
        CellVAlign::Top => 0u32,
        CellVAlign::Center | CellVAlign::Middle => 1u32,
        CellVAlign::Bottom => 2u32,
    });

    let mut matched = false;
    for cell in &tbl.cells {
        if cell.attr.row < r_lo || cell.attr.row > r_hi || cell.attr.col < c_lo || cell.attr.col > c_hi {
            continue;
        }
        matched = true;
        if let Some(v) = valign_bits {
            let lh = &mut sec.records[cell.list_header_rec_idx].body;
            let cur_attr = u32::from_le_bytes(lh[2..6].try_into().unwrap());
            let new_attr = (cur_attr & !(0x03 << 21)) | (v << 21);
            lh[2..6].copy_from_slice(&new_attr.to_le_bytes());
        }
        if let Some(new_ps) = ps_id {
            let cell_lvl = sec.records[cell.list_header_rec_idx].level;
            for k in (cell.list_header_rec_idx + 1)..cell.end_rec_idx {
                if sec.records[k].tag == tag::PARA_HEADER && sec.records[k].level == cell_lvl {
                    ParaHeaderMut::wrap(&mut sec.records[k].body)?.set_para_shape_id(new_ps);
                }
            }
        }
    }

    if !matched {
        return Err(Hwp5Error::TargetNotFound(format!(
            "set_cell_align: no cells in range ({r_lo},{c_lo})..=({r_hi},{c_hi})"
        )));
    }
    Ok(())
}

/// Set cell border side(s) while preserving fill and diagonal attributes.
#[allow(clippy::too_many_arguments)]
pub fn set_cell_border(
    docinfo: &mut DocInfoDoc,
    sec: &mut SectionDoc,
    target: &Hwp5TableTarget,
    row: usize,
    col: usize,
    sides: &[TypesBorderSide],
    color: Option<&str>,
    width_hu: Option<i32>,
    line_type: Option<TypesBorderLine>,
) -> Result<()> {
    let tbl = resolve_target_table_in_section(sec, target, "set_cell_border")?;
    let cell = find_cell_by_grid(&tbl, row, col, "set_cell_border")?;
    let base_bf = docinfo
        .get_border_fill(cell.attr.border_fill_id)
        .unwrap_or(&[])
        .to_vec();
    let bgr = parse_hex_color(color.unwrap_or("#000000"))?;
    let lt = match line_type.unwrap_or(TypesBorderLine::Solid) {
        TypesBorderLine::Solid => BorderLineType::Solid as u8,
        TypesBorderLine::Dashed => BorderLineType::Dash as u8,
        TypesBorderLine::Dotted => BorderLineType::Dot as u8,
        TypesBorderLine::Double => BorderLineType::Double as u8,
    };
    let w = width_hu
        .map(|hu| ((hu as f64 / HWPUNIT_PER_MM).round() as i32).clamp(1, 255) as u8)
        .unwrap_or(1);
    let mapped_sides: Vec<BorderSide> = sides
        .iter()
        .map(|s| match s {
            TypesBorderSide::Left => BorderSide::Left,
            TypesBorderSide::Right => BorderSide::Right,
            TypesBorderSide::Top => BorderSide::Top,
            TypesBorderSide::Bottom => BorderSide::Bottom,
        })
        .collect();

    let new_body = BorderFillBuilder::merge_borders(&base_bf, &mapped_sides, lt, w, bgr);
    let new_bf_id = docinfo.find_or_add_border_fill(new_body)?;
    sec.records[cell.list_header_rec_idx].body[32..34].copy_from_slice(&new_bf_id.to_le_bytes());
    Ok(())
}

/// Shrink font size inside every paragraph of cell `(row, col)` to `max(size_hu, min_hu)`.
pub fn shrink_cell_font(
    docinfo: &mut DocInfoDoc,
    sec: &mut SectionDoc,
    target: &Hwp5TableTarget,
    row: usize,
    col: usize,
    size_hu: i32,
    min_hu: i32,
) -> Result<()> {
    let tbl = resolve_target_table_in_section(sec, target, "shrink_cell_font")?;
    let cell = find_cell_by_grid(&tbl, row, col, "shrink_cell_font")?.clone();
    let eff_hu = size_hu.max(min_hu).max(400);
    let target_pt = (eff_hu as f64) / 100.0;
    let cell_lvl = sec.records[cell.list_header_rec_idx].level;

    for k in (cell.list_header_rec_idx + 1)..cell.end_rec_idx {
        if sec.records[k].tag != tag::PARA_HEADER || sec.records[k].level != cell_lvl {
            continue;
        }
        let p_end = ((k + 1)..cell.end_rec_idx)
            .find(|&m| sec.records[m].level <= cell_lvl)
            .unwrap_or(cell.end_rec_idx);
        let Some(cs_idx) = ((k + 1)..p_end).find(|&m| {
            sec.records[m].tag == tag::PARA_CHAR_SHAPE && sec.records[m].level == cell_lvl + 1
        }) else {
            continue;
        };
        let runs = parse_para_char_shapes(&sec.records[cs_idx].body)?;
        if runs.is_empty() {
            continue;
        }
        let mut updated = runs.clone();
        for run in &mut updated {
            if let Some(base_cs) = docinfo.get_char_shape(run.char_shape_id as u16).map(|b| b.to_vec()) {
                let patched = CharShapePatch {
                    size_pt: Some(target_pt),
                    ..Default::default()
                }
                .apply_to(&base_cs)?;
                run.char_shape_id = docinfo.find_or_add_char_shape(patched)? as u32;
            }
        }
        let merged = if let Some(first) = updated.first() {
            if updated.iter().all(|r| r.char_shape_id == first.char_shape_id) {
                vec![CharShapeRun {
                    char_pos: 0,
                    char_shape_id: first.char_shape_id,
                }]
            } else {
                updated
            }
        } else {
            updated
        };
        let run_count = u16::try_from(merged.len()).unwrap_or(u16::MAX);
        sec.records[cs_idx].body = serialize_para_char_shapes(&merged);
        ParaHeaderMut::wrap(&mut sec.records[k].body)?.set_char_shape_count(run_count);
    }
    Ok(())
}

fn compute_col_widths(tbl: &crate::section::SectionTableInfo) -> Vec<u32> {
    let cols = tbl.cols as usize;
    let mut widths = vec![0u32; cols];
    for c in &tbl.cells {
        if c.attr.col_span == 1 && (c.attr.col as usize) < cols && widths[c.attr.col as usize] == 0 {
            widths[c.attr.col as usize] = c.attr.width;
        }
    }
    for (j, w) in widths.iter_mut().enumerate() {
        if *w == 0 {
            if let Some(c) = tbl
                .cells
                .iter()
                .find(|cell| (cell.attr.col as usize) <= j && (cell.attr.col + cell.attr.col_span) as usize > j)
            {
                *w = c.attr.width / c.attr.col_span.max(1) as u32;
            } else {
                *w = (20.0 * HWPUNIT_PER_MM).round() as u32;
            }
        }
    }
    widths
}

fn clone_blank_cell_cluster(
    records: &[RawRecord],
    cells: &[crate::section::SectionTableCellInfo],
    new_row: u16,
    new_col: u16,
) -> Result<Vec<RawRecord>> {
    let mut candidates: Vec<&crate::section::SectionTableCellInfo> = cells
        .iter()
        .filter(|c| c.attr.col_span == 1 && c.attr.row_span == 1 && c.attr.col == new_col)
        .collect();
    if candidates.is_empty() {
        candidates = cells
            .iter()
            .filter(|c| c.attr.col_span == 1 && c.attr.row_span == 1)
            .collect();
    }
    let src = candidates
        .into_iter()
        .max_by_key(|c| c.attr.width)
        .ok_or_else(|| {
            Hwp5Error::InvalidTarget("table has no plain 1x1 cell to clone".to_string())
        })?;
    let slice = &records[src.list_header_rec_idx..src.end_rec_idx];
    empty_cell_cluster_from_records(slice, new_row, new_col)
}

fn empty_cell_cluster_from_records(
    cluster: &[RawRecord],
    new_row: u16,
    new_col: u16,
) -> Result<Vec<RawRecord>> {
    let Some(lh) = cluster.first().filter(|r| r.tag == tag::LIST_HEADER && r.level == 2) else {
        return Err(Hwp5Error::MalformedRecord("cell cluster missing LIST_HEADER".to_string()));
    };
    let cell_lvl = lh.level;
    let ph_idx = cluster
        .iter()
        .enumerate()
        .skip(1)
        .find(|(_, r)| r.tag == tag::PARA_HEADER && r.level == cell_lvl)
        .map(|(idx, _)| idx)
        .ok_or_else(|| Hwp5Error::MalformedRecord("cell cluster missing PARA_HEADER".to_string()))?;
    let p_end = ((ph_idx + 1)..cluster.len())
        .find(|&k| cluster[k].tag == tag::PARA_HEADER && cluster[k].level == cell_lvl)
        .unwrap_or(cluster.len());

    let mut new_lh = lh.clone();
    new_lh.body[0..2].copy_from_slice(&1u16.to_le_bytes()); // nParagraphs = 1
    new_lh.body[8..10].copy_from_slice(&new_col.to_le_bytes());
    new_lh.body[10..12].copy_from_slice(&new_row.to_le_bytes());

    let mut out = vec![new_lh];
    for rec in &cluster[ph_idx..p_end] {
        if rec.tag == tag::PARA_TEXT {
            continue;
        }
        let mut r = rec.clone();
        if r.tag == tag::PARA_HEADER {
            let mut ph = ParaHeaderMut::wrap(&mut r.body)?;
            ph.set_char_count(1, true);
            ph.set_char_shape_count(1);
            ph.set_line_seg_count(1);
        } else if r.tag == tag::PARA_CHAR_SHAPE && r.body.len() > 8 {
            r.body.truncate(8);
        } else if r.tag == tag::PARA_LINE_SEG && r.body.len() > 36 {
            r.body.truncate(36);
        }
        out.push(r);
    }
    Ok(out)
}
