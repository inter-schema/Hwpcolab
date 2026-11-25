use hopclaw_types::{HighlightValue, ParaIndexTarget, ParagraphAlign};

use crate::docinfo::{parse_hex_color, CharShapePatch, DocInfoDoc, ParaShapePatch};
use crate::error::{Hwp5Error, Result};
use crate::record::{tag, RawRecord};
use crate::section::{
    parse_para_char_shapes, serialize_para_char_shapes, CharShapeRun, ParaHeaderMut, ParaHeaderRef,
    SectionDoc,
};
use crate::text::to_para_alignment;

/// Matched text span inside a section's `PARA_TEXT` (`0x43`).
#[derive(Debug, Clone)]
pub struct TextHit {
    pub para_header_idx: usize,
    pub para_text_idx: usize,
    pub para_char_shape_idx: Option<usize>,
    pub wchar_start: u32,
    pub wchar_end: u32,
    pub para_text_level: u16,
    pub total_wchars: u32,
}

/// Two-pass target resolution: level-1 body paragraphs first, falling back to level >= 3 cells.
pub fn find_text_range(sec: &SectionDoc, target: &str) -> Result<TextHit> {
    if target.is_empty() {
        return Err(Hwp5Error::InvalidTarget(
            "target text cannot be empty".to_string(),
        ));
    }
    let needle_utf16: Vec<u16> = target.encode_utf16().collect();
    let needle_bytes: Vec<u8> = needle_utf16.iter().flat_map(|u| u.to_le_bytes()).collect();

    if let Some(hit) = scan_level(sec, &needle_bytes, needle_utf16.len() as u32, true) {
        return Ok(hit);
    }
    if let Some(hit) = scan_level(sec, &needle_bytes, needle_utf16.len() as u32, false) {
        return Ok(hit);
    }
    let trimmed = target.trim();
    if trimmed != target && !trimmed.is_empty() {
        let trim_u16: Vec<u16> = trimmed.encode_utf16().collect();
        let trim_bytes: Vec<u8> = trim_u16.iter().flat_map(|u| u.to_le_bytes()).collect();
        if let Some(hit) = scan_level(sec, &trim_bytes, trim_u16.len() as u32, true) {
            return Ok(hit);
        }
        if let Some(hit) = scan_level(sec, &trim_bytes, trim_u16.len() as u32, false) {
            return Ok(hit);
        }
    }

    Err(Hwp5Error::TargetNotFound(format!(
        "target text '{target}' not found in section"
    )))
}

fn scan_level(
    sec: &SectionDoc,
    needle_bytes: &[u8],
    needle_wchars: u32,
    body_only: bool,
) -> Option<TextHit> {
    let len = sec.records.len();
    for (pt_idx, pt_rec) in sec.records.iter().enumerate() {
        if pt_rec.tag != tag::PARA_TEXT {
            continue;
        }
        if body_only && pt_rec.level != 1 {
            continue;
        }
        if !body_only && pt_rec.level < 3 {
            continue;
        }
        let body = &pt_rec.body;
        if needle_bytes.len() > body.len() {
            continue;
        }
        let mut match_byte = None;
        let mut off = 0usize;
        while off + needle_bytes.len() <= body.len() {
            if &body[off..off + needle_bytes.len()] == needle_bytes {
                match_byte = Some(off);
                break;
            }
            off += 2;
        }
        let Some(byte_off) = match_byte else {
            continue;
        };

        let ph_level = pt_rec.level - 1;
        let ph_idx = (0..pt_idx)
            .rev()
            .find(|&k| sec.records[k].tag == tag::PARA_HEADER && sec.records[k].level == ph_level)?;
        let para_end = ((ph_idx + 1)..len)
            .find(|&k| sec.records[k].level <= ph_level)
            .unwrap_or(len);
        let cs_idx = ((ph_idx + 1)..para_end).find(|&k| {
            sec.records[k].tag == tag::PARA_CHAR_SHAPE && sec.records[k].level == pt_rec.level
        });
        let wchar_start = (byte_off / 2) as u32;
        return Some(TextHit {
            para_header_idx: ph_idx,
            para_text_idx: pt_idx,
            para_char_shape_idx: cs_idx,
            wchar_start,
            wchar_end: wchar_start + needle_wchars,
            para_text_level: pt_rec.level,
            total_wchars: (body.len() / 2) as u32,
        });
    }
    None
}

