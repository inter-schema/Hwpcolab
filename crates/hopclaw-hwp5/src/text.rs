use hopclaw_types::{ListStyleMode, ParagraphAlign};

use crate::docinfo::{DocInfoDoc, LineSpacingType, ParaAlignment, ParaShapePatch};
use crate::error::{Hwp5Error, Result};
use crate::record::{tag, RawRecord};
use crate::section::{
    encode_para_text, parse_para_char_shapes, serialize_para_char_shapes, CharShapeRun,
    ParaHeaderMut, ParaHeaderRef, SectionDoc,
};

/// Convert `ParagraphAlign` to binary `ParaAlignment`.
pub fn to_para_alignment(align: ParagraphAlign) -> ParaAlignment {
    match align {
        ParagraphAlign::Left => ParaAlignment::Left,
        ParagraphAlign::Center => ParaAlignment::Center,
        ParagraphAlign::Right => ParaAlignment::Right,
        ParagraphAlign::Justify => ParaAlignment::Justify,
        ParagraphAlign::Distribute => ParaAlignment::Distribute,
    }
}

/// Find or allocate a `PARA_SHAPE` matching `align`, cloning shape 0 when needed.
pub fn ensure_aligned_para_shape(docinfo: &mut DocInfoDoc, align: ParaAlignment) -> Result<u16> {
    let want = align as u32;
    let shapes = docinfo.para_shapes();
    for (idx, body) in shapes.iter().enumerate() {
        if body.len() >= 4 {
            let bits = (u32::from_le_bytes(body[0..4].try_into().unwrap()) >> 2) & 0x07;
            if bits == want {
                return u16::try_from(idx).map_err(|_| {
                    Hwp5Error::DocInfoCorrupt("ParaShape ID overflows u16".to_string())
                });
            }
        }
    }
    let base = docinfo
        .get_para_shape(0)
        .ok_or_else(|| Hwp5Error::DocInfoCorrupt("DocInfo has no PARA_SHAPE records".to_string()))?
        .to_vec();
    let patched = ParaShapePatch {
        alignment: Some(align),
        ..Default::default()
    }
    .apply_to(&base)?;
    docinfo.find_or_add_para_shape(patched)
}

/// Replace all occurrences of `find` with `replace` across body paragraphs (and cells if present).
/// Shifts `PARA_CHAR_SHAPE` character positions and updates `PARA_HEADER.char_count`.
pub fn replace_text(sec: &mut SectionDoc, find: &str, replace: &str) -> Result<usize> {
    if find.is_empty() {
        return Err(Hwp5Error::InvalidTarget(
            "replace_text: 'find' string cannot be empty".to_string(),
        ));
    }
    let find_utf16: Vec<u16> = find.encode_utf16().collect();
    let repl_utf16: Vec<u16> = replace.encode_utf16().collect();
    let find_bytes: Vec<u8> = find_utf16.iter().flat_map(|u| u.to_le_bytes()).collect();
    let repl_bytes: Vec<u8> = repl_utf16.iter().flat_map(|u| u.to_le_bytes()).collect();

    let mut replaced_count = 0usize;
    let len = sec.records.len();
    let mut i = 0usize;
    while i < len {
        if sec.records[i].tag != tag::PARA_TEXT {
            i += 1;
            continue;
        }
        let pt_level = sec.records[i].level;
        if pt_level == 0 {
            i += 1;
            continue;
        }
        let ph_level = pt_level - 1;
        let ph_idx = (0..i)
            .rev()
            .find(|&k| sec.records[k].tag == tag::PARA_HEADER && sec.records[k].level == ph_level);
        let Some(ph_idx) = ph_idx else {
            i += 1;
            continue;
        };

        // Find end of this paragraph's direct children
        let para_end = ((i + 1)..len)
            .find(|&k| sec.records[k].level <= ph_level)
            .unwrap_or(len);
        let cs_idx = ((ph_idx + 1)..para_end).find(|&k| {
            sec.records[k].tag == tag::PARA_CHAR_SHAPE && sec.records[k].level == pt_level
        });

        let mut search_from = 0usize;
        loop {
            let body = &sec.records[i].body;
            let Some(pos) = find_aligned_subslice(body, &find_bytes, search_from) else {
                break;
            };
            let match_wchar = (pos / 2) as i64;
            let find_wchars = find_utf16.len() as i64;
            let repl_wchars = repl_utf16.len() as i64;
            let delta = repl_wchars - find_wchars;

            // Swap UTF-16LE bytes in PARA_TEXT
            let mut new_body = Vec::with_capacity(
                body.len()
                    .saturating_sub(find_bytes.len())
                    .saturating_add(repl_bytes.len()),
            );
            new_body.extend_from_slice(&body[..pos]);
            new_body.extend_from_slice(&repl_bytes);
            new_body.extend_from_slice(&body[pos + find_bytes.len()..]);
            sec.records[i].body = new_body;

            if delta != 0 {
                // Update PARA_HEADER char_count preserving MSB last-in-list bit
                if let Ok(mut hdr) = ParaHeaderMut::wrap(&mut sec.records[ph_idx].body) {
                    let new_cc = (hdr.char_count() as i64 + delta).max(1) as u32;
                    let last = hdr.is_last_in_list();
                    hdr.set_char_count(new_cc, last);
                }
                // Shift PARA_CHAR_SHAPE positions
                if let Some(csi) = cs_idx {
                    if let Ok(mut runs) = parse_para_char_shapes(&sec.records[csi].body) {
                        for run in &mut runs {
                            let p = run.char_pos as i64;
                            if p >= match_wchar + find_wchars {
                                run.char_pos = (p + delta).max(0) as u32;
                            } else if p > match_wchar {
                                run.char_pos = match_wchar as u32;
                            }
                        }
                        sec.records[csi].body = serialize_para_char_shapes(&runs);
                    }
                }
            }

            replaced_count += 1;
            search_from = pos + repl_bytes.len();
        }

        i += 1;
    }

    Ok(replaced_count)
}

