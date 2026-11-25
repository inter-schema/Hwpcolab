use hopclaw_types::{
    BorderLineType, BorderSide, CellHAlign, CellVAlign, ColInsertPosition, EqualizeMode, HwpUnit,
    NestedTableWidthMode, RowInsertPosition, HWPUNIT_PER_MM,
};

use crate::error::HwpxError;
use crate::model::{
    cell_tail, first_char_pr_id, format_hu_as_mm, norm_hex, read_cell_addr, read_cell_size,
    read_cell_span, run_with_text, HwpxDocument, DEFAULT_DIAG, STD_SLASH_NONE,
};
use crate::ops::text::{op_apply_text_style, TextStyleOpts};
use crate::xml_util::{
    drop_linesegs, extract_t_text, fresh_id, freshen_ids, get_attr, scan_top_level,
    set_or_add_attr, set_tag_attr, splice_span, sync_item_count, XmlElementSpan,
};

const FALLBACK_TBL_ATTRS: &str = r#" id="0" zOrder="0" numberingType="TABLE" textWrap="TOP_AND_BOTTOM" textFlow="BOTH_SIDES" lock="0" dropcapstyle="None" pageBreak="CELL" repeatHeader="0" cellSpacing="0" borderFillIDRef="1" noAdjust="0""#;
const FALLBACK_TBL_META: &str = r#"<hp:sz width="42520" widthRelTo="ABSOLUTE" height="4150" heightRelTo="ABSOLUTE" protect="0"/><hp:pos treatAsChar="1" affectLSpacing="0" flowWithText="0" allowOverlap="0" holdAnchorAndSO="0" vertRelTo="PARA" horzRelTo="PARA" vertAlign="TOP" horzAlign="LEFT" vertOffset="0" horzOffset="0"/><hp:outMargin left="286" right="286" top="286" bottom="286"/><hp:inMargin left="510" right="510" top="138" bottom="138"/>"#;
const FALLBACK_CELL_ATTRS: &str =
    r#" name="" header="0" hasMargin="0" protect="0" editable="0" dirty="0" borderFillIDRef="1""#;
const FALLBACK_CELL_INNER: &str = r#"<hp:subList id="" textDirection="HORIZONTAL" lineWrap="BREAK" vertAlign="CENTER" linkListIDRef="0" linkListNextIDRef="0" textWidth="0" textHeight="0" hasTextRef="0" hasNumRef="0"><hp:p id="0" paraPrIDRef="0" styleIDRef="0" pageBreak="0" columnBreak="0" merged="0"><hp:run charPrIDRef="0"><hp:t></hp:t></hp:run></hp:p></hp:subList><hp:cellAddr colAddr="0" rowAddr="0"/><hp:cellSpan colSpan="1" rowSpan="1"/><hp:cellSz width="5155" height="2075"/><hp:cellMargin left="0" right="0" top="0" bottom="0"/>"#;

fn fit_value_into_layout(orig: &str, value: &str) -> String {
    let mut best: Option<(usize, usize)> = None;
    let bytes = orig.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b' ' {
            let start = i;
            while i < bytes.len() && bytes[i] == b' ' {
                i += 1;
            }
            let len = i - start;
            if len >= 2 && best.map(|b| len > b.1).unwrap_or(true) {
                best = Some((start, len));
            }
        } else {
            i += 1;
        }
    }
    let Some((idx, len)) = best else {
        return value.to_string();
    };
    let val_chars = value.chars().count();
    if len < val_chars {
        return value.to_string();
    }
    let keep = usize::from(len > val_chars);
    let prefix = &orig[..idx + keep];
    let suffix_start = (idx + keep + val_chars).min(orig.len());
    format!("{prefix}{value}{}", &orig[suffix_start..])
}

pub fn set_cell_inner_text(tc_inner: &str, text: &str, fit: bool) -> Result<String, HwpxError> {
    let subs = scan_top_level(tc_inner, "hp:subList")?;
    let Some(sub) = subs.first() else {
        return Ok(tc_inner.to_string());
    };
    let sub_inner = sub.inner(tc_inner);
    let ps = scan_top_level(sub_inner, "hp:p")?;
    let Some(p) = ps.first() else {
        return Ok(tc_inner.to_string());
    };
    let p_inner = p.inner(sub_inner);

    let effective_text = if fit {
        let has_inline = [
            "<hp:ctrl",
            "<hp:pic",
            "<hp:tbl",
            "<hp:chart",
            "<hp:equation",
            "<hp:rect",
            "<hp:ellipse",
            "<hp:line",
            "<hp:arc",
            "<hp:polygon",
            "<hp:curve",
            "<hp:connectLine",
            "<hp:drawText",
        ]
        .iter()
        .any(|pat| p_inner.contains(pat));
        if has_inline {
            text.to_string()
        } else {
            let orig = extract_t_text(p_inner)?;
            fit_value_into_layout(&orig, text)
        }
    } else {
        text.to_string()
    };

    let char_pr_id = first_char_pr_id(p_inner);
    let p_attrs = p.attrs(sub_inner, "hp:p");
    let new_p = format!("<hp:p{p_attrs}>{}</hp:p>", run_with_text(&char_pr_id, &effective_text));
    let new_sub_inner = splice_span(sub_inner, p, &new_p);
    let sub_attrs = sub.attrs(tc_inner, "hp:subList");
    let new_sub = format!("<hp:subList{sub_attrs}>{new_sub_inner}</hp:subList>");
    Ok(splice_span(tc_inner, sub, &new_sub))
}

fn find_cell_index_in_row(row_inner: &str, tcs: &[XmlElementSpan], col: usize) -> Option<usize> {
    for (idx, tc) in tcs.iter().enumerate() {
        let tc_inner = tc.inner(row_inner);
        let tail = cell_tail(tc_inner);
        let (addr_col, _) = read_cell_addr(tail, usize::MAX, 0);
        if addr_col == col {
            return Some(idx);
        }
    }
    if col < tcs.len() {
        Some(col)
    } else {
        None
    }
}

pub fn op_set_cell_text(
    doc: &mut HwpxDocument,
    table_index: usize,
    row: usize,
    col: usize,
    text: &str,
    fit: bool,
) -> Result<(), HwpxError> {
    let loc = doc.get_table(table_index)?;
    let sec_xml = doc.archive.read_text(&loc.section)?;
    let tbl_inner = loc.span.inner(&sec_xml);
    let rows = scan_top_level(tbl_inner, "hp:tr")?;
    let row_span = rows.get(row).ok_or_else(|| {
        HwpxError::OutOfRange(format!(
            "set_cell_text: row {row} out of range (0..{})",
            rows.len().saturating_sub(1)
        ))
    })?;
    let row_inner = row_span.inner(tbl_inner);
    let tcs = scan_top_level(row_inner, "hp:tc")?;
    let tc_idx = find_cell_index_in_row(row_inner, &tcs, col).ok_or_else(|| {
        HwpxError::OutOfRange(format!(
            "set_cell_text: col {col} out of range (0..{})",
            tcs.len().saturating_sub(1)
        ))
    })?;
    let tc = &tcs[tc_idx];
    let new_tc_inner = set_cell_inner_text(tc.inner(row_inner), text, fit)?;
    let new_tc = format!("<hp:tc{}>{new_tc_inner}</hp:tc>", tc.attrs(row_inner, "hp:tc"));
    let new_row_inner = splice_span(row_inner, tc, &new_tc);
    let new_row = format!("<hp:tr{}>{new_row_inner}</hp:tr>", row_span.attrs(tbl_inner, "hp:tr"));
    let new_tbl_inner = splice_span(tbl_inner, row_span, &new_row);
    let new_tbl = format!(
        "<hp:tbl{}>{new_tbl_inner}</hp:tbl>",
        loc.span.attrs(&sec_xml, "hp:tbl")
    );
    let updated = drop_linesegs(&splice_span(&sec_xml, &loc.span, &new_tbl))?;
    doc.archive.write_text(loc.section, updated);
    Ok(())
}

fn set_tc_child_tag_attr(
    tc_inner: &str,
    tag: &str,
    attr: &str,
    value: &str,
) -> Result<String, HwpxError> {
    let spans = scan_top_level(tc_inner, tag)?;
    let Some(last) = spans.last() else {
        return Ok(tc_inner.to_string());
    };
    let updated_tag = set_tag_attr(last.full(tc_inner), attr, value);
    Ok(splice_span(tc_inner, last, &updated_tag))
}

fn set_tbl_count_attr(tbl_attrs: &str, attr: &str, val: usize) -> String {
    set_or_add_attr(tbl_attrs, attr, &val.to_string())
}

fn bump_tbl_count_attr(tbl_attrs: &str, attr: &str, delta: isize) -> String {
    let cur = get_attr(tbl_attrs, attr)
        .and_then(|s| s.parse::<isize>().ok())
        .unwrap_or(0);
    let next = (cur + delta).max(0) as usize;
    set_or_add_attr(tbl_attrs, attr, &next.to_string())
}

pub fn cell_content_width(
    doc: &HwpxDocument,
    table_index: usize,
    row: usize,
    col: usize,
) -> Option<i32> {
    let loc = doc.get_table(table_index).ok()?;
    let sec_xml = doc.archive.read_text_ro(&loc.section).ok()?;
    let tbl_inner = loc.span.inner(&sec_xml);
    let rows = scan_top_level(tbl_inner, "hp:tr").ok()?;
    let rw = rows.get(row)?;
    let row_inner = rw.inner(tbl_inner);
    let tcs = scan_top_level(row_inner, "hp:tc").ok()?;
    let tc_idx = find_cell_index_in_row(row_inner, &tcs, col)?;
    let tc = &tcs[tc_idx];
    let tc_inner = tc.inner(row_inner);
    let tail = cell_tail(tc_inner);
    let (w, _) = read_cell_size(tail)?;
    if w <= 100 {
        return None;
    }
    let has_margin = get_attr(tc.attrs(row_inner, "hp:tc"), "hasMargin").as_deref() == Some("1");
    let sum_lr = |frag: &str, tag: &str| -> i32 {
        scan_top_level(frag, tag)
            .ok()
            .and_then(|v| v.into_iter().next())
            .map(|s| {
                let attrs = s.attrs(frag, tag);
                let l = get_attr(attrs, "left")
                    .and_then(|v| v.parse::<i32>().ok())
                    .unwrap_or(0);
                let r = get_attr(attrs, "right")
                    .and_then(|v| v.parse::<i32>().ok())
                    .unwrap_or(0);
                l + r
            })
            .unwrap_or(0)
    };
    let lr = if has_margin {
        sum_lr(tail, "hp:cellMargin")
    } else {
        sum_lr(tbl_inner, "hp:inMargin")
    };
    Some((w - lr).max(0))
}

struct BuiltTable {
    tbl_xml: String,
    src_para_attrs: Option<String>,
    src_para_wrapper: Option<(String, String)>,
}

