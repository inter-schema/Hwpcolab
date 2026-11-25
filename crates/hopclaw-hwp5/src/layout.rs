use hopclaw_types::{HeaderFooterLocation, PageNumberAlign};

use crate::docinfo::{DocInfoDoc, ParaAlignment};
use crate::error::{Hwp5Error, Result};
use crate::record::{tag, RawRecord};
use crate::section::{encode_para_text, PageDef, ParaHeaderMut, SectionDoc};
use crate::style::find_text_range;
use crate::text::ensure_aligned_para_shape;

const CTRL_HEAD_LE: &[u8; 4] = b"daeh"; // 'head' in LE
const CTRL_FOOT_LE: &[u8; 4] = b"toof"; // 'foot' in LE
const CTRL_COLD_LE: &[u8; 4] = b"dloc"; // 'cold' in LE
const CTRL_ONTA_LE: &[u8; 4] = b"atno"; // 'onta' in LE
const CTRL_FN_LE: &[u8; 4] = b"  nf"; // 'fn  ' in LE

const HF_HEAD_CHAR: &[u8] = &[
    0x10, 0x00, b'd', b'a', b'e', b'h', 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x00,
];
const HF_FOOT_CHAR: &[u8] = &[
    0x10, 0x00, b't', b'o', b'o', b'f', 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x00,
];
const FN_REF_CHAR: &[u8] = &[
    0x11, 0x00, b' ', b' ', b'n', b'f', 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x11, 0x00,
];
const NOTE_AUTONUM_CHAR: &[u8] = &[
    0x12, 0x00, b'o', b'n', b't', b'a', 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x12, 0x00,
];

/// Resolved DocInfo `STYLE` (`0x1A`) record attributes for Header / Page Number / Footnote.
struct ResolvedStyle {
    index: u8,
    para_shape: u16,
    char_shape: u16,
}

fn resolve_style_by_name(docinfo: &DocInfoDoc, name: &str) -> ResolvedStyle {
    let mut style_idx = 0usize;
    for rec in &docinfo.records {
        if rec.tag != tag::STYLE {
            continue;
        }
        let sb = &rec.body;
        if sb.len() < 4 {
            style_idx += 1;
            continue;
        }
        let k_len = u16::from_le_bytes(sb[0..2].try_into().unwrap()) as usize;
        let k_end = 2 + k_len * 2;
        if k_end + 2 > sb.len() {
            style_idx += 1;
            continue;
        }
        let kor = decode_utf16_raw(&sb[2..k_end]);
        let e_len = u16::from_le_bytes(sb[k_end..k_end + 2].try_into().unwrap()) as usize;
        let e_start = k_end + 2;
        let e_end = e_start + e_len * 2;
        if e_end + 8 > sb.len() {
            style_idx += 1;
            continue;
        }
        let eng = decode_utf16_raw(&sb[e_start..e_end]);
        if eng == name || kor == name {
            let ps = u16::from_le_bytes(sb[e_end + 4..e_end + 6].try_into().unwrap());
            let cs = u16::from_le_bytes(sb[e_end + 6..e_end + 8].try_into().unwrap());
            return ResolvedStyle {
                index: style_idx as u8,
                para_shape: ps,
                char_shape: cs,
            };
        }
        style_idx += 1;
    }
    ResolvedStyle {
        index: 0,
        para_shape: 0,
        char_shape: 0,
    }
}

fn decode_utf16_raw(bytes: &[u8]) -> String {
    let (chunks, _) = bytes.as_chunks::<2>();
    let units: Vec<u16> = chunks.iter().map(|c| u16::from_le_bytes(*c)).collect();
    String::from_utf16_lossy(&units)
}