/// Ensure UTF-16 code unit boundary alignment when searching raw UTF-16LE bytes.
fn find_aligned_subslice(haystack: &[u8], needle: &[u8], start_byte: usize) -> Option<usize> {
    if needle.is_empty() || start_byte + needle.len() > haystack.len() {
        return None;
    }
    let mut off = start_byte;
    while off + needle.len() <= haystack.len() {
        if &haystack[off..off + needle.len()] == needle {
            return Some(off);
        }
        off += 2;
    }
    None
}

/// Pick a representative template `PARA_HEADER` + `PARA_LINE_SEG` + `char_shape_id` from `sec`,
/// build a level-0 paragraph cluster, and insert it at the end of top-level paragraphs.
pub fn append_paragraph_with_char_shape(
    docinfo: &mut DocInfoDoc,
    sec: &mut SectionDoc,
    text: &str,
    align: Option<ParagraphAlign>,
    char_shape_id: Option<u32>,
) -> Result<usize> {
    let para_shape_id = match align {
        Some(a) => ensure_aligned_para_shape(docinfo, to_para_alignment(a))?,
        None => 0,
    };

    let (tpl_hdr_len, tpl_cs_id, tpl_ls_body) = find_body_para_template(sec);
    let cs_id = char_shape_id.unwrap_or(tpl_cs_id);
    let inst_id = sec.pick_fresh_instance_id();

    let (pt_bytes, char_count) = if text.is_empty() {
        (None, 1u32)
    } else {
        let (bytes, count) = encode_para_text(text);
        (Some(bytes), count)
    };

    let mut ph_body = vec![0u8; tpl_hdr_len];
    let mut ph = ParaHeaderMut::wrap(&mut ph_body)?;
    ph.set_char_count(char_count, false);
    ph.set_control_mask(0);
    ph.set_para_shape_id(para_shape_id);
    ph.set_char_shape_count(1);
    ph.set_range_tag_count(0);
    ph.set_line_seg_count(1);
    ph.set_instance_id(inst_id);

    let mut cluster = Vec::with_capacity(4);
    cluster.push(RawRecord::new(tag::PARA_HEADER, 0, ph_body));
    if let Some(pt) = pt_bytes {
        cluster.push(RawRecord::new(tag::PARA_TEXT, 1, pt));
    }
    cluster.push(RawRecord::new(
        tag::PARA_CHAR_SHAPE,
        1,
        serialize_para_char_shapes(&[CharShapeRun {
            char_pos: 0,
            char_shape_id: cs_id,
        }]),
    ));
    cluster.push(RawRecord::new(tag::PARA_LINE_SEG, 1, tpl_ls_body));

    let insert_at = sec.records.len();
    let new_para_index = sec.top_level_para_indices().len();
    sec.records.splice(insert_at..insert_at, cluster);
    sec.normalize_last_para_flags()?;
    Ok(new_para_index)
}

