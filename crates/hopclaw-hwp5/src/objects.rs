use std::path::Path;

use hopclaw_types::{Hwp5CellImageTarget, Hwp5TableTarget, SealMode, HWPUNIT_PER_MM};

use crate::cfb_container::{deflate_raw_level9, Hwp5Container};
use crate::docinfo::DocInfoDoc;
use crate::error::{Hwp5Error, Result};
use crate::record::{tag, RawRecord};
use crate::section::{ParaHeaderMut, SectionDoc};
use crate::style::find_text_range;
use crate::table::resolve_target_table_in_section;
use crate::text::ensure_aligned_para_shape;
use crate::docinfo::ParaAlignment;

const IMG_GSO_CTRL_BYTES: &[u8] = &[
    0x20, 0x6f, 0x73, 0x67, 0x11, 0x23, 0x0a, 0x14, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x70, 0x17, 0x00, 0x00, 0xa6, 0x0e, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x65, 0x96, 0x9a, 0x42, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

const IMG_PIC_COMP_BYTES: &[u8] = &[
    0x63, 0x69, 0x70, 0x24, 0x63, 0x69, 0x70, 0x24, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x01, 0x00, 0x70, 0x17, 0x00, 0x00, 0xa6, 0x0e, 0x00, 0x00, 0x70, 0x17, 0x00, 0x00,
    0xa6, 0x0e, 0x00, 0x00, 0x00, 0x00, 0x0b, 0x20, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xf0, 0x3f, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xf0, 0x3f, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xf0, 0x3f, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xf0, 0x3f, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xf0, 0x3f, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xf0, 0x3f, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00,
];

const IMG_PICTURE_DATA_BYTES: &[u8] = &[
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x70, 0x17, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x70, 0x17, 0x00, 0x00,
    0xa6, 0x0e, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xa6, 0x0e, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x70, 0x17, 0x00, 0x00, 0xc4, 0x0e, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x66, 0x96, 0x9a, 0x02, 0x00, 0x00,
    0x00, 0x00, 0x70, 0x17, 0x00, 0x00, 0xc4, 0x0e, 0x00, 0x00, 0x00,
];

const GSO_INLINE_CHAR: &[u8] = &[
    0x0b, 0x00, 0x20, 0x6f, 0x73, 0x67, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0b, 0x00,
];
const GSO_INLINE_WITH_EOP: &[u8] = &[
    0x0b, 0x00, 0x20, 0x6f, 0x73, 0x67, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0b, 0x00,
    0x0d, 0x00,
];

const TABLE_WRAP_MASK: u32 = 0x0060_0001;
const WRAP_INLINE: u32 = 0x0000_0001;
const WRAP_FRONT: u32 = 0x0060_0000;
const CELL_OBJ_VMARGIN_HU: u16 = 283;

/// Image wrap/frame specification for GSO `$pic` insertion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GsoPlacement {
    Inline,
    Floating {
        pos_x_hu: i32,
        pos_y_hu: i32,
    },
}