/// Set page dimensions and uniform margins on the section's `PAGE_DEF` (`0x49`).
pub fn setup_document(sec: &mut SectionDoc, width_hu: i32, height_hu: i32, margin_hu: i32) -> Result<()> {
    let rec = sec
        .records
        .iter_mut()
        .find(|r| r.tag == tag::PAGE_DEF)
        .ok_or_else(|| Hwp5Error::MalformedRecord("setup_document: PAGE_DEF not found".to_string()))?;
    let mut pd = PageDef::parse(&rec.body)?;
    // HWP 5.0 PageDef always stores portrait dimensions (width <= height) and encodes landscape in attr bit 0.
    if width_hu > height_hu {
        pd.width = height_hu.max(1000) as u32;
        pd.height = width_hu.max(1000) as u32;
        pd.attr |= 0x01;
    } else {
        pd.width = width_hu.max(1000) as u32;
        pd.height = height_hu.max(1000) as u32;
        pd.attr &= !0x01;
    }
    let m = margin_hu.max(0) as u32;
    pd.margin_left = m;
    pd.margin_right = m;
    pd.margin_top = m;
    pd.margin_bottom = m;
    pd.write_to(&mut rec.body)?;
    Ok(())
}

/// Update individual page margins on `PAGE_DEF` (`0x49`).
pub fn set_page_margin(
    sec: &mut SectionDoc,
    left_hu: Option<i32>,
    right_hu: Option<i32>,
    top_hu: Option<i32>,
    bottom_hu: Option<i32>,
) -> Result<()> {
    let rec = sec
        .records
        .iter_mut()
        .find(|r| r.tag == tag::PAGE_DEF)
        .ok_or_else(|| Hwp5Error::MalformedRecord("set_page_margin: PAGE_DEF not found".to_string()))?;
    let mut pd = PageDef::parse(&rec.body)?;
    if let Some(v) = left_hu {
        pd.margin_left = v.max(0) as u32;
    }
    if let Some(v) = right_hu {
        pd.margin_right = v.max(0) as u32;
    }
    if let Some(v) = top_hu {
        pd.margin_top = v.max(0) as u32;
    }
    if let Some(v) = bottom_hu {
        pd.margin_bottom = v.max(0) as u32;
    }
    pd.write_to(&mut rec.body)?;
    Ok(())
}

/// Update multi-column definition in the section's `'dloc'` (`cold`) `CTRL_HEADER`.
pub fn set_columns(sec: &mut SectionDoc, count: u8, spacing_hu: Option<i32>) -> Result<()> {
    let c = count.clamp(1, 16) as u16;
    let gap = spacing_hu
        .map(|v| v.max(0) as u16)
        .unwrap_or(match c {
            1 => 0,
            3 => 1134,
            _ => 2268,
        });
    for rec in &mut sec.records {
        if rec.tag == tag::CTRL_HEADER && rec.body.len() >= 8 && &rec.body[0..4] == CTRL_COLD_LE {
            let attr: u16 = 0x1000 | (c << 2);
            rec.body[4..6].copy_from_slice(&attr.to_le_bytes());
            rec.body[6..8].copy_from_slice(&gap.to_le_bytes());
            return Ok(());
        }
    }
    Err(Hwp5Error::MalformedRecord(
        "set_columns: no 'cold' column-definition control found in section".to_string(),
    ))
}