/// Append a bold heading paragraph scaled to `size_hu` HWPUNITs (`1 pt = 100 HWPUNIT`).
pub fn append_heading(
    docinfo: &mut DocInfoDoc,
    sec: &mut SectionDoc,
    _level: u8,
    text: &str,
    size_hu: i32,
) -> Result<usize> {
    let base_cs = docinfo
        .get_char_shape(0)
        .ok_or_else(|| Hwp5Error::DocInfoCorrupt("DocInfo has no CHAR_SHAPE records".to_string()))?
        .to_vec();
    let patched = crate::docinfo::CharShapePatch {
        size_pt: Some((size_hu as f64) / 100.0),
        bold: Some(true),
        ..Default::default()
    }
    .apply_to(&base_cs)?;
    let cs_id = docinfo.find_or_add_char_shape(patched)? as u32;
    append_paragraph_with_char_shape(docinfo, sec, text, None, Some(cs_id))
}

/// Set numbered or bulleted list formatting on a body paragraph (by `para_index` or `target_text`).
pub fn set_list_style(
    docinfo: &mut DocInfoDoc,
    sec: &mut SectionDoc,
    para_index: Option<usize>,
    target_text: Option<&str>,
    mode: ListStyleMode,
) -> Result<()> {
    let ph_idx = resolve_body_para_record_idx(sec, para_index, target_text)?;
    let base_ps_id = ParaHeaderRef::wrap(&sec.records[ph_idx].body)?.para_shape_id();
    let base_ps = docinfo
        .get_para_shape(base_ps_id)
        .or_else(|| docinfo.get_para_shape(0))
        .ok_or_else(|| Hwp5Error::DocInfoCorrupt("DocInfo has no PARA_SHAPE records".to_string()))?
        .to_vec();

    let (heading_kind, heading_id) = match mode {
        ListStyleMode::Numbered => (2u8, docinfo.append_default_numbering()?),
        ListStyleMode::Bullet => (3u8, docinfo.append_default_bullet()?),
    };

    let patched_ps = ParaShapePatch {
        heading_kind: Some(heading_kind),
        heading_id: Some(heading_id),
        line_spacing_type: Some(LineSpacingType::Percent),
        ..Default::default()
    }
    .apply_to(&base_ps)?;
    let new_ps_id = docinfo.find_or_add_para_shape(patched_ps)?;
    ParaHeaderMut::wrap(&mut sec.records[ph_idx].body)?.set_para_shape_id(new_ps_id);
    Ok(())
}

fn resolve_body_para_record_idx(
    sec: &SectionDoc,
    para_index: Option<usize>,
    target_text: Option<&str>,
) -> Result<usize> {
    if let Some(needle) = target_text.filter(|s| !s.is_empty()) {
        let hit = crate::style::find_text_range(sec, needle)?;
        return Ok(hit.para_header_idx);
    }
    let idx = para_index.ok_or_else(|| {
        Hwp5Error::InvalidTarget("set_list_style requires either 'para_index' or 'target_text'".to_string())
    })?;
    let top = sec.top_level_para_indices();
    top.get(idx).copied().ok_or_else(|| {
        Hwp5Error::TargetNotFound(format!(
            "paragraph index {idx} out of bounds (section has {} paragraphs)",
            top.len()
        ))
    })
}

fn find_body_para_template(sec: &SectionDoc) -> (usize, u32, Vec<u8>) {
    let hdr_len = sec
        .records
        .iter()
        .find(|r| r.tag == tag::PARA_HEADER)
        .map(|r| if r.body.len() >= 24 { 24 } else { 22 })
        .unwrap_or(24);
    let cs_id = sec
        .records
        .iter()
        .find(|r| r.tag == tag::PARA_CHAR_SHAPE && r.body.len() >= 8)
        .map(|r| u32::from_le_bytes(r.body[4..8].try_into().unwrap()))
        .unwrap_or(0);
    let ls_body = sec
        .records
        .iter()
        .find(|r| r.tag == tag::PARA_LINE_SEG && r.level == 1 && r.body.len() >= 36)
        .map(|r| r.body[..36].to_vec())
        .unwrap_or_else(|| vec![0u8; 36]);
    (hdr_len, cs_id, ls_body)
}
