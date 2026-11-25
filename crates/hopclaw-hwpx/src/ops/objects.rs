use std::collections::HashSet;
use std::path::Path;

use hopclaw_types::{
    CellHAlign, CellVAlign, HeaderFooterLocation, HwpUnit, ImageCellTarget, PageNumberAlign,
    SealMode, HWPUNIT_PER_MM,
};

use crate::error::HwpxError;
use crate::model::{first_char_pr_id, run_with_text, HwpxDocument};
use crate::ops::table::{
    cell_content_width, find_cell_index_in_row, op_set_cell_align, op_set_cell_padding,
};
use crate::ops::text::ensure_clean_para_pr;
use crate::xml_util::{
    drop_linesegs, extract_t_text, fresh_id, get_attr, scan_top_level, set_or_add_attr,
    set_tag_attr, splice_span, xml_escape_attr, xml_escape_text,
};

const PT_TO_MM: f64 = 25.4 / 72.0;

fn image_mime(ext: &str) -> Result<&'static str, HwpxError> {
    match ext.to_ascii_lowercase().as_str() {
        "png" => Ok("image/png"),
        "jpg" | "jpeg" => Ok("image/jpeg"),
        "bmp" => Ok("image/bmp"),
        "gif" => Ok("image/gif"),
        other => Err(HwpxError::InvalidArgument(format!(
            "insert_image: unsupported extension .{other} (png/jpg/bmp/gif)"
        ))),
    }
}