/// Attach a header or footer text block to the section's first body paragraph.
pub fn set_header_footer(
    docinfo: &DocInfoDoc,
    sec: &mut SectionDoc,
    location: HeaderFooterLocation,
    text: &str,
) -> Result<()> {
    if text.is_empty() {
        return Err(Hwp5Error::InvalidTarget(
            "set_header_footer: text cannot be empty".to_string(),
        ));
    }
    let style = resolve_style_by_name(docinfo, "Header");
    let (ph_idx, pt_idx, cluster_end) = first_body_paragraph(sec)?;
    let pd = sec.page_def()?;
    let text_width = pd.map(|p| p.usable_width()).unwrap_or(42520);
    let margin_h = pd
        .map(|p| match location {
            HeaderFooterLocation::Header => p.header_margin,
            HeaderFooterLocation::Footer => p.footer_margin,
        })
        .unwrap_or(0x109C);

    let (ctrl_le, inline_char, list_attr) = match location {
        HeaderFooterLocation::Header => (CTRL_HEAD_LE, HF_HEAD_CHAR, 0x00u32),
        HeaderFooterLocation::Footer => (CTRL_FOOT_LE, HF_FOOT_CHAR, 0x40u32),
    };

    insert_inline_ctrl_before_eop(sec, ph_idx, pt_idx, inline_char, 0x10000)?;

    let mut ctrl_body = Vec::with_capacity(12);
    ctrl_body.extend_from_slice(ctrl_le);
    ctrl_body.extend_from_slice(&[0, 0, 0, 0, 1, 0, 0, 0]);

    let mut lh_body = vec![0u8; 34];
    lh_body[0..4].copy_from_slice(&1u32.to_le_bytes());
    lh_body[4..8].copy_from_slice(&list_attr.to_le_bytes());
    lh_body[8..12].copy_from_slice(&text_width.to_le_bytes());
    lh_body[12..16].copy_from_slice(&margin_h.to_le_bytes());

    let (pt2_bytes, pt2_chars) = encode_para_text(text);
    let mut ph2_body = vec![0u8; 24];
    {
        let mut ph2 = ParaHeaderMut::wrap(&mut ph2_body)?;
        ph2.set_char_count(pt2_chars, true);
        ph2.set_para_shape_id(0);
        ph2.set_style_id(style.index);
        ph2.set_char_shape_count(1);
    }

    let mut cs_body = vec![0u8; 8];
    cs_body[4..8].copy_from_slice(&(style.char_shape as u32).to_le_bytes());

    let hf_cluster = vec![
        RawRecord::new(tag::CTRL_HEADER, 1, ctrl_body),
        RawRecord::new(tag::LIST_HEADER, 2, lh_body),
        RawRecord::new(tag::PARA_HEADER, 2, ph2_body),
        RawRecord::new(tag::PARA_TEXT, 3, pt2_bytes),
        RawRecord::new(tag::PARA_CHAR_SHAPE, 3, cs_body),
    ];

    sec.records.splice(cluster_end..cluster_end, hf_cluster);
    Ok(())
}

/// Insert an auto-incrementing page number inside the header or footer.
pub fn insert_page_number(
    docinfo: &mut DocInfoDoc,
    sec: &mut SectionDoc,
    location: HeaderFooterLocation,
    align: PageNumberAlign,
) -> Result<()> {
    let para_align = match align {
        PageNumberAlign::Left => ParaAlignment::Left,
        PageNumberAlign::Center => ParaAlignment::Center,
        PageNumberAlign::Right => ParaAlignment::Right,
    };
    let ps_id = ensure_aligned_para_shape(docinfo, para_align)?;
    let header_style = resolve_style_by_name(docinfo, "Header");
    let page_num_style = resolve_style_by_name(docinfo, "Page Number");

    let (ph_idx, pt_idx, cluster_end) = first_body_paragraph(sec)?;
    let pd = sec.page_def()?;
    let text_width = pd.map(|p| p.usable_width()).unwrap_or(42520);
    let margin_h = pd
        .map(|p| match location {
            HeaderFooterLocation::Header => p.header_margin,
            HeaderFooterLocation::Footer => p.footer_margin,
        })
        .unwrap_or(0x109C);

    let (ctrl_le, inline_char, list_attr) = match location {
        HeaderFooterLocation::Header => (CTRL_HEAD_LE, HF_HEAD_CHAR, 0x00u32),
        HeaderFooterLocation::Footer => (CTRL_FOOT_LE, HF_FOOT_CHAR, 0x40u32),
    };

    insert_inline_ctrl_before_eop(sec, ph_idx, pt_idx, inline_char, 0x10000)?;

    let mut ctrl_body = Vec::with_capacity(12);
    ctrl_body.extend_from_slice(ctrl_le);
    ctrl_body.extend_from_slice(&[0, 0, 0, 0, 1, 0, 0, 0]);

    let mut lh_body = vec![0u8; 34];
    lh_body[0..4].copy_from_slice(&1u32.to_le_bytes());
    lh_body[4..8].copy_from_slice(&list_attr.to_le_bytes());
    lh_body[8..12].copy_from_slice(&text_width.to_le_bytes());
    lh_body[12..16].copy_from_slice(&margin_h.to_le_bytes());

    let mut ph2_body = vec![0u8; 24];
    {
        let mut ph2 = ParaHeaderMut::wrap(&mut ph2_body)?;
        ph2.set_char_count(9, true); // autonum(8) + EOP
        ph2.set_control_mask(0x40000);
        ph2.set_para_shape_id(ps_id);
        ph2.set_style_id(header_style.index);
        ph2.set_char_shape_count(1);
    }

    let mut pt2_body = NOTE_AUTONUM_CHAR.to_vec();
    pt2_body.extend_from_slice(&0x000Du16.to_le_bytes());

    let mut cs_body = vec![0u8; 8];
    cs_body[4..8].copy_from_slice(&(page_num_style.char_shape as u32).to_le_bytes());

    let mut onta_body = Vec::with_capacity(16);
    onta_body.extend_from_slice(CTRL_ONTA_LE);
    onta_body.extend_from_slice(&0u32.to_le_bytes()); // note-type 0 = page number
    onta_body.extend_from_slice(&[1, 0, 0, 0, 0, 0, 0, 0]);

    let cluster = vec![
        RawRecord::new(tag::CTRL_HEADER, 1, ctrl_body),
        RawRecord::new(tag::LIST_HEADER, 2, lh_body),
        RawRecord::new(tag::PARA_HEADER, 2, ph2_body),
        RawRecord::new(tag::PARA_TEXT, 3, pt2_body),
        RawRecord::new(tag::PARA_CHAR_SHAPE, 3, cs_body),
        RawRecord::new(tag::CTRL_HEADER, 3, onta_body),
    ];
    sec.records.splice(cluster_end..cluster_end, cluster);
    Ok(())
}