/// Parameters for `apply_text_style`.
#[derive(Debug, Clone)]
pub struct TextStyleRequest<'a> {
    pub target_text: &'a str,
    pub font_family: Option<&'a str>,
    pub size_hu: Option<i32>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
    pub color: Option<&'a str>,
    pub highlight: Option<&'a HighlightValue>,
    pub letter_spacing: Option<f64>,
    pub char_ratio: Option<f64>,
}

/// Apply character formatting to the first occurrence of `req.target_text`.
pub fn apply_text_style(
    docinfo: &mut DocInfoDoc,
    sec: &mut SectionDoc,
    req: &TextStyleRequest<'_>,
) -> Result<()> {
    let hit = find_text_range(sec, req.target_text)?;
    let cs_idx = hit.para_char_shape_idx.ok_or_else(|| {
        Hwp5Error::MalformedRecord(format!(
            "PARA_CHAR_SHAPE missing for target '{}'",
            req.target_text
        ))
    })?;

    let runs = parse_para_char_shapes(&sec.records[cs_idx].body)?;
    if runs.is_empty() {
        return Err(Hwp5Error::MalformedRecord(
            "PARA_CHAR_SHAPE record has no runs".to_string(),
        ));
    }

    let base_cs_id = cs_id_at_offset(&runs, hit.wchar_start) as u16;
    let face_ids = match req.font_family.filter(|s| !s.is_empty()) {
        Some(font) => Some(docinfo.find_or_add_face_name(font)?),
        None => None,
    };

    let base_cs = docinfo
        .get_char_shape(base_cs_id)
        .ok_or_else(|| {
            Hwp5Error::DocInfoCorrupt(format!("base CharShape ID {base_cs_id} out of range"))
        })?
        .to_vec();

    let text_color_bgr = match req.color {
        Some(hex) => Some(parse_hex_color(hex)?),
        None => None,
    };
    let shade_color_bgr = match req.highlight {
        Some(HighlightValue::Color(hex)) if !hex.eq_ignore_ascii_case("none") => {
            Some(parse_hex_color(hex)?)
        }
        Some(HighlightValue::Bool(true)) => Some(parse_hex_color("#FFFF00")?),
        _ => None,
    };

    let new_cs_body = CharShapePatch {
        face_ids,
        size_pt: req.size_hu.map(|hu| (hu as f64) / 100.0),
        bold: req.bold,
        italic: req.italic,
        underline: req.underline,
        text_color_bgr,
        shade_color_bgr,
        char_ratio: req.char_ratio.map(|v| v.round().clamp(1.0, 255.0) as u8),
        letter_spacing: req
            .letter_spacing
            .map(|v| v.round().clamp(-128.0, 127.0) as i8),
        ..Default::default()
    }
    .apply_to(&base_cs)?;

    let new_cs_id = docinfo.find_or_add_char_shape(new_cs_body)? as u32;
    let updated_runs = split_char_shape_runs(&runs, hit.wchar_start, hit.wchar_end, new_cs_id);
    let run_count = u16::try_from(updated_runs.len()).unwrap_or(u16::MAX);
    sec.records[cs_idx].body = serialize_para_char_shapes(&updated_runs);
    ParaHeaderMut::wrap(&mut sec.records[hit.para_header_idx].body)?.set_char_shape_count(run_count);

    // Also emit PARA_RANGE_TAG (`0x46`) for highlight rendering in Hancom Docs.
    if let Some(hl_bgr) = shade_color_bgr {
        let mut body = vec![0u8; 12];
        body[0..4].copy_from_slice(&hit.wchar_start.to_le_bytes());
        body[4..8].copy_from_slice(&hit.wchar_end.to_le_bytes());
        let tag_word = (0x02u32 << 24) | (hl_bgr & 0x00FF_FFFF);
        body[8..12].copy_from_slice(&tag_word.to_le_bytes());
        sec.records.insert(
            cs_idx + 1,
            RawRecord::new(tag::PARA_RANGE_TAG, hit.para_text_level, body),
        );
        let mut ph = ParaHeaderMut::wrap(&mut sec.records[hit.para_header_idx].body)?;
        let cur = ph.range_tag_count();
        ph.set_range_tag_count(cur.saturating_add(1));
    }

    Ok(())
}