fn embed_image_binary(
    doc: &mut HwpxDocument,
    source_path: &str,
) -> Result<(String, String, Vec<u8>), HwpxError> {
    let p = Path::new(source_path);
    let ext = p
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("png")
        .to_ascii_lowercase();
    let mime = image_mime(&ext)?;
    let raw_bytes = std::fs::read(p)?;

    let existing_bins: Vec<String> = doc
        .archive
        .entry_names()
        .into_iter()
        .filter(|n| n.to_ascii_lowercase().starts_with("bindata/"))
        .collect();
    let mut used_ids = HashSet::new();
    if let Some(hpf_name) = doc.archive.hpf_name() {
        let hpf = doc.archive.read_text_ro(&hpf_name)?;
        for item in scan_top_level(&hpf, "opf:item")? {
            if let Some(id) = get_attr(item.attrs(&hpf, "opf:item"), "id") {
                used_ids.insert(id);
            }
        }
    }

    let mut n = 1usize;
    loop {
        let img_suffix = format!("/image{n}.{ext}");
        let img_short = format!("/img{n}.{ext}");
        let item_id = format!("image{n}");
        let conflict = existing_bins
            .iter()
            .any(|b| b.ends_with(&img_suffix) || b.ends_with(&img_short))
            || used_ids.contains(&item_id);
        if !conflict {
            break;
        }
        n += 1;
    }

    let entry = format!("BinData/image{n}.{ext}");
    let item_id = format!("image{n}");
    doc.archive.write_bytes(entry.clone(), raw_bytes.clone());

    if let Some(hpf_name) = doc.archive.hpf_name() {
        let mut hpf = doc.archive.read_text(&hpf_name)?;
        if !hpf.contains(&format!(r#"href="{entry}""#)) {
            let new_item = format!(
                r#"<opf:item id="{item_id}" href="{entry}" media-type="{mime}" isEmbeded="1"/>"#
            );
            hpf = hpf.replacen("</opf:manifest>", &format!("{new_item}</opf:manifest>"), 1);
            doc.archive.write_text(hpf_name, hpf);
        }
    }

    Ok((entry, item_id, raw_bytes))
}

fn build_pic_xml(
    doc: &HwpxDocument,
    item_id: &str,
    width: Option<HwpUnit>,
    height: Option<HwpUnit>,
) -> Result<String, HwpxError> {
    for name in doc.archive.section_names() {
        let xml = doc.archive.read_text_ro(&name)?;
        let pics = scan_top_level(&xml, "hp:pic")?;
        if let Some(first) = pics.first() {
            let pic_attrs = set_or_add_attr(
                &set_or_add_attr(first.attrs(&xml, "hp:pic"), "id", &fresh_id()),
                "instid",
                &fresh_id(),
            );
            let mut inner = first.inner(&xml).to_string();
            for cap in scan_top_level(&inner, "hp:caption")?.into_iter().rev() {
                inner = splice_span(&inner, &cap, "");
            }
            if let Some(img_el) = scan_top_level(&inner, "hc:img")?.first().cloned() {
                let updated = set_tag_attr(img_el.full(&inner), "binaryItemIDRef", item_id);
                inner = splice_span(&inner, &img_el, &updated);
            }
            if let (Some(w), Some(h)) = (width, height) {
                if let Some(cur_sz) = scan_top_level(&inner, "hp:curSz")?.first().cloned() {
                    let u = set_tag_attr(
                        &set_tag_attr(cur_sz.full(&inner), "width", &w.to_string()),
                        "height",
                        &h.to_string(),
                    );
                    inner = splice_span(&inner, &cur_sz, &u);
                }
                if let Some(rect) = scan_top_level(&inner, "hp:imgRect")?.first().cloned() {
                    inner = splice_span(
                        &inner,
                        &rect,
                        &format!(
                            r#"<hp:imgRect><hc:pt0 x="0" y="0"/><hc:pt1 x="{w}" y="0"/><hc:pt2 x="{w}" y="{h}"/><hc:pt3 x="0" y="{h}"/></hp:imgRect>"#
                        ),
                    );
                }
                if let Some(sz) = scan_top_level(&inner, "hp:sz")?.first().cloned() {
                    let u = set_tag_attr(
                        &set_tag_attr(sz.full(&inner), "width", &w.to_string()),
                        "height",
                        &h.to_string(),
                    );
                    inner = splice_span(&inner, &sz, &u);
                }
            }
            return Ok(format!("<hp:pic{pic_attrs}>{inner}</hp:pic>"));
        }
    }

    let w = width.unwrap_or(28350);
    let h = height.unwrap_or(28350);
    Ok(format!(
        r#"<hp:pic id="{}" zOrder="0" numberingType="PICTURE" textWrap="TOP_AND_BOTTOM" textFlow="BOTH_SIDES" lock="0" dropcapstyle="None" href="" groupLevel="0" instid="{}" reverse="0"><hp:offset x="0" y="0"/><hp:orgSz width="{w}" height="{h}"/><hp:curSz width="{w}" height="{h}"/><hp:flip horizontal="0" vertical="0"/><hp:rotationInfo angle="0" centerX="0" centerY="0" rotateimage="1"/><hp:renderingInfo><hc:transMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/><hc:scaMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/><hc:rotMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/></hp:renderingInfo><hp:imgRect><hc:pt0 x="0" y="0"/><hc:pt1 x="{w}" y="0"/><hc:pt2 x="{w}" y="{h}"/><hc:pt3 x="0" y="{h}"/></hp:imgRect><hp:imgClip left="0" right="{w}" top="0" bottom="{h}"/><hp:inMargin left="0" right="0" top="0" bottom="0"/><hp:imgDim dimwidth="{w}" dimheight="{h}"/><hc:img binaryItemIDRef="{item_id}" bright="0" contrast="0" effect="REAL_PIC" alpha="0"/><hp:effects/><hp:sz width="{w}" widthRelTo="ABSOLUTE" height="{h}" heightRelTo="ABSOLUTE" protect="0"/><hp:pos treatAsChar="1" affectLSpacing="0" flowWithText="1" allowOverlap="0" holdAnchorAndSO="0" vertRelTo="PARA" horzRelTo="PARA" vertAlign="TOP" horzAlign="LEFT" vertOffset="0" horzOffset="0"/><hp:outMargin left="0" right="0" top="0" bottom="0"/><hp:shapeComment>inserted image</hp:shapeComment></hp:pic>"#,
        fresh_id(),
        fresh_id()
    ))
}

pub fn op_insert_image(
    doc: &mut HwpxDocument,
    path: &str,
    _anchor_text: Option<&str>,
    width_hu: Option<HwpUnit>,
    height_hu: Option<HwpUnit>,
    cell: Option<&ImageCellTarget>,
) -> Result<(), HwpxError> {
    let (_, item_id, _) = embed_image_binary(doc, path)?;

    if let Some(target) = cell {
        let cell_obj_margin = (1.4 * HWPUNIT_PER_MM).round() as i32;
        let loc = doc.get_table(target.table_index)?;
        let sec_xml = doc.archive.read_text_ro(&loc.section)?;
        let tbl_inner = loc.span.inner(&sec_xml);
        let rows = scan_top_level(tbl_inner, "hp:tr")?;
        let rw = rows.get(target.row).ok_or_else(|| {
            HwpxError::OutOfRange(format!("set_cell_image: row {} out of range", target.row))
        })?;
        let r_in = rw.inner(tbl_inner);
        let tcs = scan_top_level(r_in, "hp:tc")?;
        let tc_idx = find_cell_index_in_row(r_in, &tcs, target.col).ok_or_else(|| {
            HwpxError::OutOfRange(format!("set_cell_image: col {} out of range", target.col))
        })?;
        let tc = &tcs[tc_idx];
        let has_margin = get_attr(tc.attrs(r_in, "hp:tc"), "hasMargin").as_deref() == Some("1");
        if !has_margin {
            op_set_cell_padding(
                doc,
                target.table_index,
                target.row,
                target.col,
                Some(cell_obj_margin),
                Some(cell_obj_margin),
                None,
                None,
            )?;
        }

        let mut w = width_hu.unwrap_or_else(|| (30.0 * HWPUNIT_PER_MM).round() as i32);
        let mut h = height_hu.unwrap_or_else(|| (20.0 * HWPUNIT_PER_MM).round() as i32);
        if let Some(cw) = cell_content_width(doc, target.table_index, target.row, target.col) {
            if w > cw {
                h = ((h as f64) * (cw as f64 / w as f64)).round().max(1.0) as i32;
                w = cw;
            }
        }
        let mut pic = build_pic_xml(doc, &item_id, Some(w), Some(h))?;
        if let Some(pos) = scan_top_level(&pic, "hp:pos")?.first().cloned() {
            let u = set_tag_attr(pos.full(&pic), "treatAsChar", "1");
            pic = splice_span(&pic, &pos, &u);
        }
        if let Some(om) = scan_top_level(&pic, "hp:outMargin")?.first().cloned() {
            pic = splice_span(
                &pic,
                &om,
                &format!(
                    r#"<hp:outMargin left="0" right="0" top="{cell_obj_margin}" bottom="{cell_obj_margin}"/>"#
                ),
            );
        }

        // Place inline object into the cell's first paragraph
        let loc2 = doc.get_table(target.table_index)?;
        let sec_xml2 = doc.archive.read_text(&loc2.section)?;
        let tbl_inner2 = loc2.span.inner(&sec_xml2);
        let rows2 = scan_top_level(tbl_inner2, "hp:tr")?;
        let rw2 = &rows2[target.row];
        let r_in2 = rw2.inner(tbl_inner2);
        let tcs2 = scan_top_level(r_in2, "hp:tc")?;
        let tc_idx2 = find_cell_index_in_row(r_in2, &tcs2, target.col).ok_or_else(|| {
            HwpxError::OutOfRange(format!("set_cell_image: col {} out of range", target.col))
        })?;
        let tc2 = &tcs2[tc_idx2];
        let tc_in2 = tc2.inner(r_in2);
        let sub = scan_top_level(tc_in2, "hp:subList")?.into_iter().next().ok_or_else(|| {
            HwpxError::InvalidArgument("set_cell_image: cell has no <hp:subList>".to_string())
        })?;
        let p = scan_top_level(sub.inner(tc_in2), "hp:p")?
            .into_iter()
            .next()
            .ok_or_else(|| {
                HwpxError::InvalidArgument("set_cell_image: cell has no <hp:p>".to_string())
            })?;
        let p_in = p.inner(sub.inner(tc_in2));
        let cpr = first_char_pr_id(p_in);
        let new_p = format!(
            "<hp:p{}>{p_in}<hp:run charPrIDRef=\"{cpr}\">{pic}</hp:run></hp:p>",
            p.attrs(sub.inner(tc_in2), "hp:p"),
        );
        let new_sub = format!(
            "<hp:subList{}>{}</hp:subList>",
            sub.attrs(tc_in2, "hp:subList"),
            splice_span(sub.inner(tc_in2), &p, &new_p)
        );
        let new_tc = format!(
            "<hp:tc{}>{}</hp:tc>",
            tc2.attrs(r_in2, "hp:tc"),
            splice_span(tc_in2, &sub, &new_sub)
        );
        let new_r = format!(
            "<hp:tr{}>{}</hp:tr>",
            rw2.attrs(tbl_inner2, "hp:tr"),
            splice_span(r_in2, tc2, &new_tc)
        );
        let new_tbl = format!(
            "<hp:tbl{}>{}</hp:tbl>",
            loc2.span.attrs(&sec_xml2, "hp:tbl"),
            splice_span(tbl_inner2, rw2, &new_r)
        );
        doc.archive
            .write_text(loc2.section, splice_span(&sec_xml2, &loc2.span, &new_tbl));

        op_set_cell_align(
            doc,
            target.table_index,
            target.row,
            target.col,
            None,
            None,
            Some(CellHAlign::Center),
            Some(CellVAlign::Center),
        )?;
        return Ok(());
    }

    let pic = build_pic_xml(doc, &item_id, width_hu, height_hu)?;
    let names = doc.archive.section_names();
    let last = names
        .last()
        .ok_or_else(|| HwpxError::MissingPart("Contents/section0.xml".to_string()))?
        .clone();
    let mut xml = doc.archive.read_text(&last)?;
    let last_cpr = scan_top_level(&xml, "hp:p")?
        .last()
        .map(|p| first_char_pr_id(p.inner(&xml)))
        .unwrap_or_else(|| "0".to_string());
    let para = format!(
        r#"<hp:p id="{}" paraPrIDRef="0" styleIDRef="0" pageBreak="0" columnBreak="0" merged="0"><hp:run charPrIDRef="{last_cpr}">{pic}</hp:run></hp:p>"#,
        fresh_id()
    );
    if let Some(close) = xml.rfind("</hs:sec>") {
        xml.insert_str(close, &para);
    } else {
        xml.push_str(&para);
    }
    doc.archive.write_text(last, xml);
    Ok(())
}

fn image_aspect_wh(b: &[u8]) -> f64 {
    if b.len() > 24 && b[0] == 0x89 && b[1] == 0x50 {
        let w = u32::from_be_bytes([b[16], b[17], b[18], b[19]]);
        let h = u32::from_be_bytes([b[20], b[21], b[22], b[23]]);
        if w > 0 && h > 0 {
            return w as f64 / h as f64;
        }
    } else if b.len() > 4 && b[0] == 0xff && b[1] == 0xd8 {
        let mut i = 2usize;
        while i + 9 < b.len() {
            if b[i] != 0xff {
                i += 1;
                continue;
            }
            let m = b[i + 1];
            if (0xc0..=0xcf).contains(&m) && m != 0xc4 && m != 0xc8 && m != 0xcc {
                let h = u16::from_be_bytes([b[i + 5], b[i + 6]]);
                let w = u16::from_be_bytes([b[i + 7], b[i + 8]]);
                if w > 0 && h > 0 {
                    return w as f64 / h as f64;
                }
            }
            let seg_len = u16::from_be_bytes([b[i + 2], b[i + 3]]) as usize;
            if seg_len < 2 {
                break;
            }
            i += 2 + seg_len;
        }
    } else if b.len() > 26 && b[0] == 0x42 && b[1] == 0x4d {
        let w = i32::from_le_bytes([b[18], b[19], b[20], b[21]]).abs();
        let h = i32::from_le_bytes([b[22], b[23], b[24], b[25]]).abs();
        if w > 0 && h > 0 {
            return w as f64 / h as f64;
        }
    }
    1.0
}

fn est_text_width_mm(s: &str, pt: f64) -> f64 {
    let em = pt * PT_TO_MM;
    s.chars()
        .map(|ch| {
            let c = ch as u32;
            let wide = (0x1100..=0x11FF).contains(&c)
                || (0x3000..=0x303F).contains(&c)
                || (0x3130..=0x318F).contains(&c)
                || (0x4E00..=0x9FFF).contains(&c)
                || (0xAC00..=0xD7A3).contains(&c)
                || (0xFF00..=0xFFEF).contains(&c);
            if wide { em } else { em * 0.5 }
        })
        .sum()
}

pub fn op_place_seal(
    doc: &mut HwpxDocument,
    anchor_text: &str,
    seal_image_path: &str,
    mode: SealMode,
) -> Result<(), HwpxError> {
    if anchor_text.is_empty() {
        return Err(HwpxError::InvalidArgument(
            "place_seal: anchor_text is required".to_string(),
        ));
    }
    let (_, item_id, img_bytes) = embed_image_binary(doc, seal_image_path)?;
    let aspect = image_aspect_wh(&img_bytes);
    let sense_mm = |pt: f64, ov: bool, box_w: Option<f64>, room_r: Option<f64>| -> f64 {
        let mut h = pt * PT_TO_MM * 1.3 * 1.6;
        if !ov {
            if let Some(rr) = room_r {
                h = h.min(rr / aspect);
            }
        } else if let Some(bw) = box_w {
            h = h.min((bw * 0.95) / aspect);
        }
        (h.clamp(7.0, 18.0) * 10.0).round() / 10.0
    };

    let build_seal_run = |doc: &HwpxDocument, seal_mm: f64, dx_mm: f64, dy_mm: f64, frame: &str| -> Result<String, HwpxError> {
        let h_hu = (seal_mm * HWPUNIT_PER_MM).round() as i32;
        let w_hu = (seal_mm * aspect * HWPUNIT_PER_MM).round() as i32;
        let mut pic = build_pic_xml(doc, &item_id, Some(w_hu), Some(h_hu))?;
        if let Some((start_end, _)) = pic.find('>').map(|i| (i, &pic[..i])) {
            let tag_end = start_end;
            let open_attrs = &pic[7..tag_end];
            let new_attrs = set_or_add_attr(open_attrs, "textWrap", "IN_FRONT_OF_TEXT");
            pic = format!("<hp:pic{new_attrs}>{}", &pic[tag_end + 1..]);
        }
        if let Some(pos) = scan_top_level(&pic, "hp:pos")?.first().cloned() {
            let mut u = pos.full(&pic).to_string();
            u = set_tag_attr(&u, "treatAsChar", "0");
            u = set_tag_attr(&u, "flowWithText", "0");
            u = set_tag_attr(&u, "allowOverlap", "1");
            u = set_tag_attr(&u, "vertRelTo", frame);
            u = set_tag_attr(&u, "horzRelTo", frame);
            u = set_tag_attr(&u, "horzOffset", &((dx_mm * HWPUNIT_PER_MM).round() as i32).to_string());
            u = set_tag_attr(&u, "vertOffset", &((dy_mm * HWPUNIT_PER_MM).round() as i32).to_string());
            pic = splice_span(&pic, &pos, &u);
        }
        Ok(format!(r#"<hp:run charPrIDRef="0">{pic}</hp:run>"#))
    };

    // Search top-level paragraphs first, then tables (first document-order match).
    for name in doc.archive.section_names() {
        let xml = doc.archive.read_text(&name)?;
        let room_w_mm = doc.page_text_width(&name) as f64 / HWPUNIT_PER_MM;
        let blocks = scan_top_level(&xml, "hp:p")?;
        for b in &blocks {
            let b_in = b.inner(&xml);
            if b_in.contains("<hp:tbl") {
                continue;
            }
            let txt = extract_t_text(b_in)?;
            if !txt.contains(anchor_text) {
                continue;
            }
            let pt = doc.char_pr_font_pt(Some(&first_char_pr_id(b_in)));
            let idx = txt.find(anchor_text).unwrap_or(txt.len());
            let start_x = est_text_width_mm(&txt[..idx], pt);
            let aw = est_text_width_mm(anchor_text, pt);
            let room_r = (room_w_mm - (start_x + aw)).max(0.0);
            let prov = sense_mm(pt, false, None, None);
            let ov = match mode {
                SealMode::Overlap => true,
                SealMode::Right => false,
                SealMode::Auto => room_r < prov * aspect + 2.0,
            };
            let seal_mm = sense_mm(pt, ov, None, Some(room_r));
            let w_mm = seal_mm * aspect;
            let dx = if ov {
                start_x + aw / 2.0 - w_mm / 2.0
            } else {
                start_x + aw + 2.0
            };
            let dy = pt * PT_TO_MM * 0.2 - seal_mm / 2.0;
            let run = build_seal_run(doc, seal_mm, dx, dy, "PAGE")?;
            let rebuilt = format!("<hp:p{}>{b_in}{run}</hp:p>", b.attrs(&xml, "hp:p"));
            doc.archive
                .write_text(name, drop_linesegs(&splice_span(&xml, b, &rebuilt))?);
            return Ok(());
        }

        // Check tables in this section
        let tbls = scan_top_level(&xml, "hp:tbl")?;
        for tbl in &tbls {
            let tbl_in = tbl.inner(&xml);
            if !extract_t_text(tbl_in)?.contains(anchor_text) {
                continue;
            }
            let rows = scan_top_level(tbl_in, "hp:tr")?;
            for rw in &rows {
                let r_in = rw.inner(tbl_in);
                for tc in scan_top_level(r_in, "hp:tc")? {
                    let tc_in = tc.inner(r_in);
                    let txt = extract_t_text(tc_in)?;
                    if !txt.contains(anchor_text) {
                        continue;
                    }
                    let sub = scan_top_level(tc_in, "hp:subList")?.into_iter().next().ok_or_else(|| {
                        HwpxError::InvalidArgument("place_seal: cell has no <hp:subList>".to_string())
                    })?;
                    let sub_in = sub.inner(tc_in);
                    let p = scan_top_level(sub_in, "hp:p")?.into_iter().next().ok_or_else(|| {
                        HwpxError::InvalidArgument("place_seal: cell has no <hp:p>".to_string())
                    })?;
                    let p_in = p.inner(sub_in);
                    let pt = doc.char_pr_font_pt(Some(&first_char_pr_id(p_in)));
                    let p_txt = extract_t_text(p_in)?;
                    let idx = p_txt.find(anchor_text).unwrap_or(p_txt.len());
                    let start_x = est_text_width_mm(&p_txt[..idx], pt);
                    let aw = est_text_width_mm(anchor_text, pt);
                    let ov = matches!(mode, SealMode::Overlap);
                    let seal_mm = sense_mm(pt, ov, None, None);
                    let w_mm = seal_mm * aspect;
                    let dx = if ov {
                        start_x + aw / 2.0 - w_mm / 2.0
                    } else {
                        start_x + aw + 2.0
                    };
                    let dy = pt * PT_TO_MM * (-0.08) - seal_mm / 2.0;
                    let run = build_seal_run(doc, seal_mm, dx, dy, "PARA")?;
                    let new_p = format!("<hp:p{}>{p_in}{run}</hp:p>", p.attrs(sub_in, "hp:p"));
                    let new_sub = format!(
                        "<hp:subList{}>{}</hp:subList>",
                        sub.attrs(tc_in, "hp:subList"),
                        splice_span(sub_in, &p, &new_p)
                    );
                    let new_tc = format!(
                        "<hp:tc{}>{}</hp:tc>",
                        tc.attrs(r_in, "hp:tc"),
                        splice_span(tc_in, &sub, &new_sub)
                    );
                    let new_r = format!(
                        "<hp:tr{}>{}</hp:tr>",
                        rw.attrs(tbl_in, "hp:tr"),
                        splice_span(r_in, &tc, &new_tc)
                    );
                    let new_tbl = format!(
                        "<hp:tbl{}>{}</hp:tbl>",
                        tbl.attrs(&xml, "hp:tbl"),
                        splice_span(tbl_in, rw, &new_r)
                    );
                    doc.archive.write_text(
                        name,
                        drop_linesegs(&splice_span(&xml, tbl, &new_tbl))?,
                    );
                    return Ok(());
                }
            }
        }
    }

    Err(HwpxError::InvalidArgument(format!(
        "place_seal: anchor \"{anchor_text}\" not found"
    )))
}

pub fn op_set_header_footer(
    doc: &mut HwpxDocument,
    location: HeaderFooterLocation,
    text: &str,
) -> Result<(), HwpxError> {
    let tag = match location {
        HeaderFooterLocation::Header => "hp:header",
        HeaderFooterLocation::Footer => "hp:footer",
    };

    for name in doc.archive.section_names() {
        let xml = doc.archive.read_text(&name)?;
        let els = scan_top_level(&xml, tag)?;
        if let Some(el) = els.first() {
            let inner = crate::ops::table::set_cell_inner_text(el.inner(&xml), text, false)?;
            let attrs = set_or_add_attr(el.attrs(&xml, tag), "applyPageType", "BOTH");
            let replacement = format!("<{tag}{attrs}>{inner}</{tag}>");
            doc.archive
                .write_text(name, drop_linesegs(&splice_span(&xml, el, &replacement))?);
            return Ok(());
        }
    }

    let first_sec = doc
        .archive
        .section_names()
        .into_iter()
        .next()
        .ok_or_else(|| HwpxError::MissingPart("Contents/section0.xml".to_string()))?;
    let mut xml = doc.archive.read_text(&first_sec)?;
    let paras = scan_top_level(&xml, "hp:p")?;
    let first_p = paras.first().ok_or_else(|| {
        HwpxError::InvalidArgument(format!("{tag}: no <hp:p> in first section"))
    })?;
    let wrapper = format!(
        r#"<hp:p id="{}" paraPrIDRef="0" styleIDRef="0" pageBreak="0" columnBreak="0" merged="0"><hp:run charPrIDRef="0"><hp:ctrl><{tag} id="0" applyPageType="BOTH"><hp:subList id="" textDirection="HORIZONTAL" lineWrap="BREAK" vertAlign="TOP" linkListIDRef="0" linkListNextIDRef="0" textWidth="0" textHeight="0" hasTextRef="0" hasNumRef="0"><hp:p id="0" paraPrIDRef="0" styleIDRef="0" pageBreak="0" columnBreak="0" merged="0"><hp:run charPrIDRef="0"><hp:t>{}</hp:t></hp:run></hp:p></hp:subList></{tag}></hp:ctrl></hp:run></hp:p>"#,
        fresh_id(),
        xml_escape_text(text)
    );
    xml.insert_str(first_p.end, &wrapper);
    doc.archive.write_text(first_sec, drop_linesegs(&xml)?);
    Ok(())
}

pub fn op_insert_page_number(
    doc: &mut HwpxDocument,
    location: HeaderFooterLocation,
    align: PageNumberAlign,
) -> Result<(), HwpxError> {
    let (tag, vert) = match location {
        HeaderFooterLocation::Header => ("hp:header", "TOP"),
        HeaderFooterLocation::Footer => ("hp:footer", "BOTTOM"),
    };
    let al_str = match align {
        PageNumberAlign::Left => "LEFT",
        PageNumberAlign::Center => "CENTER",
        PageNumberAlign::Right => "RIGHT",
    };
    let ppr_id = ensure_clean_para_pr(doc, Some(al_str), None, None, None, None, None, None)?
        .unwrap_or_else(|| "0".to_string());

    let first_sec = doc
        .archive
        .section_names()
        .into_iter()
        .next()
        .ok_or_else(|| HwpxError::MissingPart("Contents/section0.xml".to_string()))?;
    let mut xml = doc.archive.read_text(&first_sec)?;
    let paras = scan_top_level(&xml, "hp:p")?;
    let first_p = paras.first().ok_or_else(|| {
        HwpxError::InvalidArgument("insert_page_number: no <hp:p> in first section".to_string())
    })?;
    let auto_num = r#"<hp:ctrl><hp:autoNum num="1" numType="PAGE"><hp:autoNumFormat type="DIGIT" userChar="" prefixChar="" suffixChar="" supscript="0"/></hp:autoNum></hp:ctrl><hp:t/>"#;
    let wrapper = format!(
        r#"<hp:p id="{}" paraPrIDRef="0" styleIDRef="0" pageBreak="0" columnBreak="0" merged="0"><hp:run charPrIDRef="0"><hp:ctrl><{tag} id="0" applyPageType="BOTH"><hp:subList id="" textDirection="HORIZONTAL" lineWrap="BREAK" vertAlign="{vert}" linkListIDRef="0" linkListNextIDRef="0" textWidth="0" textHeight="0" hasTextRef="0" hasNumRef="0"><hp:p id="0" paraPrIDRef="{ppr_id}" styleIDRef="0" pageBreak="0" columnBreak="0" merged="0"><hp:run charPrIDRef="0">{auto_num}</hp:run></hp:p></hp:subList></{tag}></hp:ctrl></hp:run></hp:p>"#,
        fresh_id()
    );
    xml.insert_str(first_p.end, &wrapper);
    doc.archive.write_text(first_sec, drop_linesegs(&xml)?);
    Ok(())
}

pub fn op_insert_footnote(
    doc: &mut HwpxDocument,
    anchor_text: &str,
    text: &str,
    para_index: Option<isize>,
) -> Result<(), HwpxError> {
    let paras = doc.paragraphs()?;
    if paras.is_empty() {
        return Err(HwpxError::OutOfRange(
            "insert_footnote: document has no paragraphs".to_string(),
        ));
    }
    let mut target_idx: Option<usize> = None;
    if !anchor_text.is_empty() {
        for (i, p) in paras.iter().enumerate() {
            let xml = doc.archive.read_text_ro(&p.section)?;
            if extract_t_text(p.span.inner(&xml))?.contains(anchor_text) {
                target_idx = Some(i);
                break;
            }
        }
    }
    let idx = match target_idx {
        Some(i) => i,
        None => match para_index {
            None | Some(-1) => paras.len() - 1,
            Some(n) if n < -1 => {
                return Err(HwpxError::OutOfRange(format!(
                    "insert_footnote: index {n} out of range"
                )))
            }
            Some(n) => (n as usize).min(paras.len() - 1),
        },
    };
    let loc = &paras[idx];
    let xml = doc.archive.read_text(&loc.section)?;
    let p_inner = loc.span.inner(&xml);
    let cpr = first_char_pr_id(p_inner);
    let note_run = format!(
        r#"<hp:run charPrIDRef="{cpr}"><hp:ctrl><hp:footNote id="0"><hp:subList id="" textDirection="HORIZONTAL" lineWrap="BREAK" vertAlign="TOP" linkListIDRef="0" linkListNextIDRef="0" textWidth="0" textHeight="0" hasTextRef="0" hasNumRef="0"><hp:p id="0" paraPrIDRef="0" styleIDRef="0" pageBreak="0" columnBreak="0" merged="0"><hp:run charPrIDRef="0"><hp:t>{}</hp:t></hp:run></hp:p></hp:subList></hp:footNote></hp:ctrl></hp:run>"#,
        xml_escape_text(text)
    );
    let rebuilt = format!("<hp:p{}>{p_inner}{note_run}</hp:p>", loc.span.attrs(&xml, "hp:p"));
    doc.archive.write_text(
        loc.section.clone(),
        drop_linesegs(&splice_span(&xml, &loc.span, &rebuilt))?,
    );
    Ok(())
}

pub fn op_set_columns(
    doc: &mut HwpxDocument,
    count: u8,
    spacing_hu: Option<HwpUnit>,
) -> Result<(), HwpxError> {
    if count < 1 {
        return Err(HwpxError::InvalidArgument(
            "set_columns: count must be >= 1".to_string(),
        ));
    }
    let gap = if count > 1 {
        spacing_hu.unwrap_or(1134)
    } else {
        0
    };
    let mut changed = 0usize;
    for name in doc.archive.section_names() {
        let mut xml = doc.archive.read_text(&name)?;
        let cols = scan_top_level(&xml, "hp:colPr")?;
        if let Some(col_pr) = cols.first() {
            let mut u = set_tag_attr(col_pr.full(&xml), "colCount", &count.to_string());
            u = set_tag_attr(&u, "sameSz", "1");
            u = set_tag_attr(&u, "sameGap", &gap.to_string());
            xml = splice_span(&xml, col_pr, &u);
            doc.archive.write_text(name, xml);
            changed += 1;
        }
    }
    if changed == 0 {
        return Err(HwpxError::MissingPart(
            "set_columns: no <hp:colPr> found in sections".to_string(),
        ));
    }
    Ok(())
}

pub fn op_set_page_margin(
    doc: &mut HwpxDocument,
    left_hu: Option<HwpUnit>,
    right_hu: Option<HwpUnit>,
    top_hu: Option<HwpUnit>,
    bottom_hu: Option<HwpUnit>,
) -> Result<(), HwpxError> {
    let mut changed = 0usize;
    for name in doc.archive.section_names() {
        let mut xml = doc.archive.read_text(&name)?;
        if let Some(mg) = scan_top_level(&xml, "hp:margin")?.first().cloned() {
            let mut u = mg.full(&xml).to_string();
            if let Some(v) = left_hu {
                u = set_tag_attr(&u, "left", &v.to_string());
            }
            if let Some(v) = right_hu {
                u = set_tag_attr(&u, "right", &v.to_string());
            }
            if let Some(v) = top_hu {
                u = set_tag_attr(&u, "top", &v.to_string());
            }
            if let Some(v) = bottom_hu {
                u = set_tag_attr(&u, "bottom", &v.to_string());
            }
            xml = splice_span(&xml, &mg, &u);
            doc.archive.write_text(name, xml);
            changed += 1;
        }
    }
    if changed == 0 {
        return Err(HwpxError::MissingPart(
            "set_page_margin: no <hp:margin> found in sections".to_string(),
        ));
    }
    let _ = xml_escape_attr("");
    let _ = run_with_text;
    Ok(())
}