/// Insert a footnote anchored after `anchor_text` (or in `para_index`).
pub fn insert_footnote(
    docinfo: &DocInfoDoc,
    sec: &mut SectionDoc,
    anchor_text: &str,
    note_text: &str,
    para_index: Option<isize>,
) -> Result<()> {
    let style = resolve_style_by_name(docinfo, "Footnote");

    let (ph_idx, pt_idx, ins_byte, cluster_end) = if !anchor_text.is_empty() {
        let hit = find_text_range(sec, anchor_text)?;
        let c_end = ((hit.para_header_idx + 1)..sec.records.len())
            .find(|&k| sec.records[k].level <= sec.records[hit.para_header_idx].level)
            .unwrap_or(sec.records.len());
        (
            hit.para_header_idx,
            hit.para_text_idx,
            (hit.wchar_end as usize) * 2,
            c_end,
        )
    } else if let Some(pi) = para_index {
        let top = sec.top_level_para_indices();
        let target = if pi < 0 {
            top.len().saturating_sub(1)
        } else {
            pi as usize
        };
        let &ph_i = top.get(target).ok_or_else(|| {
            Hwp5Error::TargetNotFound(format!("insert_footnote: paragraph {pi} not found"))
        })?;
        let c_end = ((ph_i + 1)..sec.records.len())
            .find(|&k| sec.records[k].level == 0)
            .unwrap_or(sec.records.len());
        let pt_i = ((ph_i + 1)..c_end)
            .find(|&k| sec.records[k].tag == tag::PARA_TEXT && sec.records[k].level == 1)
            .ok_or_else(|| {
                Hwp5Error::TargetNotFound("insert_footnote: target paragraph has no PARA_TEXT".to_string())
            })?;
        let body = &sec.records[pt_i].body;
        let eop_off = if body.len() >= 2 && body[body.len() - 2..] == [0x0d, 0x00] {
            body.len() - 2
        } else {
            body.len()
        };
        (ph_i, pt_i, eop_off, c_end)
    } else {
        return Err(Hwp5Error::InvalidTarget(
            "insert_footnote requires anchor_text or para_index".to_string(),
        ));
    };

    let base_level = sec.records[ph_idx].level;
    let note_inst = sec.pick_fresh_instance_id();

    // 1. Insert footnote ref control char in anchor paragraph text
    let pt_body = &mut sec.records[pt_idx].body;
    let at = ins_byte.min(pt_body.len());
    pt_body.splice(at..at, FN_REF_CHAR.iter().copied());
    {
        let mut ph = ParaHeaderMut::wrap(&mut sec.records[ph_idx].body)?;
        let cc = ph.char_count() + 8;
        let last = ph.is_last_in_list();
        ph.set_char_count(cc, last);
        ph.set_control_mask(ph.control_mask() | 0x20000);
    }

    // 2. Build footnote content cluster
    let mut ctrl_body = Vec::with_capacity(20);
    ctrl_body.extend_from_slice(CTRL_FN_LE);
    ctrl_body.extend_from_slice(&[
        0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x29, 0x00, 0x00, 0x00, 0x00, 0x00,
    ]);
    ctrl_body.extend_from_slice(&note_inst.to_le_bytes());

    let mut lh_body = vec![0u8; 16];
    lh_body[0..4].copy_from_slice(&1u32.to_le_bytes());

    let (text_bytes, _) = encode_para_text(&format!(" {note_text}"));
    let mut pt2_body = NOTE_AUTONUM_CHAR.to_vec();
    pt2_body.extend_from_slice(&text_bytes);
    let note_chars = (pt2_body.len() / 2) as u32;

    let mut ph2_body = vec![0u8; 24];
    {
        let mut ph2 = ParaHeaderMut::wrap(&mut ph2_body)?;
        ph2.set_char_count(note_chars, true);
        ph2.set_control_mask(0x40000);
        ph2.set_para_shape_id(style.para_shape);
        ph2.set_style_id(style.index);
        ph2.set_char_shape_count(1);
    }

    let mut cs_body = vec![0u8; 8];
    cs_body[4..8].copy_from_slice(&(style.char_shape as u32).to_le_bytes());

    let mut onta_body = Vec::with_capacity(16);
    onta_body.extend_from_slice(CTRL_ONTA_LE);
    onta_body.extend_from_slice(&1u32.to_le_bytes()); // note-type 1 = footnote
    onta_body.extend_from_slice(&[1, 0, 0, 0, 0, 0, 0x29, 0]);

    let lvl_shift = base_level;
    let fn_cluster = vec![
        RawRecord::new(tag::CTRL_HEADER, 1 + lvl_shift, ctrl_body),
        RawRecord::new(tag::LIST_HEADER, 2 + lvl_shift, lh_body),
        RawRecord::new(tag::PARA_HEADER, 2 + lvl_shift, ph2_body),
        RawRecord::new(tag::PARA_TEXT, 3 + lvl_shift, pt2_body),
        RawRecord::new(tag::PARA_CHAR_SHAPE, 3 + lvl_shift, cs_body),
        RawRecord::new(tag::CTRL_HEADER, 3 + lvl_shift, onta_body),
    ];
    sec.records.splice(cluster_end..cluster_end, fn_cluster);
    Ok(())
}