fn build_table_xml(
    doc: &mut HwpxDocument,
    rows: usize,
    cols: usize,
    cells: &[Vec<String>],
    total_width: Option<i32>,
    freshen_table_id: bool,
    header_row: Option<bool>,
) -> Result<BuiltTable, HwpxError> {
    let tables = doc.top_tables()?;
    let (
        src_section,
        src_para_attrs,
        src_para_wrapper,
        template_row_attrs,
        mut src_tbl_attrs,
        mut tbl_meta,
        cell_template_attrs,
        mut cell_template_inner,
    ) = if let Some(src_tbl) = tables.first() {
        let src_sec = src_tbl.section.clone();
        let src_xml = doc.archive.read_text_ro(&src_sec)?;
        let paras = scan_top_level(&src_xml, "hp:p")?;
        let enclosing_p = paras
            .into_iter()
            .find(|p| p.start <= src_tbl.span.start && p.end >= src_tbl.span.end);
        let (p_attrs, p_wrap) = if let Some(sp) = enclosing_p {
            let tbl_off_in_p = src_tbl.span.start - sp.open_end;
            let tbl_len = src_tbl.span.end - src_tbl.span.start;
            let p_inner = sp.inner(&src_xml);
            let before = p_inner[..tbl_off_in_p].to_string();
            let after = p_inner[tbl_off_in_p + tbl_len..].to_string();
            (Some(sp.attrs(&src_xml, "hp:p").to_string()), Some((before, after)))
        } else {
            (None, None)
        };

        let tbl_inner = src_tbl.span.inner(&src_xml);
        let src_rows = scan_top_level(tbl_inner, "hp:tr")?;
        if src_rows.is_empty() {
            return Err(HwpxError::InvalidArgument(
                "insert_table: source table has no rows".to_string(),
            ));
        }
        let template_row = if src_rows.len() >= 2 {
            &src_rows[1]
        } else {
            &src_rows[0]
        };
        let tr_attrs = template_row.attrs(tbl_inner, "hp:tr").to_string();
        let tr_inner = template_row.inner(tbl_inner);
        let src_tcs = scan_top_level(tr_inner, "hp:tc")?;
        let first_tc = src_tcs.first().ok_or_else(|| {
            HwpxError::InvalidArgument("insert_table: source row has no cells".to_string())
        })?;
        let tc_attrs = first_tc.attrs(tr_inner, "hp:tc").to_string();
        let tc_inner = first_tc.inner(tr_inner).to_string();
        let tbl_attrs = src_tbl.span.attrs(&src_xml, "hp:tbl").to_string();

        let first_tr_idx = tbl_inner.find("<hp:tr").unwrap_or(tbl_inner.len());
        let mut meta = tbl_inner[..first_tr_idx].to_string();
        for cap in scan_top_level(&meta, "hp:caption")?.into_iter().rev() {
            meta = splice_span(&meta, &cap, "");
        }

        (src_sec, p_attrs, p_wrap, tr_attrs, tbl_attrs, meta, tc_attrs, tc_inner)
    } else {
        let invisible_sides = concat!(
            r#"<hh:leftBorder type="NONE" width="0.1 mm" color="none"/>"#,
            r#"<hh:rightBorder type="NONE" width="0.1 mm" color="none"/>"#,
            r#"<hh:topBorder type="NONE" width="0.1 mm" color="none"/>"#,
            r#"<hh:bottomBorder type="NONE" width="0.1 mm" color="none"/>"#
        );
        let invisible_tbl_bf =
            doc.ensure_border_fill(&format!("{STD_SLASH_NONE}{invisible_sides}"))?;
        let visible_cell_bf = doc.ensure_border_fill(&format!(
            "{STD_SLASH_NONE}{}",
            crate::model::PLAIN_SIDES
        ))?;
        let sec0 = doc
            .archive
            .section_names()
            .into_iter()
            .next()
            .ok_or_else(|| HwpxError::MissingPart("Contents/section0.xml".to_string()))?;
        (
            sec0,
            None,
            None,
            String::new(),
            set_or_add_attr(FALLBACK_TBL_ATTRS, "borderFillIDRef", &invisible_tbl_bf),
            FALLBACK_TBL_META.to_string(),
            set_or_add_attr(FALLBACK_CELL_ATTRS, "borderFillIDRef", &visible_cell_bf),
            FALLBACK_CELL_INNER.to_string(),
        )
    };

    // Size the new table to fit the page or parent cell.
    let target_w = if let Some(w) = total_width.filter(|&w| w > 0) {
        w
    } else {
        let page_w = doc.page_text_width(&src_section);
        let src_w = scan_top_level(&tbl_meta, "hp:sz")?
            .first()
            .and_then(|s| get_attr(s.attrs(&tbl_meta, "hp:sz"), "width"))
            .and_then(|s| s.parse::<i32>().ok())
            .filter(|&w| w > 0)
            .unwrap_or(page_w);
        src_w.min(page_w)
    };
    let cell_w = (target_w / cols as i32).max(1);
    if let Some(csz) = scan_top_level(&cell_template_inner, "hp:cellSz")?
        .last()
        .cloned()
    {
        let updated = set_tag_attr(csz.full(&cell_template_inner), "width", &cell_w.to_string());
        cell_template_inner = splice_span(&cell_template_inner, &csz, &updated);
    }
    if let Some(sz) = scan_top_level(&tbl_meta, "hp:sz")?.first().cloned() {
        let updated = set_tag_attr(
            sz.full(&tbl_meta),
            "width",
            &(cell_w * cols as i32).to_string(),
        );
        tbl_meta = splice_span(&tbl_meta, &sz, &updated);
    }

    let mut built_rows = String::new();
    for (r, row_cells) in (0..rows).map(|r| (r, cells.get(r))) {
        let mut cell_strs = String::new();
        for c in 0..cols {
            let text = row_cells
                .and_then(|rc| rc.get(c))
                .map(String::as_str)
                .unwrap_or("");
            let mut inner = set_cell_inner_text(&cell_template_inner, text, false)?;
            if let Some(ca) = scan_top_level(&inner, "hp:cellAddr")?.last().cloned() {
                inner = splice_span(
                    &inner,
                    &ca,
                    &format!(r#"<hp:cellAddr colAddr="{c}" rowAddr="{r}"/>"#),
                );
            } else {
                inner = inner.replacen(
                    "</hp:subList>",
                    &format!(r#"</hp:subList><hp:cellAddr colAddr="{c}" rowAddr="{r}"/>"#),
                    1,
                );
            }
            if let Some(cs) = scan_top_level(&inner, "hp:cellSpan")?.last().cloned() {
                inner = splice_span(&inner, &cs, r#"<hp:cellSpan colSpan="1" rowSpan="1"/>"#);
            }
            let mut c_attrs = cell_template_attrs.clone();
            if let Some(hdr) = header_row {
                if r == 0 {
                    c_attrs = set_or_add_attr(&c_attrs, "header", if hdr { "1" } else { "0" });
                }
            }
            let tc = freshen_ids(&format!("<hp:tc{c_attrs}>{inner}</hp:tc>"))?;
            cell_strs.push_str(&tc);
        }
        built_rows.push_str(&format!("<hp:tr{template_row_attrs}>{cell_strs}</hp:tr>"));
    }

    if freshen_table_id {
        src_tbl_attrs = set_or_add_attr(&src_tbl_attrs, "id", &fresh_id());
    }
    src_tbl_attrs = set_tbl_count_attr(&src_tbl_attrs, "rowCnt", rows);
    src_tbl_attrs = set_tbl_count_attr(&src_tbl_attrs, "colCnt", cols);
    let tbl_xml = format!("<hp:tbl{src_tbl_attrs}>{tbl_meta}{built_rows}</hp:tbl>");

    Ok(BuiltTable {
        tbl_xml,
        src_para_attrs,
        src_para_wrapper,
    })
}

pub fn op_insert_table(
    doc: &mut HwpxDocument,
    after_para_index: Option<isize>,
    headers: &[String],
    rows_data: &[Vec<String>],
) -> Result<(), HwpxError> {
    let mut all_cells = Vec::with_capacity(1 + rows_data.len());
    if !headers.is_empty() {
        all_cells.push(headers.to_vec());
    }
    all_cells.extend(rows_data.iter().cloned());
    if all_cells.is_empty() {
        return Err(HwpxError::InvalidArgument(
            "insert_table: table must have at least 1 row".to_string(),
        ));
    }
    let rows = all_cells.len();
    let cols = all_cells.iter().map(|r| r.len()).max().unwrap_or(0).max(1);

    let built = build_table_xml(doc, rows, cols, &all_cells, None, false, None)?;
    let new_para = if let (Some(p_attrs), Some((before, after))) =
        (built.src_para_attrs, built.src_para_wrapper)
    {
        let new_attrs = set_or_add_attr(&p_attrs, "id", &fresh_id());
        let inner = drop_linesegs(&format!("{before}{}{after}", built.tbl_xml))?;
        format!("<hp:p{new_attrs}>{inner}</hp:p>")
    } else {
        format!(
            r#"<hp:p id="{}" paraPrIDRef="0" styleIDRef="0" pageBreak="0" columnBreak="0" merged="0"><hp:run charPrIDRef="0">{}</hp:run></hp:p>"#,
            fresh_id(),
            built.tbl_xml
        )
    };

    let all_paras = doc.paragraphs()?;
    let target_idx: isize = match after_para_index {
        None => {
            if all_paras.is_empty() {
                -1
            } else {
                (all_paras.len() - 1) as isize
            }
        }
        Some(idx) if idx >= all_paras.len() as isize => (all_paras.len() as isize) - 1,
        Some(idx) => idx,
    };
    if target_idx < -1 || target_idx >= all_paras.len() as isize {
        return Err(HwpxError::OutOfRange(format!(
            "insert_table: index {target_idx} out of range (-1..{})",
            all_paras.len() as isize - 1
        )));
    }

    if target_idx == -1 {
        let first_sec = doc
            .archive
            .section_names()
            .into_iter()
            .next()
            .ok_or_else(|| HwpxError::MissingPart("Contents/section0.xml".to_string()))?;
        let mut xml = doc.archive.read_text(&first_sec)?;
        if let Some(first_p) = scan_top_level(&xml, "hp:p")?.first() {
            xml.insert_str(first_p.start, &new_para);
        } else if let Some(close) = xml.rfind("</hs:sec>") {
            xml.insert_str(close, &new_para);
        } else {
            xml.push_str(&new_para);
        }
        doc.archive.write_text(first_sec, xml);
    } else {
        let target = &all_paras[target_idx as usize];
        let mut xml = doc.archive.read_text(&target.section)?;
        xml.insert_str(target.span.end, &new_para);
        doc.archive.write_text(target.section.clone(), xml);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn op_insert_nested_table(
    doc: &mut HwpxDocument,
    table_index: usize,
    row: usize,
    col: usize,
    rows: usize,
    cols: usize,
    data: Option<&[Vec<String>]>,
    header_row: bool,
    width_mode: NestedTableWidthMode,
) -> Result<(), HwpxError> {
    let loc = doc.get_table(table_index)?;
    if loc.depth >= 2 {
        return Err(HwpxError::NestedTableDepthExceeded { depth: loc.depth });
    }
    if !(1..=8).contains(&rows) || !(1..=8).contains(&cols) {
        return Err(HwpxError::InvalidArgument(format!(
            "insert_nested_table: rows ({rows}) and cols ({cols}) must be in 1..=8"
        )));
    }

    let target_width = match width_mode {
        NestedTableWidthMode::FitCell => cell_content_width(doc, table_index, row, col)
            .filter(|&w| w > 100),
    };
    let cells = data.unwrap_or(&[]);
    let built = build_table_xml(
        doc,
        rows,
        cols,
        cells,
        target_width,
        true,
        Some(header_row),
    )?;
    let nested_tbl_xml = built.tbl_xml;

    let sec_xml = doc.archive.read_text(&loc.section)?;
    let tbl_inner = loc.span.inner(&sec_xml);
    let tr_list = scan_top_level(tbl_inner, "hp:tr")?;
    let tr_span = tr_list.get(row).ok_or_else(|| {
        HwpxError::OutOfRange(format!(
            "insert_nested_table: row {row} out of range (table has {} rows)",
            tr_list.len()
        ))
    })?;
    let tr_inner = tr_span.inner(tbl_inner);
    let tc_list = scan_top_level(tr_inner, "hp:tc")?;
    let tc_idx = find_cell_index_in_row(tr_inner, &tc_list, col).ok_or_else(|| {
        HwpxError::OutOfRange(format!(
            "insert_nested_table: col {col} out of range (row has {} cells)",
            tc_list.len()
        ))
    })?;
    let tc_span = &tc_list[tc_idx];
    let tc_inner = tc_span.inner(tr_inner);
    let subs = scan_top_level(tc_inner, "hp:subList")?;
    let sub_span = subs.first().ok_or_else(|| {
        HwpxError::InvalidArgument("insert_nested_table: target cell has no <hp:subList>".to_string())
    })?;
    let sub_inner = sub_span.inner(tc_inner);
    let ps = scan_top_level(sub_inner, "hp:p")?;

    let new_sub_inner = if let Some(last_p) = ps.last() {
        let p_inner = last_p.inner(sub_inner);
        let has_text = !extract_t_text(p_inner)?.trim().is_empty();
        let has_other_ctrl = ["<hp:tbl", "<hp:pic", "<hp:drawText", "<hp:chart", "<hp:shape"]
            .iter()
            .any(|pat| p_inner.contains(pat));
        let char_pr_id = first_char_pr_id(p_inner);
        let p_attrs = last_p.attrs(sub_inner, "hp:p");
        let para_pr_id = get_attr(p_attrs, "paraPrIDRef").unwrap_or_else(|| "0".to_string());
        if !has_text && !has_other_ctrl {
            let new_run = format!(r#"<hp:run charPrIDRef="{char_pr_id}">{nested_tbl_xml}</hp:run>"#);
            let runs = scan_top_level(p_inner, "hp:run")?;
            let new_p_inner = if let Some(first_run) = runs.first() {
                splice_span(p_inner, first_run, &new_run)
            } else {
                new_run
            };
            let new_p = format!("<hp:p{p_attrs}>{}</hp:p>", drop_linesegs(&new_p_inner)?);
            splice_span(sub_inner, last_p, &new_p)
        } else {
            let new_p = format!(
                r#"<hp:p id="{}" paraPrIDRef="{para_pr_id}" styleIDRef="0" pageBreak="0" columnBreak="0" merged="0"><hp:run charPrIDRef="{char_pr_id}">{nested_tbl_xml}</hp:run></hp:p>"#,
                fresh_id()
            );
            format!("{sub_inner}{new_p}")
        }
    } else {
        format!(
            r#"<hp:p id="{}" paraPrIDRef="0" styleIDRef="0" pageBreak="0" columnBreak="0" merged="0"><hp:run charPrIDRef="0">{nested_tbl_xml}</hp:run></hp:p>"#,
            fresh_id()
        )
    };

    let new_sub_inner = drop_linesegs(&new_sub_inner)?;
    let new_sub = format!(
        "<hp:subList{}>{new_sub_inner}</hp:subList>",
        sub_span.attrs(tc_inner, "hp:subList")
    );
    let new_tc = format!(
        "<hp:tc{}>{}</hp:tc>",
        tc_span.attrs(tr_inner, "hp:tc"),
        splice_span(tc_inner, sub_span, &new_sub)
    );
    let new_tr = format!(
        "<hp:tr{}>{}</hp:tr>",
        tr_span.attrs(tbl_inner, "hp:tr"),
        splice_span(tr_inner, tc_span, &new_tc)
    );
    let new_tbl = format!(
        "<hp:tbl{}>{}</hp:tbl>",
        loc.span.attrs(&sec_xml, "hp:tbl"),
        splice_span(tbl_inner, tr_span, &new_tr)
    );
    doc.archive
        .write_text(loc.section, splice_span(&sec_xml, &loc.span, &new_tbl));
    Ok(())
}

fn normalize_row_addrs(tbl_inner: &str) -> Result<String, HwpxError> {
    let mut out = tbl_inner.to_string();
    let rows = scan_top_level(&out, "hp:tr")?;
    for (r, rw) in rows.into_iter().enumerate().rev() {
        let mut r_inner = rw.inner(&out).to_string();
        let tcs = scan_top_level(&r_inner, "hp:tc")?;
        for (c, tc) in tcs.into_iter().enumerate().rev() {
            let new_tc_inner = set_tc_child_tag_attr(tc.inner(&r_inner), "hp:cellAddr", "rowAddr", &r.to_string())?;
            let _ = c;
            let tc_attrs = tc.attrs(&r_inner, "hp:tc").to_string();
            r_inner = splice_span(&r_inner, &tc, &format!("<hp:tc{tc_attrs}>{new_tc_inner}</hp:tc>"));
        }
        let rw_attrs = rw.attrs(&out, "hp:tr").to_string();
        out = splice_span(&out, &rw, &format!("<hp:tr{rw_attrs}>{r_inner}</hp:tr>"));
    }
    Ok(out)
}

pub fn op_insert_table_row(
    doc: &mut HwpxDocument,
    table_index: usize,
    row: usize,
    position: RowInsertPosition,
    cells: Option<&[String]>,
) -> Result<(), HwpxError> {
    let loc = doc.get_table(table_index)?;
    let sec_xml = doc.archive.read_text(&loc.section)?;
    let tbl_inner = loc.span.inner(&sec_xml);
    let rows = scan_top_level(tbl_inner, "hp:tr")?;
    if rows.is_empty() {
        return Err(HwpxError::InvalidArgument(
            "insert_table_row: table has no rows".to_string(),
        ));
    }
    let ref_idx = row.min(rows.len() - 1);
    let ref_row = &rows[ref_idx];
    let ref_inner = ref_row.inner(tbl_inner);
    let tcs = scan_top_level(ref_inner, "hp:tc")?;
    let mut acc = String::new();
    for (ci, tc) in tcs.iter().enumerate() {
        let txt = cells.and_then(|c| c.get(ci)).map(String::as_str).unwrap_or("");
        let cell_inner = set_cell_inner_text(tc.inner(ref_inner), txt, false)?;
        let raw_tc = format!("<hp:tc{}>{cell_inner}</hp:tc>", tc.attrs(ref_inner, "hp:tc"));
        acc.push_str(&freshen_ids(&raw_tc)?);
    }
    let ref_attrs = ref_row.attrs(tbl_inner, "hp:tr");
    let new_row = format!("<hp:tr{ref_attrs}>{acc}</hp:tr>");
    let replacement = match position {
        RowInsertPosition::Below => format!("{}{new_row}", ref_row.full(tbl_inner)),
        RowInsertPosition::Above => format!("{new_row}{}", ref_row.full(tbl_inner)),
    };
    let spliced = splice_span(tbl_inner, ref_row, &replacement);
    let normalized = normalize_row_addrs(&spliced)?;
    let new_attrs = bump_tbl_count_attr(loc.span.attrs(&sec_xml, "hp:tbl"), "rowCnt", 1);
    let new_tbl = format!("<hp:tbl{new_attrs}>{normalized}</hp:tbl>");
    doc.archive
        .write_text(loc.section, splice_span(&sec_xml, &loc.span, &new_tbl));
    Ok(())
}

pub fn op_delete_table_row(
    doc: &mut HwpxDocument,
    table_index: usize,
    row: usize,
) -> Result<(), HwpxError> {
    let loc = doc.get_table(table_index)?;
    let sec_xml = doc.archive.read_text(&loc.section)?;
    let tbl_inner = loc.span.inner(&sec_xml);
    let rows = scan_top_level(tbl_inner, "hp:tr")?;
    let rw = rows.get(row).ok_or_else(|| {
        HwpxError::OutOfRange(format!(
            "delete_table_row: row {row} out of range (0..{})",
            rows.len().saturating_sub(1)
        ))
    })?;
    let spliced = splice_span(tbl_inner, rw, "");
    let normalized = normalize_row_addrs(&spliced)?;
    let new_attrs = bump_tbl_count_attr(loc.span.attrs(&sec_xml, "hp:tbl"), "rowCnt", -1);
    let new_tbl = format!("<hp:tbl{new_attrs}>{normalized}</hp:tbl>");
    doc.archive
        .write_text(loc.section, splice_span(&sec_xml, &loc.span, &new_tbl));
    Ok(())
}

pub fn op_insert_table_col(
    doc: &mut HwpxDocument,
    table_index: usize,
    col: usize,
    position: ColInsertPosition,
    cells: Option<&[String]>,
) -> Result<(), HwpxError> {
    let loc = doc.get_table(table_index)?;
    let sec_xml = doc.archive.read_text(&loc.section)?;
    let mut tbl_inner = loc.span.inner(&sec_xml).to_string();
    let rows = scan_top_level(&tbl_inner, "hp:tr")?;
    for (r, row_el) in rows.into_iter().enumerate().rev() {
        let row_inner_orig = row_el.inner(&tbl_inner).to_string();
        let tcs = scan_top_level(&row_inner_orig, "hp:tc")?;
        if tcs.is_empty() {
            continue;
        }
        let ref_idx = col.min(tcs.len() - 1);
        let ref_tc = &tcs[ref_idx];
        let txt = cells.and_then(|c| c.get(r)).map(String::as_str).unwrap_or("");
        let cell_inner = set_cell_inner_text(ref_tc.inner(&row_inner_orig), txt, false)?;
        let new_tc = freshen_ids(&format!(
            "<hp:tc{}>{cell_inner}</hp:tc>",
            ref_tc.attrs(&row_inner_orig, "hp:tc")
        ))?;
        let replacement = match position {
            ColInsertPosition::Right => format!("{}{new_tc}", ref_tc.full(&row_inner_orig)),
            ColInsertPosition::Left => format!("{new_tc}{}", ref_tc.full(&row_inner_orig)),
        };
        let mut row_inner = splice_span(&row_inner_orig, ref_tc, &replacement);
        let cells2 = scan_top_level(&row_inner, "hp:tc")?;
        for (c, tc2) in cells2.into_iter().enumerate().rev() {
            let fixed = set_tc_child_tag_attr(tc2.inner(&row_inner), "hp:cellAddr", "colAddr", &c.to_string())?;
            let tc2_attrs = tc2.attrs(&row_inner, "hp:tc").to_string();
            row_inner = splice_span(&row_inner, &tc2, &format!("<hp:tc{tc2_attrs}>{fixed}</hp:tc>"));
        }
        let row_attrs = row_el.attrs(&tbl_inner, "hp:tr").to_string();
        tbl_inner = splice_span(&tbl_inner, &row_el, &format!("<hp:tr{row_attrs}>{row_inner}</hp:tr>"));
    }
    let new_attrs = bump_tbl_count_attr(loc.span.attrs(&sec_xml, "hp:tbl"), "colCnt", 1);
    let new_tbl = format!("<hp:tbl{new_attrs}>{tbl_inner}</hp:tbl>");
    doc.archive
        .write_text(loc.section, splice_span(&sec_xml, &loc.span, &new_tbl));
    Ok(())
}

pub fn op_delete_table_col(
    doc: &mut HwpxDocument,
    table_index: usize,
    col: usize,
) -> Result<(), HwpxError> {
    let loc = doc.get_table(table_index)?;
    let sec_xml = doc.archive.read_text(&loc.section)?;
    let mut tbl_inner = loc.span.inner(&sec_xml).to_string();
    let rows = scan_top_level(&tbl_inner, "hp:tr")?;
    for row_el in rows.into_iter().rev() {
        let row_inner_orig = row_el.inner(&tbl_inner).to_string();
        let tcs = scan_top_level(&row_inner_orig, "hp:tc")?;
        if col >= tcs.len() {
            continue;
        }
        let mut row_inner = splice_span(&row_inner_orig, &tcs[col], "");
        let cells2 = scan_top_level(&row_inner, "hp:tc")?;
        for (c, tc2) in cells2.into_iter().enumerate().rev() {
            let fixed = set_tc_child_tag_attr(tc2.inner(&row_inner), "hp:cellAddr", "colAddr", &c.to_string())?;
            let tc2_attrs = tc2.attrs(&row_inner, "hp:tc").to_string();
            row_inner = splice_span(&row_inner, &tc2, &format!("<hp:tc{tc2_attrs}>{fixed}</hp:tc>"));
        }
        let row_attrs = row_el.attrs(&tbl_inner, "hp:tr").to_string();
        tbl_inner = splice_span(&tbl_inner, &row_el, &format!("<hp:tr{row_attrs}>{row_inner}</hp:tr>"));
    }
    let new_attrs = bump_tbl_count_attr(loc.span.attrs(&sec_xml, "hp:tbl"), "colCnt", -1);
    let new_tbl = format!("<hp:tbl{new_attrs}>{tbl_inner}</hp:tbl>");
    doc.archive
        .write_text(loc.section, splice_span(&sec_xml, &loc.span, &new_tbl));
    Ok(())
}

fn merge_horizontal(
    tbl_inner: &str,
    row: usize,
    start: usize,
    count: usize,
) -> Result<String, HwpxError> {
    if count < 2 {
        return Err(HwpxError::InvalidArgument(
            "merge_cells horizontal: count must be >= 2".to_string(),
        ));
    }
    let rows = scan_top_level(tbl_inner, "hp:tr")?;
    let row_el = rows.get(row).ok_or_else(|| {
        HwpxError::OutOfRange(format!("merge_cells: row {row} out of range"))
    })?;
    let mut inner = row_el.inner(tbl_inner).to_string();
    let tcs0 = scan_top_level(&inner, "hp:tc")?;
    if start + count > tcs0.len() {
        return Err(HwpxError::OutOfRange(
            "merge_cells horizontal: range out of bounds".to_string(),
        ));
    }
    for i in (1..count).rev() {
        let cur_tcs = scan_top_level(&inner, "hp:tc")?;
        inner = splice_span(&inner, &cur_tcs[start + i], "");
    }
    let first_tcs = scan_top_level(&inner, "hp:tc")?;
    let target = &first_tcs[start];
    let merged_inner = set_tc_child_tag_attr(
        target.inner(&inner),
        "hp:cellSpan",
        "colSpan",
        &count.to_string(),
    )?;
    let target_attrs = target.attrs(&inner, "hp:tc");
    inner = splice_span(&inner, target, &format!("<hp:tc{target_attrs}>{merged_inner}</hp:tc>"));
    let row_attrs = row_el.attrs(tbl_inner, "hp:tr");
    Ok(splice_span(tbl_inner, row_el, &format!("<hp:tr{row_attrs}>{inner}</hp:tr>")))
}

fn merge_vertical(
    tbl_inner: &str,
    col: usize,
    start: usize,
    count: usize,
) -> Result<String, HwpxError> {
    if count < 2 {
        return Err(HwpxError::InvalidArgument(
            "merge_cells vertical: count must be >= 2".to_string(),
        ));
    }
    let mut out = tbl_inner.to_string();
    let rows0 = scan_top_level(&out, "hp:tr")?;
    if start + count > rows0.len() {
        return Err(HwpxError::OutOfRange(
            "merge_cells vertical: range out of bounds".to_string(),
        ));
    }
    for i in (1..count).rev() {
        let rows = scan_top_level(&out, "hp:tr")?;
        let r_el = &rows[start + i];
        let r_inner = r_el.inner(&out);
        let tcs = scan_top_level(r_inner, "hp:tc")?;
        if col < tcs.len() {
            let new_r_inner = splice_span(r_inner, &tcs[col], "");
            let r_attrs = r_el.attrs(&out, "hp:tr").to_string();
            out = splice_span(&out, r_el, &format!("<hp:tr{r_attrs}>{new_r_inner}</hp:tr>"));
        }
    }
    let rows = scan_top_level(&out, "hp:tr")?;
    let first_r = &rows[start];
    let r_inner = first_r.inner(&out);
    let tcs = scan_top_level(r_inner, "hp:tc")?;
    let target = tcs.get(col).ok_or_else(|| {
        HwpxError::OutOfRange(format!("merge_cells vertical: col {col} out of range"))
    })?;
    let merged_inner = set_tc_child_tag_attr(
        target.inner(r_inner),
        "hp:cellSpan",
        "rowSpan",
        &count.to_string(),
    )?;
    let t_attrs = target.attrs(r_inner, "hp:tc");
    let new_r_inner = splice_span(r_inner, target, &format!("<hp:tc{t_attrs}>{merged_inner}</hp:tc>"));
    let r_attrs = first_r.attrs(&out, "hp:tr");
    Ok(splice_span(&out, first_r, &format!("<hp:tr{r_attrs}>{new_r_inner}</hp:tr>")))
}

pub fn op_merge_cells(
    doc: &mut HwpxDocument,
    table_index: usize,
    from_row: usize,
    from_col: usize,
    to_row: usize,
    to_col: usize,
) -> Result<(), HwpxError> {
    if to_row < from_row || to_col < from_col {
        return Err(HwpxError::InvalidArgument(
            "merge_cells: to_row/to_col must be >= from_row/from_col".to_string(),
        ));
    }
    let row_span = to_row - from_row + 1;
    let col_span = to_col - from_col + 1;
    if row_span == 1 && col_span == 1 {
        return Ok(());
    }
    let loc = doc.get_table(table_index)?;
    let sec_xml = doc.archive.read_text(&loc.section)?;
    let mut tbl_inner = loc.span.inner(&sec_xml).to_string();
    if col_span >= 2 {
        for r in from_row..=to_row {
            tbl_inner = merge_horizontal(&tbl_inner, r, from_col, col_span)?;
        }
    }
    if row_span >= 2 {
        tbl_inner = merge_vertical(&tbl_inner, from_col, from_row, row_span)?;
    }
    let new_tbl = format!(
        "<hp:tbl{}>{}</hp:tbl>",
        loc.span.attrs(&sec_xml, "hp:tbl"),
        drop_linesegs(&tbl_inner)?
    );
    doc.archive
        .write_text(loc.section, splice_span(&sec_xml, &loc.span, &new_tbl));
    Ok(())
}

pub fn op_unmerge_cells(
    doc: &mut HwpxDocument,
    table_index: usize,
    row: usize,
    col: usize,
) -> Result<(), HwpxError> {
    let loc = doc.get_table(table_index)?;
    let sec_xml = doc.archive.read_text(&loc.section)?;
    let mut tbl_inner = loc.span.inner(&sec_xml).to_string();
    let rows0 = scan_top_level(&tbl_inner, "hp:tr")?;
    let r_span = rows0.get(row).ok_or_else(|| {
        HwpxError::OutOfRange(format!("unmerge_cells: row {row} out of range"))
    })?;
    let r_inner0 = r_span.inner(&tbl_inner);
    let tcs0 = scan_top_level(r_inner0, "hp:tc")?;
    let tc_idx = find_cell_index_in_row(r_inner0, &tcs0, col).ok_or_else(|| {
        HwpxError::OutOfRange(format!("unmerge_cells: cell ({row}, {col}) not found"))
    })?;
    let target = &tcs0[tc_idx];
    let target_attrs = target.attrs(r_inner0, "hp:tc").to_string();
    let target_inner = target.inner(r_inner0).to_string();
    let tail = cell_tail(&target_inner);
    let (ca0, ra0) = read_cell_addr(tail, col, row);
    let (cs, rs) = read_cell_span(tail);
    if cs < 2 && rs < 2 {
        return Err(HwpxError::InvalidArgument(format!(
            "unmerge_cells: cell ({row}, {col}) is not merged"
        )));
    }
    let (w, h) = read_cell_size(tail).unwrap_or((0, 0));
    let w_each = if w > cs as i32 { w / cs as i32 } else { w };
    let h_each = if h > rs as i32 { h / rs as i32 } else { h };

    let sub_el = scan_top_level(&target_inner, "hp:subList")?.into_iter().next();
    let sub_attrs = sub_el
        .as_ref()
        .map(|s| s.attrs(&target_inner, "hp:subList").to_string())
        .unwrap_or_default();
    let p_el = sub_el
        .as_ref()
        .and_then(|s| scan_top_level(s.inner(&target_inner), "hp:p").ok())
        .and_then(|v| v.into_iter().next());
    let (p_attrs, cpr) = match (&sub_el, &p_el) {
        (Some(sub), Some(p)) => {
            let s_in = sub.inner(&target_inner);
            (
                p.attrs(s_in, "hp:p").to_string(),
                first_char_pr_id(p.inner(s_in)),
            )
        }
        _ => (
            r#" id="0" paraPrIDRef="0" styleIDRef="0" pageBreak="0" columnBreak="0" merged="0""#
                .to_string(),
            "0".to_string(),
        ),
    };
    let margin_el = scan_top_level(tail, "hp:cellMargin")?
        .into_iter()
        .next()
        .map(|m| m.full(tail).to_string())
        .unwrap_or_default();

    let mk_empty = |ra: usize, ca: usize| -> String {
        format!(
            r#"<hp:tc{target_attrs}><hp:subList{sub_attrs}><hp:p{p_attrs}><hp:run charPrIDRef="{cpr}"/></hp:p></hp:subList><hp:cellAddr colAddr="{ca}" rowAddr="{ra}"/><hp:cellSpan colSpan="1" rowSpan="1"/><hp:cellSz width="{w_each}" height="{h_each}"/>{margin_el}</hp:tc>"#
        )
    };

    let mut survivor = target_inner;
    survivor = set_tc_child_tag_attr(&survivor, "hp:cellSpan", "colSpan", "1")?;
    survivor = set_tc_child_tag_attr(&survivor, "hp:cellSpan", "rowSpan", "1")?;
    survivor = set_tc_child_tag_attr(&survivor, "hp:cellSz", "width", &w_each.to_string())?;
    survivor = set_tc_child_tag_attr(&survivor, "hp:cellSz", "height", &h_each.to_string())?;

    {
        let rows = scan_top_level(&tbl_inner, "hp:tr")?;
        let r_el = &rows[row];
        let r_in = r_el.inner(&tbl_inner);
        let tcs = scan_top_level(r_in, "hp:tc")?;
        let tgt_idx = find_cell_index_in_row(r_in, &tcs, col).expect("target");
        let mut fill = format!("<hp:tc{target_attrs}>{survivor}</hp:tc>");
        for k in 1..cs {
            fill.push_str(&mk_empty(ra0, ca0 + k));
        }
        let new_r_in = splice_span(r_in, &tcs[tgt_idx], &fill);
        let r_attrs = r_el.attrs(&tbl_inner, "hp:tr");
        tbl_inner = splice_span(&tbl_inner, r_el, &format!("<hp:tr{r_attrs}>{new_r_in}</hp:tr>"));
    }

    for dr in 1..rs {
        let rows = scan_top_level(&tbl_inner, "hp:tr")?;
        let Some(r_el) = rows.get(row + dr) else {
            break;
        };
        let r_in = r_el.inner(&tbl_inner);
        let tcs = scan_top_level(r_in, "hp:tc")?;
        let mut insert_after: Option<&XmlElementSpan> = None;
        for tc in &tcs {
            let tc_in = tc.inner(r_in);
            let (a, _) = read_cell_addr(cell_tail(tc_in), usize::MAX, 0);
            if a < ca0 {
                insert_after = Some(tc);
            }
        }
        let mut fills = String::new();
        for k in 0..cs {
            fills.push_str(&mk_empty(ra0 + dr, ca0 + k));
        }
        let new_r_in = if let Some(prev) = insert_after {
            splice_span(r_in, prev, &format!("{}{fills}", prev.full(r_in)))
        } else {
            format!("{fills}{r_in}")
        };
        let r_attrs = r_el.attrs(&tbl_inner, "hp:tr");
        tbl_inner = splice_span(&tbl_inner, r_el, &format!("<hp:tr{r_attrs}>{new_r_in}</hp:tr>"));
    }

    let new_tbl = format!(
        "<hp:tbl{}>{}</hp:tbl>",
        loc.span.attrs(&sec_xml, "hp:tbl"),
        drop_linesegs(&tbl_inner)?
    );
    doc.archive
        .write_text(loc.section, splice_span(&sec_xml, &loc.span, &new_tbl));
    Ok(())
}

pub fn op_split_cell(
    doc: &mut HwpxDocument,
    table_index: usize,
    row: usize,
    col: usize,
    into_rows: usize,
    into_cols: usize,
) -> Result<(), HwpxError> {
    let n_rows = into_rows.max(1);
    let n_cols = into_cols.max(1);
    if n_rows < 2 && n_cols < 2 {
        return Err(HwpxError::InvalidArgument(
            "split_cell: need into_rows >= 2 or into_cols >= 2".to_string(),
        ));
    }
    let loc = doc.get_table(table_index)?;
    let sec_xml = doc.archive.read_text(&loc.section)?;
    let tbl_inner = loc.span.inner(&sec_xml);
    let rows_els = scan_top_level(tbl_inner, "hp:tr")?;

    let mut found: Option<(usize, XmlElementSpan, String, String)> = None;
    for (r, r_el) in rows_els.iter().enumerate() {
        let r_in = r_el.inner(tbl_inner);
        for tc in scan_top_level(r_in, "hp:tc")? {
            let tc_in = tc.inner(r_in);
            let tail = cell_tail(tc_in);
            let (ca, ra) = read_cell_addr(tail, usize::MAX, usize::MAX);
            if ca == col && ra == row {
                found = Some((
                    r,
                    tc.clone(),
                    tc.attrs(r_in, "hp:tc").to_string(),
                    tc_in.to_string(),
                ));
                break;
            }
        }
        if found.is_some() {
            break;
        }
    }

    let (target_tr_idx, _, target_attrs, target_inner) = found.ok_or_else(|| {
        HwpxError::OutOfRange(format!("split_cell: cell ({row}, {col}) not found"))
    })?;
    let target_tail = cell_tail(&target_inner);
    let (cs0, rs0) = read_cell_span(target_tail);
    if cs0 != 1 || rs0 != 1 {
        return Err(HwpxError::InvalidArgument(
            "split_cell: target is a merged cell; unmerge before splitting".to_string(),
        ));
    }
    let (t_w, t_h) = read_cell_size(target_tail).unwrap_or((1, 1));
    let sub_w = ((t_w as f64 / n_cols as f64).round() as i32).max(1);
    let sub_h = ((t_h as f64 / n_rows as f64).round() as i32).max(1);
    let dm = n_cols - 1;
    let dn = n_rows - 1;

    let sub_cell = |c: usize, r: usize, empty: bool| -> Result<String, HwpxError> {
        let mut inner = target_inner.clone();
        inner = set_tc_child_tag_attr(&inner, "hp:cellAddr", "colAddr", &c.to_string())?;
        inner = set_tc_child_tag_attr(&inner, "hp:cellAddr", "rowAddr", &r.to_string())?;
        inner = set_tc_child_tag_attr(&inner, "hp:cellSpan", "colSpan", "1")?;
        inner = set_tc_child_tag_attr(&inner, "hp:cellSpan", "rowSpan", "1")?;
        inner = set_tc_child_tag_attr(&inner, "hp:cellSz", "width", &sub_w.to_string())?;
        inner = set_tc_child_tag_attr(&inner, "hp:cellSz", "height", &sub_h.to_string())?;
        if empty {
            inner = set_cell_inner_text(&inner, "", false)?;
        }
        freshen_ids(&format!("<hp:tc{target_attrs}>{inner}</hp:tc>"))
    };

    let mut new_rows = String::new();
    for (r, row_el) in rows_els.iter().enumerate() {
        let r_in = row_el.inner(tbl_inner);
        let r_attrs = row_el.attrs(tbl_inner, "hp:tr");
        let mut pieces = String::new();
        for tc in scan_top_level(r_in, "hp:tc")? {
            let tc_in = tc.inner(r_in);
            let tail = cell_tail(tc_in);
            let (ca, ra) = read_cell_addr(tail, 0, r);
            let (cs, rs) = read_cell_span(tail);
            if r == target_tr_idx && ca == col && ra == row {
                for dc in 0..n_cols {
                    pieces.push_str(&sub_cell(col + dc, row, dc != 0)?);
                }
                continue;
            }
            let (mut nca, mut nra, mut ncs, mut nrs) = (ca, ra, cs, rs);
            if ca > col {
                nca += dm;
            } else if ca <= col && ca + cs > col {
                ncs += dm;
            }
            if ra > row {
                nra += dn;
            } else if ra <= row && ra + rs > row {
                nrs += dn;
            }
            let mut inner = tc_in.to_string();
            inner = set_tc_child_tag_attr(&inner, "hp:cellAddr", "colAddr", &nca.to_string())?;
            inner = set_tc_child_tag_attr(&inner, "hp:cellAddr", "rowAddr", &nra.to_string())?;
            inner = set_tc_child_tag_attr(&inner, "hp:cellSpan", "colSpan", &ncs.to_string())?;
            inner = set_tc_child_tag_attr(&inner, "hp:cellSpan", "rowSpan", &nrs.to_string())?;
            pieces.push_str(&format!("<hp:tc{}>{inner}</hp:tc>", tc.attrs(r_in, "hp:tc")));
        }
        new_rows.push_str(&format!("<hp:tr{r_attrs}>{pieces}</hp:tr>"));
        if r == target_tr_idx && dn > 0 {
            for dr in 1..=dn {
                let mut cells_str = String::new();
                for dc in 0..n_cols {
                    cells_str.push_str(&sub_cell(col + dc, row + dr, true)?);
                }
                new_rows.push_str(&format!("<hp:tr{r_attrs}>{cells_str}</hp:tr>"));
            }
        }
    }

    let mut new_attrs = loc.span.attrs(&sec_xml, "hp:tbl").to_string();
    if dm > 0 {
        new_attrs = bump_tbl_count_attr(&new_attrs, "colCnt", dm as isize);
    }
    if dn > 0 {
        new_attrs = bump_tbl_count_attr(&new_attrs, "rowCnt", dn as isize);
    }
    let first_tr_idx = tbl_inner.find("<hp:tr").unwrap_or(tbl_inner.len());
    let head = &tbl_inner[..first_tr_idx];
    let new_tbl = format!("<hp:tbl{new_attrs}>{head}{}</hp:tbl>", drop_linesegs(&new_rows)?);
    doc.archive
        .write_text(loc.section, splice_span(&sec_xml, &loc.span, &new_tbl));
    Ok(())
}

pub fn op_equalize_table(
    doc: &mut HwpxDocument,
    table_index: usize,
    mode: EqualizeMode,
) -> Result<(), HwpxError> {
    let loc = doc.get_table(table_index)?;
    let sec_xml = doc.archive.read_text(&loc.section)?;
    let tbl_inner = loc.span.inner(&sec_xml);
    let rows = scan_top_level(tbl_inner, "hp:tr")?;
    if rows.is_empty() {
        return Err(HwpxError::InvalidArgument(
            "equalize_table: table has no rows".to_string(),
        ));
    }

    let mut row_heights = Vec::with_capacity(rows.len());
    for r in &rows {
        let r_in = r.inner(tbl_inner);
        let h = scan_top_level(r_in, "hp:tc")?
            .first()
            .and_then(|c| read_cell_size(c.inner(r_in)))
            .map(|(_, h)| h)
            .unwrap_or(0);
        row_heights.push(h);
    }
    let cols0 = scan_top_level(rows[0].inner(tbl_inner), "hp:tc")?;
    let col_widths: Vec<i32> = cols0
        .iter()
        .map(|c| {
            read_cell_size(c.inner(rows[0].inner(tbl_inner)))
                .map(|(w, _)| w)
                .unwrap_or(0)
        })
        .collect();
    let eq_h = (row_heights.iter().sum::<i32>() as f64 / rows.len() as f64).round() as i32;
    let eq_w =
        (col_widths.iter().sum::<i32>() as f64 / col_widths.len().max(1) as f64).round() as i32;

    let mut out = tbl_inner.to_string();
    for rw in rows.into_iter().rev() {
        let mut r_in = rw.inner(&out).to_string();
        let tcs = scan_top_level(&r_in, "hp:tc")?;
        for tc in tcs.into_iter().rev() {
            let mut tc_in = tc.inner(&r_in).to_string();
            if matches!(mode, EqualizeMode::Width | EqualizeMode::Both) {
                tc_in = set_tc_child_tag_attr(&tc_in, "hp:cellSz", "width", &eq_w.to_string())?;
            }
            if matches!(mode, EqualizeMode::Height | EqualizeMode::Both) {
                tc_in = set_tc_child_tag_attr(&tc_in, "hp:cellSz", "height", &eq_h.to_string())?;
            }
            let t_attrs = tc.attrs(&r_in, "hp:tc").to_string();
            r_in = splice_span(&r_in, &tc, &format!("<hp:tc{t_attrs}>{tc_in}</hp:tc>"));
        }
        let r_attrs = rw.attrs(&out, "hp:tr").to_string();
        out = splice_span(&out, &rw, &format!("<hp:tr{r_attrs}>{r_in}</hp:tr>"));
    }
    let new_tbl = format!("<hp:tbl{}>{out}</hp:tbl>", loc.span.attrs(&sec_xml, "hp:tbl"));
    doc.archive
        .write_text(loc.section, splice_span(&sec_xml, &loc.span, &new_tbl));
    Ok(())
}

#[derive(Clone)]
struct CellZone {
    start_row: usize,
    start_col: usize,
    end_row: usize,
    end_col: usize,
    bf: String,
}

fn add_cellzone(
    doc: &mut HwpxDocument,
    table_index: usize,
    row: usize,
    col: usize,
    border_fill_id: &str,
) -> Result<(), HwpxError> {
    let loc = doc.get_table(table_index)?;
    let sec_xml = doc.archive.read_text(&loc.section)?;
    let tbl_inner = loc.span.inner(&sec_xml);
    let lists = scan_top_level(tbl_inner, "hp:cellzoneList")?;

    let mut zones: Vec<CellZone> = Vec::new();
    if let Some(list) = lists.first() {
        let l_in = list.inner(tbl_inner);
        for z in scan_top_level(l_in, "hp:cellzone")? {
            let za = z.attrs(l_in, "hp:cellzone");
            zones.push(CellZone {
                start_row: get_attr(za, "startRowAddr").and_then(|s| s.parse().ok()).unwrap_or(0),
                start_col: get_attr(za, "startColAddr").and_then(|s| s.parse().ok()).unwrap_or(0),
                end_row: get_attr(za, "endRowAddr").and_then(|s| s.parse().ok()).unwrap_or(0),
                end_col: get_attr(za, "endColAddr").and_then(|s| s.parse().ok()).unwrap_or(0),
                bf: get_attr(za, "borderFillIDRef").unwrap_or_default(),
            });
        }
    }

    let mut merged = false;
    for z in &mut zones {
        if z.bf != border_fill_id {
            continue;
        }
        let contained = z.start_row <= row && row <= z.end_row && z.start_col <= col && col <= z.end_col;
        if contained {
            merged = true;
            break;
        }
        let horiz_adj = z.start_row == row && z.end_row == row && (z.end_col + 1 == col || z.start_col == col + 1);
        let vert_adj = z.start_col == col && z.end_col == col && (z.end_row + 1 == row || z.start_row == row + 1);
        if horiz_adj {
            z.start_col = z.start_col.min(col);
            z.end_col = z.end_col.max(col);
            merged = true;
            break;
        }
        if vert_adj {
            z.start_row = z.start_row.min(row);
            z.end_row = z.end_row.max(row);
            merged = true;
            break;
        }
    }
    if !merged {
        zones.push(CellZone {
            start_row: row,
            start_col: col,
            end_row: row,
            end_col: col,
            bf: border_fill_id.to_string(),
        });
    }

    let serialized: String = zones
        .iter()
        .map(|z| {
            format!(
                r#"<hp:cellzone startRowAddr="{}" startColAddr="{}" endRowAddr="{}" endColAddr="{}" borderFillIDRef="{}"/>"#,
                z.start_row, z.start_col, z.end_row, z.end_col, z.bf
            )
        })
        .collect();
    let list_xml = format!("<hp:cellzoneList>{serialized}</hp:cellzoneList>");
    let new_tbl_inner = if let Some(list) = lists.first() {
        splice_span(tbl_inner, list, &list_xml)
    } else {
        let first_tr = tbl_inner.find("<hp:tr").ok_or_else(|| {
            HwpxError::InvalidArgument("cell style: table has no rows".to_string())
        })?;
        format!("{}{list_xml}{}", &tbl_inner[..first_tr], &tbl_inner[first_tr..])
    };
    let new_tbl = format!("<hp:tbl{}>{new_tbl_inner}</hp:tbl>", loc.span.attrs(&sec_xml, "hp:tbl"));
    doc.archive
        .write_text(loc.section, splice_span(&sec_xml, &loc.span, &new_tbl));
    Ok(())
}

fn apply_cell_shade_color(
    doc: &mut HwpxDocument,
    table_index: usize,
    row: usize,
    col: usize,
    color: &str,
) -> Result<(), HwpxError> {
    let loc = doc.get_table(table_index)?;
    let sec_xml = doc.archive.read_text_ro(&loc.section)?;
    let tbl_inner = loc.span.inner(&sec_xml);
    let rows = scan_top_level(tbl_inner, "hp:tr")?;
    let Some(rw) = rows.get(row) else {
        return Ok(());
    };
    let r_in = rw.inner(tbl_inner);
    let tcs = scan_top_level(r_in, "hp:tc")?;
    let Some(tc_idx) = find_cell_index_in_row(r_in, &tcs, col) else {
        return Ok(());
    };
    let tc_in = tcs[tc_idx].inner(r_in);
    let source_ref = first_char_pr_id(tc_in);
    let Some(header_name) = doc.archive.header_name() else {
        return Ok(());
    };
    let header = doc.archive.read_text(&header_name)?;
    let char_prs = scan_top_level(&header, "hh:charPr")?;
    if char_prs.is_empty() {
        return Ok(());
    }
    let base = char_prs
        .iter()
        .find(|c| get_attr(c.attrs(&header, "hh:charPr"), "id").as_deref() == Some(source_ref.as_str()))
        .unwrap_or(&char_prs[0]);
    let base_inner = base.inner(&header);

    for cp in &char_prs {
        if cp.inner(&header) == base_inner
            && get_attr(cp.attrs(&header, "hh:charPr"), "shadeColor").as_deref() == Some(color)
        {
            if let Some(id) = get_attr(cp.attrs(&header, "hh:charPr"), "id") {
                return retarget_cell_runs(doc, table_index, row, col, &id);
            }
        }
    }

    let ref_counts = doc.build_attr_ref_counts("charPrIDRef")?;
    let placeholder = char_prs.iter().find(|c| {
        let id = get_attr(c.attrs(&header, "hh:charPr"), "id").unwrap_or_default();
        ref_counts.get(&id).copied().unwrap_or(0) == 0 && id != source_ref
    });
    let max_id = char_prs
        .iter()
        .filter_map(|c| {
            get_attr(c.attrs(&header, "hh:charPr"), "id").and_then(|s| s.parse::<usize>().ok())
        })
        .max()
        .unwrap_or(0);
    let use_id = placeholder
        .and_then(|c| get_attr(c.attrs(&header, "hh:charPr"), "id"))
        .unwrap_or_else(|| (max_id + 1).to_string());
    let mut new_attrs = set_or_add_attr(base.attrs(&header, "hh:charPr"), "shadeColor", color);
    new_attrs = set_or_add_attr(&new_attrs, "id", &use_id);
    let updated = format!("<hh:charPr{new_attrs}>{base_inner}</hh:charPr>");
    let new_header = if let Some(ph) = placeholder {
        splice_span(&header, ph, &updated)
    } else {
        let h2 = splice_span(&header, base, &format!("{}{updated}", base.full(&header)));
        sync_item_count(&h2, "hh:charProperties", "hh:charPr")?
    };
    doc.archive.write_text(header_name, new_header);
    retarget_cell_runs(doc, table_index, row, col, &use_id)
}

fn retarget_cell_runs(
    doc: &mut HwpxDocument,
    table_index: usize,
    row: usize,
    col: usize,
    new_char_pr_id: &str,
) -> Result<(), HwpxError> {
    let loc = doc.get_table(table_index)?;
    let sec_xml = doc.archive.read_text(&loc.section)?;
    let tbl_inner = loc.span.inner(&sec_xml);
    let rows = scan_top_level(tbl_inner, "hp:tr")?;
    let rw = &rows[row];
    let r_in = rw.inner(tbl_inner);
    let tcs = scan_top_level(r_in, "hp:tc")?;
    let tc_idx = find_cell_index_in_row(r_in, &tcs, col).expect("cell exists");
    let tc = &tcs[tc_idx];
    let mut new_tc_inner = tc.inner(r_in).to_string();
    // Retarget all <hp:run> inside the cell
    let mut out = String::with_capacity(new_tc_inner.len());
    let mut cursor = 0usize;
    let needle = r#"charPrIDRef=""#;
    while let Some(pos) = new_tc_inner[cursor..].find(needle) {
        let abs = cursor + pos + needle.len();
        out.push_str(&new_tc_inner[cursor..abs]);
        if let Some(end_q) = new_tc_inner[abs..].find('"') {
            out.push_str(new_char_pr_id);
            cursor = abs + end_q;
        } else {
            cursor = abs;
            break;
        }
    }
    out.push_str(&new_tc_inner[cursor..]);
    new_tc_inner = out;

    let new_tc = format!("<hp:tc{}>{new_tc_inner}</hp:tc>", tc.attrs(r_in, "hp:tc"));
    let new_r_in = splice_span(r_in, tc, &new_tc);
    let new_r = format!("<hp:tr{}>{new_r_in}</hp:tr>", rw.attrs(tbl_inner, "hp:tr"));
    let new_tbl = format!(
        "<hp:tbl{}>{}</hp:tbl>",
        loc.span.attrs(&sec_xml, "hp:tbl"),
        splice_span(tbl_inner, rw, &new_r)
    );
    doc.archive
        .write_text(loc.section, splice_span(&sec_xml, &loc.span, &new_tbl));
    Ok(())
}

pub fn op_set_cell_background(
    doc: &mut HwpxDocument,
    table_index: usize,
    row: usize,
    col: usize,
    color: &str,
) -> Result<(), HwpxError> {
    let c = norm_hex(color);
    let brush = format!(
        r##"<hc:fillBrush><hc:winBrush faceColor="{c}" hatchColor="#999999" alpha="0"/></hc:fillBrush>"##
    );
    let bf_id = doc.ensure_border_fill(&format!(
        "{STD_SLASH_NONE}{}{DEFAULT_DIAG}{brush}",
        crate::model::NONE_SIDES
    ))?;
    add_cellzone(doc, table_index, row, col, &bf_id)?;
    apply_cell_shade_color(doc, table_index, row, col, &c)
}

#[allow(clippy::too_many_arguments)]
pub fn op_set_cell_border(
    doc: &mut HwpxDocument,
    table_index: usize,
    row: usize,
    col: usize,
    sides: &[BorderSide],
    color: &str,
    width_hu: HwpUnit,
    line_type: BorderLineType,
) -> Result<(), HwpxError> {
    let c = norm_hex(color);
    let w = format_hu_as_mm(width_hu);
    let lt = match line_type {
        BorderLineType::Solid => "SOLID",
        BorderLineType::Dashed => "DASH",
        BorderLineType::Dotted => "DOT",
        BorderLineType::Double => "DOUBLE_SLIM",
    };
    let has = |s: BorderSide| sides.is_empty() || sides.contains(&s);
    let side_xml = |tag: &str, selected: bool| {
        if selected {
            format!(r#"<hh:{tag} type="{lt}" width="{w}" color="{c}"/>"#)
        } else {
            format!(r##"<hh:{tag} type="SOLID" width="0.12 mm" color="#000000"/>"##)
        }
    };
    let sides_xml = format!(
        "{}{}{}{}",
        side_xml("leftBorder", has(BorderSide::Left)),
        side_xml("rightBorder", has(BorderSide::Right)),
        side_xml("topBorder", has(BorderSide::Top)),
        side_xml("bottomBorder", has(BorderSide::Bottom)),
    );
    let bf_id = doc.ensure_border_fill(&format!("{STD_SLASH_NONE}{sides_xml}{DEFAULT_DIAG}"))?;
    add_cellzone(doc, table_index, row, col, &bf_id)
}

#[allow(clippy::too_many_arguments)]
pub fn op_set_cell_align(
    doc: &mut HwpxDocument,
    table_index: usize,
    row: usize,
    col: usize,
    end_row: Option<usize>,
    end_col: Option<usize>,
    horizontal: Option<CellHAlign>,
    vertical: Option<CellVAlign>,
) -> Result<(), HwpxError> {
    let r_end = end_row.unwrap_or(row).max(row);
    let c_end = end_col.unwrap_or(col).max(col);
    for r in row..=r_end {
        for c in col..=c_end {
            set_single_cell_align(doc, table_index, r, c, horizontal, vertical)?;
        }
    }
    Ok(())
}

fn set_single_cell_align(
    doc: &mut HwpxDocument,
    table_index: usize,
    row: usize,
    col: usize,
    horizontal: Option<CellHAlign>,
    vertical: Option<CellVAlign>,
) -> Result<(), HwpxError> {
    let h_str = horizontal.map(|h| match h {
        CellHAlign::Left => "LEFT",
        CellHAlign::Center => "CENTER",
        CellHAlign::Right => "RIGHT",
        CellHAlign::Justify => "JUSTIFY",
    });
    let v_str = vertical.map(|v| match v {
        CellVAlign::Top => "TOP",
        CellVAlign::Center | CellVAlign::Middle => "CENTER",
        CellVAlign::Bottom => "BOTTOM",
    });

    let ppr_id = if let Some(align) = h_str {
        let header_name = doc.require_header_name()?;
        let header = doc.archive.read_text(&header_name)?;
        let para_prs = scan_top_level(&header, "hh:paraPr")?;
        if para_prs.is_empty() {
            return Err(HwpxError::MissingPart("no <hh:paraPr> in header.xml".to_string()));
        }
        let loc = doc.get_table(table_index)?;
        let sec_xml = doc.archive.read_text_ro(&loc.section)?;
        let tbl_inner = loc.span.inner(&sec_xml);
        let rows = scan_top_level(tbl_inner, "hp:tr")?;
        let rw = rows.get(row).ok_or_else(|| {
            HwpxError::OutOfRange(format!("set_cell_align: row {row} out of range"))
        })?;
        let r_in = rw.inner(tbl_inner);
        let tcs = scan_top_level(r_in, "hp:tc")?;
        let tc_idx = find_cell_index_in_row(r_in, &tcs, col).ok_or_else(|| {
            HwpxError::OutOfRange(format!("set_cell_align: col {col} out of range"))
        })?;
        let tc_in = tcs[tc_idx].inner(r_in);
        let sub = scan_top_level(tc_in, "hp:subList")?.into_iter().next().ok_or_else(|| {
            HwpxError::InvalidArgument("set_cell_align: cell has no <hp:subList>".to_string())
        })?;
        let p = scan_top_level(sub.inner(tc_in), "hp:p")?
            .into_iter()
            .next()
            .ok_or_else(|| {
                HwpxError::InvalidArgument("set_cell_align: cell has no <hp:p>".to_string())
            })?;
        let src_ref = get_attr(p.attrs(sub.inner(tc_in), "hp:p"), "paraPrIDRef")
            .unwrap_or_else(|| "0".to_string());
        let base = para_prs
            .iter()
            .find(|pp| get_attr(pp.attrs(&header, "hh:paraPr"), "id").as_deref() == Some(src_ref.as_str()))
            .unwrap_or(&para_prs[0]);
        let ref_counts = doc.build_attr_ref_counts("paraPrIDRef")?;
        let placeholder = para_prs.iter().find(|pp| {
            let id = get_attr(pp.attrs(&header, "hh:paraPr"), "id").unwrap_or_default();
            ref_counts.get(&id).copied().unwrap_or(0) == 0 && id != src_ref
        });
        let max_id = para_prs
            .iter()
            .filter_map(|pp| {
                get_attr(pp.attrs(&header, "hh:paraPr"), "id").and_then(|s| s.parse::<usize>().ok())
            })
            .max()
            .unwrap_or(0);
        let use_id = placeholder
            .and_then(|pp| get_attr(pp.attrs(&header, "hh:paraPr"), "id"))
            .unwrap_or_else(|| (max_id + 1).to_string());
        let mut mut_inner = base.inner(&header).to_string();
        let align_xml = format!(r#"<hh:align horizontal="{align}" vertical="BASELINE"/>"#);
        if let Some(a_el) = scan_top_level(&mut_inner, "hh:align")?.first().cloned() {
            mut_inner = splice_span(&mut_inner, &a_el, &align_xml);
        } else {
            mut_inner = format!("{align_xml}{mut_inner}");
        }
        let mut_attrs = set_or_add_attr(base.attrs(&header, "hh:paraPr"), "id", &use_id);
        let updated = format!("<hh:paraPr{mut_attrs}>{mut_inner}</hh:paraPr>");
        let new_header = if let Some(ph) = placeholder {
            splice_span(&header, ph, &updated)
        } else {
            let h2 = splice_span(&header, base, &format!("{}{updated}", base.full(&header)));
            sync_item_count(&h2, "hh:paraProperties", "hh:paraPr")?
        };
        doc.archive.write_text(header_name, new_header);
        Some(use_id)
    } else {
        None
    };

    let loc = doc.get_table(table_index)?;
    let sec_xml = doc.archive.read_text(&loc.section)?;
    let tbl_inner = loc.span.inner(&sec_xml);
    let rows = scan_top_level(tbl_inner, "hp:tr")?;
    let rw = rows.get(row).ok_or_else(|| {
        HwpxError::OutOfRange(format!("set_cell_align: row {row} out of range"))
    })?;
    let r_in = rw.inner(tbl_inner);
    let tcs = scan_top_level(r_in, "hp:tc")?;
    let tc_idx = find_cell_index_in_row(r_in, &tcs, col).ok_or_else(|| {
        HwpxError::OutOfRange(format!("set_cell_align: col {col} out of range"))
    })?;
    let tc = &tcs[tc_idx];
    let mut tc_inner = tc.inner(r_in).to_string();

    if let Some(sub) = scan_top_level(&tc_inner, "hp:subList")?.first().cloned() {
        let mut sub_attrs = sub.attrs(&tc_inner, "hp:subList").to_string();
        let mut sub_inner = sub.inner(&tc_inner).to_string();
        if let Some(v) = v_str {
            sub_attrs = set_or_add_attr(&sub_attrs, "vertAlign", v);
        }
        if let Some(id) = &ppr_id {
            if let Some(p) = scan_top_level(&sub_inner, "hp:p")?.first().cloned() {
                let p_attrs = set_or_add_attr(p.attrs(&sub_inner, "hp:p"), "paraPrIDRef", id);
                let new_p = format!("<hp:p{p_attrs}>{}</hp:p>", p.inner(&sub_inner));
                sub_inner = splice_span(&sub_inner, &p, &new_p);
            }
        }
        tc_inner = splice_span(&tc_inner, &sub, &format!("<hp:subList{sub_attrs}>{sub_inner}</hp:subList>"));
    }

    let new_tc = format!("<hp:tc{}>{tc_inner}</hp:tc>", tc.attrs(r_in, "hp:tc"));
    let new_r_in = splice_span(r_in, tc, &new_tc);
    let new_r = format!("<hp:tr{}>{new_r_in}</hp:tr>", rw.attrs(tbl_inner, "hp:tr"));
    let new_tbl = format!(
        "<hp:tbl{}>{}</hp:tbl>",
        loc.span.attrs(&sec_xml, "hp:tbl"),
        splice_span(tbl_inner, rw, &new_r)
    );
    doc.archive
        .write_text(loc.section, splice_span(&sec_xml, &loc.span, &new_tbl));
    Ok(())
}

pub fn op_set_col_width(
    doc: &mut HwpxDocument,
    table_index: usize,
    col: usize,
    width_hu: HwpUnit,
    keep_table_width: bool,
) -> Result<(), HwpxError> {
    let loc = doc.get_table(table_index)?;
    let sec_xml = doc.archive.read_text(&loc.section)?;
    let tbl_inner = loc.span.inner(&sec_xml);
    let rows = scan_top_level(tbl_inner, "hp:tr")?;
    if rows.is_empty() {
        return Err(HwpxError::InvalidArgument(
            "set_col_width: table has no rows".to_string(),
        ));
    }
    let cols0 = scan_top_level(rows[0].inner(tbl_inner), "hp:tc")?;
    if col >= cols0.len() {
        return Err(HwpxError::OutOfRange(format!(
            "set_col_width: col {col} out of range ({} columns)",
            cols0.len()
        )));
    }
    let mut col_widths: Vec<i32> = cols0
        .iter()
        .map(|c| {
            read_cell_size(c.inner(rows[0].inner(tbl_inner)))
                .map(|(w, _)| w)
                .unwrap_or(0)
        })
        .collect();
    let old_w = col_widths[col];
    let delta_w = width_hu - old_w;
    col_widths[col] = width_hu;

    let adj = if col + 1 < col_widths.len() {
        col + 1
    } else {
        col.saturating_sub(1)
    };
    if keep_table_width && col_widths.len() > 1 {
        let min_w = (5.0 * HWPUNIT_PER_MM).round() as i32;
        col_widths[adj] = (col_widths[adj] - delta_w).max(min_w);
    }

    let mut out = tbl_inner.to_string();
    for rw in rows.into_iter().rev() {
        let mut r_in = rw.inner(&out).to_string();
        let tcs = scan_top_level(&r_in, "hp:tc")?;
        for (ci, tc) in tcs.into_iter().enumerate().rev() {
            if ci == col || (keep_table_width && col_widths.len() > 1 && ci == adj) {
                let target_w = col_widths[ci];
                let new_tc_in =
                    set_tc_child_tag_attr(tc.inner(&r_in), "hp:cellSz", "width", &target_w.to_string())?;
                let t_attrs = tc.attrs(&r_in, "hp:tc").to_string();
                r_in = splice_span(&r_in, &tc, &format!("<hp:tc{t_attrs}>{new_tc_in}</hp:tc>"));
            }
        }
        let r_attrs = rw.attrs(&out, "hp:tr").to_string();
        out = splice_span(&out, &rw, &format!("<hp:tr{r_attrs}>{r_in}</hp:tr>"));
    }

    if !keep_table_width {
        let total_w: i32 = col_widths.iter().sum();
        if let Some(sz) = scan_top_level(&out, "hp:sz")?.first().cloned() {
            let updated = set_tag_attr(sz.full(&out), "width", &total_w.to_string());
            out = splice_span(&out, &sz, &updated);
        }
    }

    let new_tbl = format!("<hp:tbl{}>{out}</hp:tbl>", loc.span.attrs(&sec_xml, "hp:tbl"));
    doc.archive
        .write_text(loc.section, splice_span(&sec_xml, &loc.span, &new_tbl));
    Ok(())
}

pub fn op_set_row_height(
    doc: &mut HwpxDocument,
    table_index: usize,
    row: usize,
    height_hu: HwpUnit,
) -> Result<(), HwpxError> {
    let loc = doc.get_table(table_index)?;
    let sec_xml = doc.archive.read_text(&loc.section)?;
    let tbl_inner = loc.span.inner(&sec_xml);
    let rows = scan_top_level(tbl_inner, "hp:tr")?;
    let rw = rows.get(row).ok_or_else(|| {
        HwpxError::OutOfRange(format!("set_row_height: row {row} out of range"))
    })?;
    let mut r_in = rw.inner(tbl_inner).to_string();
    let tcs = scan_top_level(&r_in, "hp:tc")?;
    for tc in tcs.into_iter().rev() {
        let new_tc_in =
            set_tc_child_tag_attr(tc.inner(&r_in), "hp:cellSz", "height", &height_hu.to_string())?;
        let t_attrs = tc.attrs(&r_in, "hp:tc").to_string();
        r_in = splice_span(&r_in, &tc, &format!("<hp:tc{t_attrs}>{new_tc_in}</hp:tc>"));
    }
    let new_r = format!("<hp:tr{}>{r_in}</hp:tr>", rw.attrs(tbl_inner, "hp:tr"));
    let new_tbl = format!(
        "<hp:tbl{}>{}</hp:tbl>",
        loc.span.attrs(&sec_xml, "hp:tbl"),
        splice_span(tbl_inner, rw, &new_r)
    );
    doc.archive
        .write_text(loc.section, splice_span(&sec_xml, &loc.span, &new_tbl));
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn op_set_cell_padding(
    doc: &mut HwpxDocument,
    table_index: usize,
    row: usize,
    col: usize,
    left_hu: Option<HwpUnit>,
    right_hu: Option<HwpUnit>,
    top_hu: Option<HwpUnit>,
    bottom_hu: Option<HwpUnit>,
) -> Result<(), HwpxError> {
    if left_hu.is_none() && right_hu.is_none() && top_hu.is_none() && bottom_hu.is_none() {
        return Err(HwpxError::InvalidArgument(
            "set_cell_padding: at least one margin side required".to_string(),
        ));
    }
    let loc = doc.get_table(table_index)?;
    let sec_xml = doc.archive.read_text(&loc.section)?;
    let tbl_inner = loc.span.inner(&sec_xml);
    let rows = scan_top_level(tbl_inner, "hp:tr")?;
    let rw = rows.get(row).ok_or_else(|| {
        HwpxError::OutOfRange(format!("set_cell_padding: row {row} out of range"))
    })?;
    let r_in = rw.inner(tbl_inner);
    let tcs = scan_top_level(r_in, "hp:tc")?;
    let tc_idx = find_cell_index_in_row(r_in, &tcs, col).ok_or_else(|| {
        HwpxError::OutOfRange(format!("set_cell_padding: col {col} out of range"))
    })?;
    let tc = &tcs[tc_idx];
    let mut tc_in = tc.inner(r_in).to_string();

    if let Some(cm) = scan_top_level(&tc_in, "hp:cellMargin")?.last().cloned() {
        let mut tag = cm.full(&tc_in).to_string();
        if let Some(v) = left_hu {
            tag = set_tag_attr(&tag, "left", &v.to_string());
        }
        if let Some(v) = right_hu {
            tag = set_tag_attr(&tag, "right", &v.to_string());
        }
        if let Some(v) = top_hu {
            tag = set_tag_attr(&tag, "top", &v.to_string());
        }
        if let Some(v) = bottom_hu {
            tag = set_tag_attr(&tag, "bottom", &v.to_string());
        }
        tc_in = splice_span(&tc_in, &cm, &tag);
    } else {
        tc_in.push_str(&format!(
            r#"<hp:cellMargin left="{}" right="{}" top="{}" bottom="{}"/>"#,
            left_hu.unwrap_or(0),
            right_hu.unwrap_or(0),
            top_hu.unwrap_or(0),
            bottom_hu.unwrap_or(0)
        ));
    }

    let tc_attrs = set_or_add_attr(tc.attrs(r_in, "hp:tc"), "hasMargin", "1");
    let new_tc = format!("<hp:tc{tc_attrs}>{tc_in}</hp:tc>");
    let new_r_in = splice_span(r_in, tc, &new_tc);
    let new_r = format!("<hp:tr{}>{new_r_in}</hp:tr>", rw.attrs(tbl_inner, "hp:tr"));
    let new_tbl = format!(
        "<hp:tbl{}>{}</hp:tbl>",
        loc.span.attrs(&sec_xml, "hp:tbl"),
        splice_span(tbl_inner, rw, &new_r)
    );
    doc.archive
        .write_text(loc.section, splice_span(&sec_xml, &loc.span, &new_tbl));
    Ok(())
}

pub fn op_fit_table_to_page(
    doc: &mut HwpxDocument,
    table_index: usize,
    target_width_hu: Option<HwpUnit>,
) -> Result<(), HwpxError> {
    let first_sec = doc
        .archive
        .section_names()
        .into_iter()
        .next()
        .ok_or_else(|| HwpxError::MissingPart("Contents/section0.xml".to_string()))?;
    let printable_hu = doc.page_text_width(&first_sec);
    let target_hu = target_width_hu.unwrap_or(printable_hu);

    let loc = doc.get_table(table_index)?;
    let sec_xml = doc.archive.read_text(&loc.section)?;
    let tbl_inner = loc.span.inner(&sec_xml);
    let rows = scan_top_level(tbl_inner, "hp:tr")?;
    if rows.is_empty() {
        return Err(HwpxError::InvalidArgument(
            "fit_table_to_page: table has no rows".to_string(),
        ));
    }
    let cols0 = scan_top_level(rows[0].inner(tbl_inner), "hp:tc")?;
    let col_widths: Vec<i32> = cols0
        .iter()
        .map(|c| {
            read_cell_size(c.inner(rows[0].inner(tbl_inner)))
                .map(|(w, _)| w)
                .unwrap_or(0)
        })
        .collect();
    let current_total: i32 = col_widths.iter().sum();
    let scale = if current_total > 0 {
        target_hu as f64 / current_total as f64
    } else {
        1.0
    };
    let mut new_widths: Vec<i32> = col_widths
        .iter()
        .map(|w| ((*w as f64) * scale).floor() as i32)
        .collect();
    let sum: i32 = new_widths.iter().sum();
    if let Some(last) = new_widths.last_mut() {
        *last += target_hu - sum;
    }

    let mut out = tbl_inner.to_string();
    for rw in rows.into_iter().rev() {
        let mut r_in = rw.inner(&out).to_string();
        let tcs = scan_top_level(&r_in, "hp:tc")?;
        for (ci, tc) in tcs.into_iter().enumerate().rev() {
            let tw = new_widths.get(ci).copied().unwrap_or(new_widths[0]);
            let new_tc_in =
                set_tc_child_tag_attr(tc.inner(&r_in), "hp:cellSz", "width", &tw.to_string())?;
            let t_attrs = tc.attrs(&r_in, "hp:tc").to_string();
            r_in = splice_span(&r_in, &tc, &format!("<hp:tc{t_attrs}>{new_tc_in}</hp:tc>"));
        }
        let r_attrs = rw.attrs(&out, "hp:tr").to_string();
        out = splice_span(&out, &rw, &format!("<hp:tr{r_attrs}>{r_in}</hp:tr>"));
    }
    if let Some(sz) = scan_top_level(&out, "hp:sz")?.first().cloned() {
        let updated = set_tag_attr(sz.full(&out), "width", &target_hu.to_string());
        out = splice_span(&out, &sz, &updated);
    }
    let new_tbl = format!("<hp:tbl{}>{out}</hp:tbl>", loc.span.attrs(&sec_xml, "hp:tbl"));
    doc.archive
        .write_text(loc.section, splice_span(&sec_xml, &loc.span, &new_tbl));
    Ok(())
}

pub fn op_shrink_cell_font(
    doc: &mut HwpxDocument,
    table_index: usize,
    row: usize,
    col: usize,
    size_hu: HwpUnit,
    min_hu: HwpUnit,
) -> Result<(), HwpxError> {
    let eff_hu = size_hu.max(min_hu);
    let loc = doc.get_table(table_index)?;
    let sec_xml = doc.archive.read_text_ro(&loc.section)?;
    let tbl_inner = loc.span.inner(&sec_xml);
    let rows = scan_top_level(tbl_inner, "hp:tr")?;
    let rw = rows.get(row).ok_or_else(|| {
        HwpxError::OutOfRange(format!("shrink_cell_font: row {row} out of range"))
    })?;
    let r_in = rw.inner(tbl_inner);
    let tcs = scan_top_level(r_in, "hp:tc")?;
    let tc_idx = find_cell_index_in_row(r_in, &tcs, col).ok_or_else(|| {
        HwpxError::OutOfRange(format!("shrink_cell_font: col {col} out of range"))
    })?;
    let text = extract_t_text(tcs[tc_idx].inner(r_in))?;
    let trimmed = text.trim();
    if !trimmed.is_empty() {
        op_apply_text_style(
            doc,
            trimmed,
            TextStyleOpts {
                size_hu: Some(eff_hu),
                ..Default::default()
            },
        )?;
    }
    Ok(())
}