/// Insert an image from disk into `/BinData/BIN000N.<ext>`, DocInfo `BIN_DATA`, and `Section0`.
#[allow(clippy::too_many_arguments)]
pub fn insert_image(
    container: &mut Hwp5Container,
    docinfo: &mut DocInfoDoc,
    sec: &mut SectionDoc,
    image_path: &str,
    anchor_text: Option<&str>,
    width_hu: Option<i32>,
    height_hu: Option<i32>,
    cell: Option<&Hwp5CellImageTarget>,
    placement: GsoPlacement,
) -> Result<()> {
    let img_bytes = std::fs::read(image_path).map_err(|err| {
        Hwp5Error::InvalidTarget(format!("insert_image: failed to read '{image_path}': {err}"))
    })?;
    let ext = Path::new(image_path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("png")
        .to_ascii_lowercase();

    let (w_hu, h_hu) = resolve_image_size_hu(&img_bytes, width_hu, height_hu);

    // 1. Add /BinData/BIN000N.<ext> stream (raw-deflate compressed when FileHeader compressed)
    let storage_id = next_bindata_storage_id(container);
    let stream_path = format!("/BinData/BIN{storage_id:04X}.{ext}");
    let stream_payload = if container.file_header().is_compressed() {
        deflate_raw_level9(&img_bytes)?
    } else {
        img_bytes
    };
    container.write_raw_stream(&stream_path, &stream_payload)?;

    // 2. Append DocInfo BIN_DATA (0x12) & resolve aligned ParaShape if dropping into a table cell
    let bin_data_id = docinfo.append_embedded_bin_data(storage_id, &ext)?;
    let center_ps_id = if cell.is_some() {
        ensure_aligned_para_shape(docinfo, ParaAlignment::Center)?
    } else {
        0
    };

    // 3. Build 6-record `$pic` cluster
    let para_inst = sec.pick_fresh_instance_id();
    let gso_inst = para_inst.wrapping_add(0x100);
    let mut cluster = build_pic_cluster(bin_data_id, para_inst, gso_inst, w_hu, h_hu, placement)?;

    if let Some(target_cell) = cell {
        splice_pic_into_cell(sec, &target_cell.table, target_cell.row, target_cell.col, &mut cluster, center_ps_id)?;
        return Ok(());
    }

    if let GsoPlacement::Floating { .. } = placement {
        if let Some(anchor) = anchor_text.filter(|s| !s.is_empty()) {
            if let Ok(hit) = find_text_range(sec, anchor) {
                let ph_level = sec.records[hit.para_header_idx].level;
                let mut gso_part = cluster.split_off(3); // [CTRL_HEADER, SHAPE_COMPONENT, PICTURE]
                if ph_level > 0 {
                    for r in &mut gso_part {
                        r.level += ph_level;
                    }
                }
                let pt_body = &mut sec.records[hit.para_text_idx].body;
                let ins_byte = if pt_body.len() >= 2
                    && pt_body[pt_body.len() - 2..] == [0x0d, 0x00]
                {
                    pt_body.len() - 2
                } else {
                    pt_body.len()
                };
                pt_body.splice(ins_byte..ins_byte, GSO_INLINE_CHAR.iter().copied());

                {
                    let mut ph = ParaHeaderMut::wrap(&mut sec.records[hit.para_header_idx].body)?;
                    let cc = ph.char_count() + 8;
                    let last = ph.is_last_in_list();
                    ph.set_char_count(cc, last);
                    ph.set_control_mask(ph.control_mask() | 0x800);
                }

                let para_end = ((hit.para_header_idx + 1)..sec.records.len())
                    .find(|&k| sec.records[k].level <= ph_level)
                    .unwrap_or(sec.records.len());
                sec.records.splice(para_end..para_end, gso_part);
                return Ok(());
            }
        }
    }

    // Inline (or floating fallback): insert as a new top-level paragraph after anchor (or at end)
    let insert_at = if let Some(anchor) = anchor_text.filter(|s| !s.is_empty()) {
        if let Ok(hit) = find_text_range(sec, anchor) {
            let ph_level = sec.records[hit.para_header_idx].level;
            if ph_level == 0 {
                ((hit.para_header_idx + 1)..sec.records.len())
                    .find(|&k| sec.records[k].level == 0)
                    .unwrap_or(sec.records.len())
            } else {
                sec.records.len()
            }
        } else {
            sec.records.len()
        }
    } else {
        sec.records.len()
    };

    sec.records.splice(insert_at..insert_at, cluster);
    sec.normalize_last_para_flags()?;
    Ok(())
}

/// Place a stamp/seal PNG onto an anchor phrase using metric glyph-advance placement.
pub fn place_seal(
    container: &mut Hwp5Container,
    docinfo: &mut DocInfoDoc,
    sec: &mut SectionDoc,
    anchor_text: &str,
    seal_image_path: &str,
    mode: SealMode,
) -> Result<()> {
    let hit = find_text_range(sec, anchor_text)?;
    let font_pt = 10.0f64;
    let em_mm = (font_pt * 25.4) / 72.0;
    let size_mm = (em_mm * 1.6).clamp(7.0, 18.0);
    let size_hu = (size_mm * HWPUNIT_PER_MM).round() as i32;

    let pt_body = &sec.records[hit.para_text_idx].body;
    let start_x_mm = measure_para_prefix_mm(pt_body, (hit.wchar_start as usize) * 2, em_mm);
    let anchor_w_mm: f64 = anchor_text.chars().map(|ch| glyph_em(ch as u32) * em_mm).sum();
    let eff_mode = match mode {
        SealMode::Auto => {
            if 150.0 - (start_x_mm + anchor_w_mm) >= size_mm + 2.0 {
                SealMode::Right
            } else {
                SealMode::Overlap
            }
        }
        other => other,
    };

    let pos_x_mm = match eff_mode {
        SealMode::Right => start_x_mm + anchor_w_mm + 2.0,
        _ => start_x_mm + anchor_w_mm / 2.0 - size_mm / 2.0,
    };
    let pos_y_mm = -(size_mm - em_mm) / 2.0;
    let pos_x_hu = (pos_x_mm * HWPUNIT_PER_MM).round() as i32;
    let pos_y_hu = (pos_y_mm * HWPUNIT_PER_MM).round() as i32;

    insert_image(
        container,
        docinfo,
        sec,
        seal_image_path,
        Some(anchor_text),
        Some(size_hu),
        Some(size_hu),
        None,
        GsoPlacement::Floating { pos_x_hu, pos_y_hu },
    )
}

fn splice_pic_into_cell(
    sec: &mut SectionDoc,
    table_target: &Hwp5TableTarget,
    row: usize,
    col: usize,
    cluster: &mut [RawRecord],
    center_ps_id: u16,
) -> Result<()> {
    let tbl = resolve_target_table_in_section(sec, table_target, "insert_image")?;
    let r = row as u16;
    let c = col as u16;
    let cell = tbl
        .cells
        .iter()
        .find(|x| x.attr.row == r && x.attr.col == c)
        .ok_or_else(|| {
            Hwp5Error::TargetNotFound(format!("insert_image: cell ({row},{col}) not found"))
        })?;

    // Re-level body cluster (+2) and set centered ParaShape + vertical margin
    for rec in cluster.iter_mut() {
        rec.level += 2;
        if rec.tag == tag::PARA_HEADER {
            ParaHeaderMut::wrap(&mut rec.body)?.set_para_shape_id(center_ps_id);
        } else if rec.tag == tag::CTRL_HEADER && rec.body.len() >= 36 {
            rec.body[32..34].copy_from_slice(&CELL_OBJ_VMARGIN_HU.to_le_bytes());
            rec.body[34..36].copy_from_slice(&CELL_OBJ_VMARGIN_HU.to_le_bytes());
        }
    }

    let cell_lvl = sec.records[cell.list_header_rec_idx].level;
    let end_rec = cell.end_rec_idx;
    let lh_idx = cell.list_header_rec_idx;
    let old_n_paras = u16::from_le_bytes(sec.records[lh_idx].body[0..2].try_into().unwrap());
    sec.records[lh_idx].body[0..2].copy_from_slice(&(old_n_paras + 1).to_le_bytes());

    // Clear last_in_list on prior last paragraph of cell before appending
    for k in (lh_idx + 1)..end_rec {
        if sec.records[k].tag == tag::PARA_HEADER && sec.records[k].level == cell_lvl {
            if let Ok(mut ph) = ParaHeaderMut::wrap(&mut sec.records[k].body) {
                if ph.is_last_in_list() {
                    ph.set_last_in_list(false);
                }
            }
        }
    }
    if let Some(first) = cluster.first_mut() {
        if let Ok(mut ph) = ParaHeaderMut::wrap(&mut first.body) {
            ph.set_last_in_list(true);
        }
    }
    sec.records.splice(end_rec..end_rec, cluster.iter().cloned());
    sec.normalize_last_para_flags()?;
    Ok(())
}

fn build_pic_cluster(
    bin_data_id: u16,
    para_inst: u32,
    gso_inst: u32,
    w_hu: i32,
    h_hu: i32,
    placement: GsoPlacement,
) -> Result<Vec<RawRecord>> {
    let mut ph_body = vec![0u8; 24];
    {
        let mut ph = ParaHeaderMut::wrap(&mut ph_body)?;
        ph.set_char_count(9, false);
        ph.set_control_mask(0x800);
        ph.set_para_shape_id(0);
        ph.set_char_shape_count(1);
        ph.set_instance_id(para_inst);
    }

    let mut ch = IMG_GSO_CTRL_BYTES.to_vec();
    let mut attr = u32::from_le_bytes(ch[4..8].try_into().unwrap()) & !TABLE_WRAP_MASK;
    match placement {
        GsoPlacement::Inline => attr |= WRAP_INLINE,
        GsoPlacement::Floating { pos_x_hu, pos_y_hu } => {
            attr |= WRAP_FRONT;
            ch[8..12].copy_from_slice(&(pos_y_hu as u32).to_le_bytes());
            ch[12..16].copy_from_slice(&(pos_x_hu as u32).to_le_bytes());
        }
    }
    ch[4..8].copy_from_slice(&attr.to_le_bytes());
    ch[16..20].copy_from_slice(&(w_hu.max(1) as u32).to_le_bytes());
    ch[20..24].copy_from_slice(&(h_hu.max(1) as u32).to_le_bytes());
    ch[36..40].copy_from_slice(&gso_inst.to_le_bytes());

    let mut comp = IMG_PIC_COMP_BYTES.to_vec();
    comp[20..24].copy_from_slice(&(w_hu.max(1) as u32).to_le_bytes());
    comp[28..32].copy_from_slice(&(w_hu.max(1) as u32).to_le_bytes());
    comp[24..28].copy_from_slice(&(h_hu.max(1) as u32).to_le_bytes());
    comp[32..36].copy_from_slice(&(h_hu.max(1) as u32).to_le_bytes());

    let mut pic = IMG_PICTURE_DATA_BYTES.to_vec();
    pic[71..73].copy_from_slice(&bin_data_id.to_le_bytes());

    Ok(vec![
        RawRecord::new(tag::PARA_HEADER, 0, ph_body),
        RawRecord::new(tag::PARA_TEXT, 1, GSO_INLINE_WITH_EOP.to_vec()),
        RawRecord::new(tag::PARA_CHAR_SHAPE, 1, vec![0u8; 8]),
        RawRecord::new(tag::CTRL_HEADER, 1, ch),
        RawRecord::new(tag::SHAPE_COMPONENT, 2, comp),
        RawRecord::new(tag::SHAPE_COMPONENT_PICTURE, 3, pic),
    ])
}

fn next_bindata_storage_id(container: &Hwp5Container) -> u16 {
    let mut max_id = 0u16;
    for path in container.stream_paths() {
        if let Some(rest) = path.strip_prefix("/BinData/BIN") {
            if let Some(hex_part) = rest.get(..4) {
                if let Ok(n) = u16::from_str_radix(hex_part, 16) {
                    max_id = max_id.max(n);
                }
            }
        }
    }
    max_id.saturating_add(1)
}

fn resolve_image_size_hu(bytes: &[u8], w_hu: Option<i32>, h_hu: Option<i32>) -> (i32, i32) {
    let ratio = read_image_pixel_size(bytes)
        .and_then(|(w, h)| (w > 0 && h > 0).then_some(w as f64 / h as f64))
        .unwrap_or(1.6);
    match (w_hu, h_hu) {
        (Some(w), Some(h)) => (w.max(1), h.max(1)),
        (Some(w), None) => (w.max(1), ((w as f64 / ratio).round() as i32).max(1)),
        (None, Some(h)) => (((h as f64 * ratio).round() as i32).max(1), h.max(1)),
        (None, None) => (6000, ((6000.0 / ratio).round() as i32).max(1)),
    }
}

fn read_image_pixel_size(buf: &[u8]) -> Option<(u32, u32)> {
    // PNG
    if buf.len() >= 24 && &buf[0..4] == b"\x89PNG" {
        let w = u32::from_be_bytes(buf[16..20].try_into().ok()?);
        let h = u32::from_be_bytes(buf[20..24].try_into().ok()?);
        return Some((w, h));
    }
    // JPEG
    if buf.len() >= 4 && buf[0] == 0xFF && buf[1] == 0xD8 {
        let mut p = 2usize;
        while p + 9 < buf.len() {
            if buf[p] != 0xFF {
                p += 1;
                continue;
            }
            let m = buf[p + 1];
            if (0xC0..=0xCF).contains(&m) && !matches!(m, 0xC4 | 0xC8 | 0xCC) {
                let h = u16::from_be_bytes(buf[p + 5..p + 7].try_into().ok()?) as u32;
                let w = u16::from_be_bytes(buf[p + 7..p + 9].try_into().ok()?) as u32;
                return Some((w, h));
            }
            if p + 4 > buf.len() {
                break;
            }
            let seg_len = u16::from_be_bytes(buf[p + 2..p + 4].try_into().ok()?) as usize;
            p = p.saturating_add(2 + seg_len.max(2));
        }
    }
    // GIF
    if buf.len() >= 10 && (&buf[0..6] == b"GIF87a" || &buf[0..6] == b"GIF89a") {
        let w = u16::from_le_bytes(buf[6..8].try_into().ok()?) as u32;
        let h = u16::from_le_bytes(buf[8..10].try_into().ok()?) as u32;
        return Some((w, h));
    }
    // BMP
    if buf.len() >= 26 && &buf[0..2] == b"BM" {
        let w = i32::from_le_bytes(buf[18..22].try_into().ok()?).unsigned_abs();
        let h = i32::from_le_bytes(buf[22..26].try_into().ok()?).unsigned_abs();
        return Some((w, h));
    }
    None
}

fn measure_para_prefix_mm(pt_body: &[u8], upto_byte: usize, em_mm: f64) -> f64 {
    let mut off = 0usize;
    let mut w = 0.0f64;
    let limit = upto_byte.min(pt_body.len());
    while off + 2 <= limit {
        let code = u16::from_le_bytes([pt_body[off], pt_body[off + 1]]);
        if code >= 0x20 {
            w += glyph_em(code as u32) * em_mm;
        }
        let step_wchars = if crate::section::is_extended_ctrl_wchar(code) {
            8
        } else {
            1
        };
        off += step_wchars * 2;
    }
    w
}

fn glyph_em(code: u32) -> f64 {
    if code < 0x20 {
        0.0
    } else if code <= 0x7E || (0xFF61..=0xFFDC).contains(&code) {
        0.5
    } else {
        1.0
    }
}