fn first_body_paragraph(sec: &mut SectionDoc) -> Result<(usize, usize, usize)> {
    let top = sec.top_level_para_indices();
    for (idx, &ph_idx) in top.iter().enumerate() {
        let cluster_end = top.get(idx + 1).copied().unwrap_or(sec.records.len());
        if let Some(pt_idx) = ((ph_idx + 1)..cluster_end)
            .find(|&k| sec.records[k].tag == tag::PARA_TEXT && sec.records[k].level == 1)
        {
            return Ok((ph_idx, pt_idx, cluster_end));
        }
    }
    // Synthesize an empty PARA_TEXT if first top-level paragraph lacks one
    if let Some(&ph_idx) = top.first() {
        let cluster_end = top.get(1).copied().unwrap_or(sec.records.len());
        sec.records.insert(
            ph_idx + 1,
            RawRecord::new(tag::PARA_TEXT, 1, vec![0x0d, 0x00]),
        );
        return Ok((ph_idx, ph_idx + 1, cluster_end + 1));
    }
    Err(Hwp5Error::TargetNotFound(
        "section has no body paragraph to host header/footer".to_string(),
    ))
}

fn insert_inline_ctrl_before_eop(
    sec: &mut SectionDoc,
    ph_idx: usize,
    pt_idx: usize,
    inline_ctrl: &[u8],
    ctrl_mask_bit: u32,
) -> Result<()> {
    let body = &mut sec.records[pt_idx].body;
    let ins_at = if body.len() >= 2 && body[body.len() - 2..] == [0x0d, 0x00] {
        body.len() - 2
    } else {
        body.len()
    };
    body.splice(ins_at..ins_at, inline_ctrl.iter().copied());

    let mut ph = ParaHeaderMut::wrap(&mut sec.records[ph_idx].body)?;
    let old_cc = ph.char_count();
    let last = ph.is_last_in_list();
    ph.set_char_count(old_cc + 8, last);
    ph.set_control_mask(ph.control_mask() | ctrl_mask_bit);
    Ok(())
}