/// Parameters for `apply_paragraph_style`.
#[derive(Debug, Clone)]
pub struct ParaStyleRequest<'a> {
    pub para_index: Option<ParaIndexTarget>,
    pub target_text: Option<&'a str>,
    pub align: Option<ParagraphAlign>,
    pub indent_hu: Option<i32>,
    pub line_spacing_percent: Option<f64>,
    pub spacing_before_hu: Option<i32>,
    pub spacing_after_hu: Option<i32>,
}

/// Apply paragraph alignment, indent, and vertical spacing to an existing paragraph.
pub fn apply_paragraph_style(
    docinfo: &mut DocInfoDoc,
    sec: &mut SectionDoc,
    req: &ParaStyleRequest<'_>,
) -> Result<()> {
    let ph_idx = if let Some(text) = req.target_text.filter(|s| !s.is_empty()) {
        find_text_range(sec, text)?.para_header_idx
    } else {
        let top = sec.top_level_para_indices();
        if top.is_empty() {
            return Err(Hwp5Error::TargetNotFound(
                "section contains no paragraphs".to_string(),
            ));
        }
        let target_pos = match req.para_index {
            Some(ParaIndexTarget::Last) | None => top.len() - 1,
            Some(ParaIndexTarget::Index(i)) if i < 0 => top.len() - 1,
            Some(ParaIndexTarget::Index(i)) => i as usize,
        };
        top.get(target_pos).copied().ok_or_else(|| {
            Hwp5Error::TargetNotFound(format!(
                "paragraph index {target_pos} out of bounds (section has {} paragraphs)",
                top.len()
            ))
        })?
    };

    let base_ps_id = ParaHeaderRef::wrap(&sec.records[ph_idx].body)?.para_shape_id();
    let base_ps = docinfo
        .get_para_shape(base_ps_id)
        .or_else(|| docinfo.get_para_shape(0))
        .ok_or_else(|| Hwp5Error::DocInfoCorrupt("DocInfo has no PARA_SHAPE records".to_string()))?
        .to_vec();

    let patched_ps = ParaShapePatch {
        alignment: req.align.map(to_para_alignment),
        indent: req.indent_hu,
        spacing_before: req.spacing_before_hu,
        spacing_after: req.spacing_after_hu,
        line_spacing: req
            .line_spacing_percent
            .map(|p| p.round() as i32),
        line_spacing_type: req
            .line_spacing_percent
            .map(|_| crate::docinfo::LineSpacingType::Percent),
        ..Default::default()
    }
    .apply_to(&base_ps)?;

    let new_ps_id = docinfo.find_or_add_para_shape(patched_ps)?;
    ParaHeaderMut::wrap(&mut sec.records[ph_idx].body)?.set_para_shape_id(new_ps_id);
    Ok(())
}

pub fn cs_id_at_offset(runs: &[CharShapeRun], pos: u32) -> u32 {
    let mut id = runs.first().map(|r| r.char_shape_id).unwrap_or(0);
    for r in runs {
        if r.char_pos <= pos {
            id = r.char_shape_id;
        }
    }
    id
}

pub fn split_char_shape_runs(
    runs: &[CharShapeRun],
    start: u32,
    end: u32,
    new_cs_id: u32,
) -> Vec<CharShapeRun> {
    if start >= end {
        return runs.to_vec();
    }
    let after_id = cs_id_at_offset(runs, end);
    let mut out = Vec::with_capacity(runs.len() + 2);
    for r in runs {
        if r.char_pos < start {
            out.push(*r);
        }
    }
    out.push(CharShapeRun {
        char_pos: start,
        char_shape_id: new_cs_id,
    });
    if end < u32::MAX {
        let mut pushed_tail = false;
        for r in runs {
            if r.char_pos >= end {
                if !pushed_tail {
                    if r.char_pos > end {
                        out.push(CharShapeRun {
                            char_pos: end,
                            char_shape_id: after_id,
                        });
                    }
                    pushed_tail = true;
                }
                out.push(*r);
            }
        }
        if !pushed_tail {
            out.push(CharShapeRun {
                char_pos: end,
                char_shape_id: after_id,
            });
        }
    }
    // Collapse consecutive duplicate char_pos keeping the last
    let mut dedup: Vec<CharShapeRun> = Vec::with_capacity(out.len());
    for r in out {
        if let Some(last) = dedup.last_mut() {
            if last.char_pos == r.char_pos {
                *last = r;
                continue;
            }
        }
        dedup.push(r);
    }
    dedup
}
