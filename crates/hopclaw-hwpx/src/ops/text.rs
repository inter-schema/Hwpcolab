use hopclaw_types::{
    HighlightValue, HwpUnit, ListStyleMode, ParaIndexTarget, ParagraphAlign, HWPUNIT_PER_MM,
};
use quick_xml::events::Event;
use quick_xml::Reader;

use crate::archive::HwpxArchive;
use crate::error::HwpxError;
use crate::model::{first_char_pr_id, norm_hex, run_with_text, HwpxDocument, HANCOM_NATIVE_STUB};
use crate::xml_util::{
    drop_linesegs, extract_t_text, fresh_id, get_attr, scan_top_level, set_or_add_attr,
    set_tag_attr, splice_span, sync_item_count, xml_escape_text, XmlElementSpan,
};

const INLINE_CTRL_TAGS: &[&str] = &[
    "hp:fwSpace",
    "hp:tab",
    "hp:lineBreak",
    "hp:hyphen",
    "hp:nbSpace",
    "hp:titleMark",
    "hp:insertBegin",
    "hp:insertEnd",
];

const EMBEDDED_OBJ_PREFIXES: &[&str] = &[
    "<hp:pic",
    "<hp:chart",
    "<hp:tbl",
    "<hp:ellipse",
    "<hp:rect",
    "<hp:line",
    "<hp:arc",
    "<hp:polygon",
    "<hp:curve",
    "<hp:equation",
    "<hp:container",
    "<hp:ole",
    "<hp:video",
    "<hp:textart",
    "<hp:connectLine",
    "<hp:compose",
    "<hp:dutmal",
    "<hp:drawText",
];

#[derive(Clone)]
struct StreamToken {
    is_text: bool,
    raw: String,
    run_idx: usize,
}

fn tokenize_stream(stream: &str, run_idx: usize) -> Result<Vec<StreamToken>, HwpxError> {
    let mut reader = Reader::from_str(stream);
    let mut toks = Vec::new();
    let mut cursor = 0usize;

    loop {
        let pos_before = reader.buffer_position() as usize;
        match reader.read_event()? {
            Event::Eof => break,
            Event::Empty(ref e) if INLINE_CTRL_TAGS.contains(&e.name().as_ref()) => {
                let pos_after = reader.buffer_position() as usize;
                if pos_before > cursor {
                    toks.push(StreamToken {
                        is_text: true,
                        raw: stream[cursor..pos_before].to_string(),
                        run_idx,
                    });
                }
                toks.push(StreamToken {
                    is_text: false,
                    raw: stream[pos_before..pos_after].to_string(),
                    run_idx,
                });
                cursor = pos_after;
            }
            _ => {}
        }
    }
    if cursor < stream.len() {
        toks.push(StreamToken {
            is_text: true,
            raw: stream[cursor..].to_string(),
            run_idx,
        });
    }
    Ok(toks)
}

fn splice_token_list(mut cur: Vec<StreamToken>, find: &str, replace: &str) -> (Vec<StreamToken>, usize) {
    let mut count = 0usize;
    let escaped_replace = xml_escape_text(replace);
    for _ in 0..4000 {
        let norm: String = cur
            .iter()
            .filter(|t| t.is_text)
            .map(|t| t.raw.as_str())
            .collect();
        let Some(s) = norm.find(find) else {
            break;
        };
        let e = s + find.len();
        let mut out = Vec::with_capacity(cur.len());
        let mut pos = 0usize;
        let mut inserted = false;
        for tok in cur {
            if !tok.is_text {
                if !(pos > s && pos < e) {
                    out.push(tok);
                }
                continue;
            }
            let ts = pos;
            let te = pos + tok.raw.len();
            pos = te;
            if te <= s || ts >= e {
                out.push(tok);
                continue;
            }
            let before = if ts < s { &tok.raw[..s - ts] } else { "" };
            let after = if te > e { &tok.raw[e - ts..] } else { "" };
            let mut merged = String::with_capacity(before.len() + escaped_replace.len() + after.len());
            merged.push_str(before);
            if !inserted {
                merged.push_str(&escaped_replace);
                inserted = true;
            }
            merged.push_str(after);
            if !merged.is_empty() {
                out.push(StreamToken {
                    is_text: true,
                    raw: merged,
                    run_idx: tok.run_idx,
                });
            }
        }
        cur = out;
        count += 1;
    }
    (cur, count)
}

/// Find every `<hp:t>` element (at any depth) in `xml`.
fn find_all_t_elements(xml: &str) -> Result<Vec<XmlElementSpan>, HwpxError> {
    let mut reader = Reader::from_str(xml);
    let mut out = Vec::new();
    let mut stack: Vec<(usize, usize)> = Vec::new();
    loop {
        let pos_before = reader.buffer_position() as usize;
        match reader.read_event()? {
            Event::Eof => break,
            Event::Start(ref e) if e.name().as_ref() == "hp:t" => {
                let pos_after = reader.buffer_position() as usize;
                stack.push((pos_before, pos_after));
            }
            Event::End(ref e) if e.name().as_ref() == "hp:t" => {
                let pos_after = reader.buffer_position() as usize;
                if let Some((start, open_end)) = stack.pop() {
                    if stack.is_empty() {
                        out.push(XmlElementSpan {
                            start,
                            open_end,
                            close_start: pos_before,
                            end: pos_after,
                            self_closing: false,
                        });
                    }
                }
            }
            _ => {}
        }
    }
    Ok(out)
}

/// Find every leaf `<hp:run>...</hp:run>` (at any depth, not containing nested `<hp:run>`).
fn find_leaf_runs(xml: &str) -> Result<Vec<XmlElementSpan>, HwpxError> {
    let mut reader = Reader::from_str(xml);
    let mut out = Vec::new();
    let mut stack: Vec<(usize, usize, bool)> = Vec::new();
    loop {
        let pos_before = reader.buffer_position() as usize;
        match reader.read_event()? {
            Event::Eof => break,
            Event::Start(ref e) if e.name().as_ref() == "hp:run" => {
                let pos_after = reader.buffer_position() as usize;
                if let Some(parent) = stack.last_mut() {
                    parent.2 = true;
                }
                stack.push((pos_before, pos_after, false));
            }
            Event::End(ref e) if e.name().as_ref() == "hp:run" => {
                let pos_after = reader.buffer_position() as usize;
                if let Some((start, open_end, has_child_run)) = stack.pop() {
                    if !has_child_run {
                        out.push(XmlElementSpan {
                            start,
                            open_end,
                            close_start: pos_before,
                            end: pos_after,
                            self_closing: false,
                        });
                    }
                }
            }
            _ => {}
        }
    }
    Ok(out)
}

fn is_plain_text_run(run_xml: &str) -> bool {
    run_xml.contains("<hp:t") && !EMBEDDED_OBJ_PREFIXES.iter().any(|p| run_xml.contains(p))
}

fn strip_t_wrappers(inner: &str) -> Result<String, HwpxError> {
    let mut reader = Reader::from_str(inner);
    let mut out = String::with_capacity(inner.len());
    let mut cursor = 0usize;
    loop {
        let pos_before = reader.buffer_position() as usize;
        match reader.read_event()? {
            Event::Eof => break,
            Event::Start(ref e) if e.name().as_ref() == "hp:t" => {
                let pos_after = reader.buffer_position() as usize;
                out.push_str(&inner[cursor..pos_before]);
                cursor = pos_after;
            }
            Event::End(ref e) if e.name().as_ref() == "hp:t" => {
                let pos_after = reader.buffer_position() as usize;
                out.push_str(&inner[cursor..pos_before]);
                cursor = pos_after;
            }
            Event::Empty(ref e) if e.name().as_ref() == "hp:t" => {
                let pos_after = reader.buffer_position() as usize;
                out.push_str(&inner[cursor..pos_before]);
                cursor = pos_after;
            }
            _ => {}
        }
    }
    out.push_str(&inner[cursor..]);
    Ok(out)
}

fn replace_text_across_runs(xml: &str, find: &str, replace: &str) -> Result<(String, usize), HwpxError> {
    let runs = find_leaf_runs(xml)?;
    if runs.is_empty() {
        return Ok((xml.to_string(), 0));
    }

    let mut groups: Vec<Vec<XmlElementSpan>> = Vec::new();
    for r in runs {
        let raw = r.full(xml);
        if is_plain_text_run(raw) {
            if let Some(last_grp) = groups.last_mut() {
                let prev_end = last_grp.last().expect("non-empty group").end;
                if xml[prev_end..r.start].chars().all(char::is_whitespace) {
                    last_grp.push(r);
                    continue;
                }
            }
            groups.push(vec![r]);
        } else if !groups.last().map(|g| g.is_empty()).unwrap_or(true) {
            groups.push(Vec::new());
        }
    }
    groups.retain(|g| !g.is_empty());

    let mut total_count = 0usize;
    let mut out = xml.to_string();
    for grp in groups.into_iter().rev() {
        let mut toks = Vec::new();
        let mut opens = Vec::with_capacity(grp.len());
        for (idx, r) in grp.iter().enumerate() {
            opens.push(&xml[r.start..r.open_end]);
            let stream = strip_t_wrappers(r.inner(xml))?;
            toks.extend(tokenize_stream(&stream, idx)?);
        }
        let flat: String = toks
            .iter()
            .filter(|t| t.is_text)
            .map(|t| t.raw.as_str())
            .collect();
        if !flat.contains(find) {
            continue;
        }
        let (new_toks, c) = splice_token_list(toks, find, replace);
        total_count += c;
        let mut by_run = vec![String::new(); grp.len()];
        for tok in new_toks {
            by_run[tok.run_idx].push_str(&tok.raw);
        }
        let mut rebuilt = String::new();
        for (idx, open) in opens.iter().enumerate() {
            rebuilt.push_str(open);
            rebuilt.push_str("<hp:t>");
            rebuilt.push_str(&by_run[idx]);
            rebuilt.push_str("</hp:t></hp:run>");
        }
        let grp_start = grp.first().expect("non-empty").start;
        let grp_end = grp.last().expect("non-empty").end;
        out.replace_range(grp_start..grp_end, &rebuilt);
    }
    Ok((out, total_count))
}

pub fn op_replace_text(doc: &mut HwpxDocument, find: &str, replace: &str) -> Result<usize, HwpxError> {
    if find.is_empty() {
        return Err(HwpxError::InvalidArgument(
            "replace_text: \"find\" must be non-empty".to_string(),
        ));
    }
    let mut total = 0usize;
    for name in doc.archive.section_names() {
        let mut xml = doc.archive.read_text(&name)?;
        let mut changed = false;

        // Pass 1: within each single <hp:t> node (control-aware).
        let t_spans = find_all_t_elements(&xml)?;
        for span in t_spans.into_iter().rev() {
            let toks = tokenize_stream(span.inner(&xml), 0)?;
            let (new_toks, c) = splice_token_list(toks, find, replace);
            if c > 0 {
                total += c;
                changed = true;
                let inner_rebuilt: String = new_toks.into_iter().map(|t| t.raw).collect();
                let open = &xml[span.start..span.open_end];
                let replacement = format!("{open}{inner_rebuilt}</hp:t>");
                xml = splice_span(&xml, &span, &replacement);
            }
        }

        // Pass 2: across adjacent plain text runs.
        let (cross_xml, cross_count) = replace_text_across_runs(&xml, find, replace)?;
        if cross_count > 0 {
            xml = cross_xml;
            total += cross_count;
            changed = true;
        }

        if changed {
            doc.archive.write_text(name, drop_linesegs(&xml)?);
        }
    }
    Ok(total)
}

fn resolve_para_idx(
    doc: &HwpxDocument,
    paras: &[crate::model::ParagraphLocation],
    index: Option<isize>,
    target_text: Option<&str>,
) -> Result<Option<usize>, HwpxError> {
    if let Some(target) = target_text.filter(|s| !s.is_empty()) {
        for (idx, p) in paras.iter().enumerate() {
            let xml = doc.archive.read_text_ro(&p.section)?;
            if extract_t_text(p.span.inner(&xml))?.contains(target) {
                return Ok(Some(idx));
            }
        }
    }
    if paras.is_empty() {
        return Ok(None);
    }
    match index {
        None | Some(-1) => Ok(Some(paras.len() - 1)),
        Some(n) if n < -1 => Err(HwpxError::OutOfRange(format!(
            "paragraph index {n} out of range (0..{})",
            paras.len() - 1
        ))),
        Some(n) => {
            let u = n as usize;
            Ok(Some(if u >= paras.len() { paras.len() - 1 } else { u }))
        }
    }
}

pub fn op_set_paragraph_text(
    doc: &mut HwpxDocument,
    index: usize,
    text: &str,
) -> Result<(), HwpxError> {
    let paras = doc.paragraphs()?;
    let idx = resolve_para_idx(doc, &paras, Some(index as isize), None)?.ok_or_else(|| {
        HwpxError::OutOfRange("set_paragraph_text: document has no paragraphs".to_string())
    })?;
    let loc = &paras[idx];
    let xml = doc.archive.read_text(&loc.section)?;
    let attrs = loc.span.attrs(&xml, "hp:p");
    let p_inner = loc.span.inner(&xml);
    let char_pr_id = first_char_pr_id(p_inner);
    let mut preserved = String::new();
    for ctrl in scan_top_level(p_inner, "hp:ctrl")? {
        if ctrl.inner(p_inner).contains("<hp:colPr") {
            preserved.push_str(ctrl.full(p_inner));
        }
    }
    for sec_pr in scan_top_level(p_inner, "hp:secPr")? {
        preserved.push_str(sec_pr.full(p_inner));
    }
    let body = if preserved.is_empty() {
        run_with_text(&char_pr_id, text)
    } else {
        format!(
            r#"<hp:run charPrIDRef="{char_pr_id}">{preserved}<hp:t>{}</hp:t></hp:run>"#,
            xml_escape_text(text)
        )
    };
    let rebuilt = format!("<hp:p{attrs}>{body}</hp:p>");
    let updated = drop_linesegs(&splice_span(&xml, &loc.span, &rebuilt))?;
    doc.archive.write_text(loc.section.clone(), updated);
    Ok(())
}

pub fn op_append_paragraph(
    doc: &mut HwpxDocument,
    text: &str,
    align: Option<ParagraphAlign>,
) -> Result<usize, HwpxError> {
    let names = doc.archive.section_names();
    let last = names
        .last()
        .ok_or_else(|| HwpxError::MissingPart("Contents/section0.xml".to_string()))?
        .clone();
    let mut xml = doc.archive.read_text(&last)?;
    let paras = scan_top_level(&xml, "hp:p")?;
    let (attrs, char_pr_id) = if let Some(last_p) = paras.last() {
        let a = set_or_add_attr(last_p.attrs(&xml, "hp:p"), "id", &fresh_id());
        let c = first_char_pr_id(last_p.inner(&xml));
        (a, c)
    } else {
        (
            format!(
                r#" id="{}" paraPrIDRef="0" styleIDRef="0" pageBreak="0" columnBreak="0" merged="0""#,
                fresh_id()
            ),
            "0".to_string(),
        )
    };
    let para = format!("<hp:p{attrs}>{}</hp:p>", run_with_text(&char_pr_id, text));
    if let Some(close_pos) = xml.rfind("</hs:sec>") {
        xml.insert_str(close_pos, &para);
    } else {
        xml.push_str(&para);
    }
    doc.archive.write_text(last, xml);

    let new_idx = doc.paragraphs()?.len().saturating_sub(1);
    if let Some(al) = align {
        let target = if text.is_empty() { None } else { Some(text) };
        op_apply_paragraph_style(
            doc,
            Some(ParaIndexTarget::Index(-1)),
            target,
            Some(al),
            None,
            None,
            None,
            None,
        )?;
    }
    Ok(new_idx)
}

pub fn op_append_heading(
    doc: &mut HwpxDocument,
    _level: u8,
    text: &str,
    size_hu: HwpUnit,
) -> Result<(), HwpxError> {
    op_append_paragraph(doc, text, None)?;
    if !text.is_empty() {
        op_apply_text_style(
            doc,
            text,
            TextStyleOpts {
                bold: Some(true),
                size_hu: Some(size_hu),
                ..Default::default()
            },
        )?;
    }
    Ok(())
}

#[derive(Default)]
pub struct TextStyleOpts<'a> {
    pub font_family: Option<&'a str>,
    pub size_hu: Option<HwpUnit>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
    pub color: Option<&'a str>,
    pub highlight: Option<&'a HighlightValue>,
    pub letter_spacing: Option<f64>,
    pub char_ratio: Option<f64>,
}

fn apply_highlight(
    doc: &mut HwpxDocument,
    target: &str,
    highlight: &HighlightValue,
) -> Result<(), HwpxError> {
    match highlight {
        HighlightValue::Bool(false) => {
            for name in doc.archive.section_names() {
                let xml = doc.archive.read_text(&name)?;
                let mut reader = Reader::from_str(&xml);
                let mut out = String::with_capacity(xml.len());
                let mut cursor = 0usize;
                let mut changed = false;
                loop {
                    let pos_before = reader.buffer_position() as usize;
                    match reader.read_event()? {
                        Event::Eof => break,
                        Event::Empty(ref e)
                            if matches!(e.name().as_ref(), "hp:markpenBegin" | "hp:markpenEnd") =>
                        {
                            let pos_after = reader.buffer_position() as usize;
                            out.push_str(&xml[cursor..pos_before]);
                            cursor = pos_after;
                            changed = true;
                        }
                        _ => {}
                    }
                }
                if changed {
                    out.push_str(&xml[cursor..]);
                    doc.archive.write_text(name, drop_linesegs(&out)?);
                }
            }
        }
        HighlightValue::Bool(true) | HighlightValue::Color(_) => {
            let color = match highlight {
                HighlightValue::Bool(true) => "#FFFF00".to_string(),
                HighlightValue::Color(c) => norm_hex(c),
                HighlightValue::Bool(false) => unreachable!(),
            };
            let begin = format!(r#"<hp:markpenBegin color="{color}"/>"#);
            let end = "<hp:markpenEnd/>";
            for name in doc.archive.section_names() {
                let xml = doc.archive.read_text(&name)?;
                for t_span in find_all_t_elements(&xml)? {
                    let inner = t_span.inner(&xml);
                    if let Some(pos) = inner.find(target) {
                        let new_inner = format!(
                            "{}{}{}{}{}",
                            &inner[..pos],
                            begin,
                            target,
                            end,
                            &inner[pos + target.len()..]
                        );
                        let open = &xml[t_span.start..t_span.open_end];
                        let updated = splice_span(&xml, &t_span, &format!("{open}{new_inner}</hp:t>"));
                        doc.archive.write_text(name, drop_linesegs(&updated)?);
                        return Ok(());
                    }
                }
            }
        }
    }
    Ok(())
}

fn find_run_for_text(xml: &str, target: &str) -> Result<Option<(XmlElementSpan, String)>, HwpxError> {
    for r in find_leaf_runs(xml)? {
        let inner = r.inner(xml);
        if inner.contains("<hp:tbl") {
            continue;
        }
        if extract_t_text(inner)?.contains(target) {
            let src_ref = get_attr(r.attrs(xml, "hp:run"), "charPrIDRef")
                .unwrap_or_else(|| "0".to_string());
            return Ok(Some((r, src_ref)));
        }
    }
    Ok(None)
}

fn toggle_child_empty_tag(inner: &str, tag: &str, on: bool) -> Result<String, HwpxError> {
    let existing = scan_top_level(inner, tag)?;
    if on && existing.is_empty() {
        Ok(format!("<{tag}/>{inner}"))
    } else if !on && !existing.is_empty() {
        let mut out = inner.to_string();
        for span in existing.into_iter().rev() {
            out = splice_span(&out, &span, "");
        }
        Ok(out)
    } else {
        Ok(inner.to_string())
    }
}

fn replace_or_prepend_tag(inner: &str, tag: &str, snippet: &str) -> Result<String, HwpxError> {
    let existing = scan_top_level(inner, tag)?;
    if let Some(first) = existing.first() {
        Ok(splice_span(inner, first, snippet))
    } else {
        Ok(format!("{snippet}{inner}"))
    }
}

fn replace_or_append_tag(inner: &str, tag: &str, snippet: &str) -> Result<String, HwpxError> {
    let existing = scan_top_level(inner, tag)?;
    if let Some(first) = existing.first() {
        Ok(splice_span(inner, first, snippet))
    } else {
        Ok(format!("{inner}{snippet}"))
    }
}

pub fn op_apply_text_style(
    doc: &mut HwpxDocument,
    target_text: &str,
    opts: TextStyleOpts<'_>,
) -> Result<(), HwpxError> {
    if let Some(hl) = opts.highlight {
        apply_highlight(doc, target_text, hl)?;
    }

    let wants_char_pr = opts.font_family.is_some()
        || opts.size_hu.is_some()
        || opts.bold.is_some()
        || opts.italic.is_some()
        || opts.underline.is_some()
        || opts.color.is_some()
        || opts.letter_spacing.is_some()
        || opts.char_ratio.is_some();
    if !wants_char_pr {
        return Ok(());
    }

    let header_name = doc.require_header_name()?;
    let mut hit: Option<(String, XmlElementSpan, String)> = None;
    for name in doc.archive.section_names() {
        let xml = doc.archive.read_text_ro(&name)?;
        if let Some((run, src_ref)) = find_run_for_text(&xml, target_text)? {
            hit = Some((name, run, src_ref));
            break;
        }
    }
    let Some((hit_section, hit_run, source_ref)) = hit else {
        return Ok(());
    };

    let header = doc.archive.read_text(&header_name)?;
    let char_prs = scan_top_level(&header, "hh:charPr")?;
    if char_prs.is_empty() {
        return Err(HwpxError::MissingPart("no <hh:charPr> in header.xml".to_string()));
    }
    let base = char_prs
        .iter()
        .find(|c| get_attr(c.attrs(&header, "hh:charPr"), "id").as_deref() == Some(source_ref.as_str()))
        .unwrap_or(&char_prs[0]);

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

    let mut attrs = set_or_add_attr(base.attrs(&header, "hh:charPr"), "id", &use_id);
    let mut inner = base.inner(&header).to_string();

    if let Some(size_hu) = opts.size_hu {
        attrs = set_or_add_attr(&attrs, "height", &size_hu.to_string());
    }
    if let Some(color) = opts.color {
        attrs = set_or_add_attr(&attrs, "textColor", &norm_hex(color));
    }
    if let Some(bold) = opts.bold {
        inner = toggle_child_empty_tag(&inner, "hh:bold", bold)?;
    }
    if let Some(italic) = opts.italic {
        inner = toggle_child_empty_tag(&inner, "hh:italic", italic)?;
    }
    if let Some(underline) = opts.underline {
        if underline {
            let ul = r##"<hh:underline type="BOTTOM" shape="SOLID" color="#000000"/>"##;
            if scan_top_level(&inner, "hh:underline")?.is_empty() {
                inner = format!("{ul}{inner}");
            }
        } else {
            inner = toggle_child_empty_tag(&inner, "hh:underline", false)?;
        }
    }
    if let Some(r) = opts.char_ratio {
        let rv = r.round() as i64;
        let ratio_tag = format!(
            r#"<hh:ratio hangul="{rv}" latin="{rv}" hanja="{rv}" japanese="{rv}" other="{rv}" symbol="{rv}" user="{rv}"/>"#
        );
        inner = replace_or_append_tag(&inner, "hh:ratio", &ratio_tag)?;
    }
    if let Some(sp) = opts.letter_spacing {
        let sv = sp.round() as i64;
        let sp_tag = format!(
            r#"<hh:spacing hangul="{sv}" latin="{sv}" hanja="{sv}" japanese="{sv}" other="{sv}" symbol="{sv}" user="{sv}"/>"#
        );
        inner = replace_or_append_tag(&inner, "hh:spacing", &sp_tag)?;
    }
    if let Some(face) = opts.font_family {
        let fid = doc.find_font_id_by_face(face)?.ok_or_else(|| {
            HwpxError::InvalidArgument(format!(
                "apply_text_style: font \"{face}\" not registered in <hh:fontfaces>"
            ))
        })?;
        let font_ref = format!(
            r#"<hh:fontRef hangul="{fid}" latin="{fid}" hanja="{fid}" japanese="{fid}" other="{fid}" symbol="{fid}" user="{fid}"/>"#
        );
        inner = replace_or_prepend_tag(&inner, "hh:fontRef", &font_ref)?;
    }

    let updated_char_pr = format!("<hh:charPr{attrs}>{inner}</hh:charPr>");
    let new_header = if let Some(ph) = placeholder {
        splice_span(&header, ph, &updated_char_pr)
    } else {
        let combined = format!("{}{updated_char_pr}", base.full(&header));
        let h2 = splice_span(&header, base, &combined);
        sync_item_count(&h2, "hh:charProperties", "hh:charPr")?
    };
    doc.archive.write_text(header_name, new_header);

    let sec_xml = doc.archive.read_text(&hit_section)?;
    let rewritten = split_or_retarget_run(&sec_xml, &hit_run, target_text, &use_id)?;
    doc.archive.write_text(hit_section, drop_linesegs(&rewritten)?);
    Ok(())
}

fn split_or_retarget_run(
    xml: &str,
    run: &XmlElementSpan,
    target: &str,
    new_id: &str,
) -> Result<String, HwpxError> {
    let run_inner = run.inner(xml);
    let t_spans = scan_top_level(run_inner, "hp:t")?;
    let old_attrs = run.attrs(xml, "hp:run");
    let new_attrs = set_or_add_attr(old_attrs, "charPrIDRef", new_id);

    if t_spans.len() == 1 && t_spans[0].start == 0 && t_spans[0].end == run_inner.len() {
        let t_attrs = t_spans[0].attrs(run_inner, "hp:t");
        let raw_text = t_spans[0].inner(run_inner);
        if !raw_text.contains('<') {
            if let Some(idx) = raw_text.find(target) {
                if idx == 0 && target.len() == raw_text.len() {
                    return Ok(splice_span(
                        xml,
                        run,
                        &format!("<hp:run{new_attrs}>{run_inner}</hp:run>"),
                    ));
                }
                let before = &raw_text[..idx];
                let after = &raw_text[idx + target.len()..];
                let mut rebuilt = String::new();
                if !before.is_empty() {
                    rebuilt.push_str(&format!(
                        "<hp:run{old_attrs}><hp:t{t_attrs}>{before}</hp:t></hp:run>"
                    ));
                }
                rebuilt.push_str(&format!(
                    "<hp:run{new_attrs}><hp:t{t_attrs}>{target}</hp:t></hp:run>"
                ));
                if !after.is_empty() {
                    rebuilt.push_str(&format!(
                        "<hp:run{old_attrs}><hp:t{t_attrs}>{after}</hp:t></hp:run>"
                    ));
                }
                return Ok(splice_span(xml, run, &rebuilt));
            }
        }
    }

    Ok(splice_span(
        xml,
        run,
        &format!("<hp:run{new_attrs}>{run_inner}</hp:run>"),
    ))
}

#[derive(Default)]
struct ParaPrVals {
    align: Option<String>,
    indent: Option<HwpUnit>,
    margin_left: Option<HwpUnit>,
    margin_right: Option<HwpUnit>,
    spacing_before: Option<HwpUnit>,
    spacing_after: Option<HwpUnit>,
    line_spacing: Option<i64>,
}

fn read_para_pr_vals(doc: &HwpxDocument, ppr_id: &str) -> Result<ParaPrVals, HwpxError> {
    let Some(header_name) = doc.archive.header_name() else {
        return Ok(ParaPrVals::default());
    };
    let header = doc.archive.read_text_ro(&header_name)?;
    let Some(pp) = scan_top_level(&header, "hh:paraPr")?
        .into_iter()
        .find(|p| get_attr(p.attrs(&header, "hh:paraPr"), "id").as_deref() == Some(ppr_id))
    else {
        return Ok(ParaPrVals::default());
    };
    let blk = pp.inner(&header);
    let mut vals = ParaPrVals::default();
    if let Some(al) = scan_top_level(blk, "hh:align")?.first() {
        vals.align = get_attr(al.attrs(blk, "hh:align"), "horizontal");
    }
    if let Some(ls) = scan_top_level(blk, "hh:lineSpacing")?.first() {
        vals.line_spacing = get_attr(ls.attrs(blk, "hh:lineSpacing"), "value")
            .and_then(|s| s.parse::<i64>().ok());
    }
    let read_margin_tag = |frag: &str, tag: &str| -> Option<HwpUnit> {
        scan_top_level(frag, tag)
            .ok()
            .and_then(|v| v.into_iter().next())
            .and_then(|s| get_attr(s.attrs(frag, tag), "value"))
            .and_then(|s| s.parse::<HwpUnit>().ok())
    };
    if let Some(mg) = scan_top_level(blk, "hh:margin")?.first() {
        let m_inner = mg.inner(blk);
        vals.indent = read_margin_tag(m_inner, "hh:intent");
        vals.margin_left = read_margin_tag(m_inner, "hh:left");
        vals.margin_right = read_margin_tag(m_inner, "hh:right");
        vals.spacing_before = read_margin_tag(m_inner, "hh:prev");
        vals.spacing_after = read_margin_tag(m_inner, "hh:next");
    }
    if vals.spacing_before.is_none() {
        if let Some(def) = scan_top_level(blk, "hp:default")?.first() {
            let d_inner = def.inner(blk);
            let scale = |v: i32| ((v as f64 / 200.0) * HWPUNIT_PER_MM).round() as HwpUnit;
            vals.indent = vals.indent.or_else(|| read_margin_tag(d_inner, "hc:intent").map(scale));
            vals.margin_left = vals
                .margin_left
                .or_else(|| read_margin_tag(d_inner, "hc:left").map(scale));
            vals.margin_right = vals
                .margin_right
                .or_else(|| read_margin_tag(d_inner, "hc:right").map(scale));
            vals.spacing_before = vals
                .spacing_before
                .or_else(|| read_margin_tag(d_inner, "hc:prev").map(scale));
            vals.spacing_after = vals
                .spacing_after
                .or_else(|| read_margin_tag(d_inner, "hc:next").map(scale));
        }
    }
    Ok(vals)
}

fn para_pr_is_plain(inner: &str) -> Result<bool, HwpxError> {
    let mut vals = Vec::new();
    for prefix in ["hh", "hc"] {
        for name in ["intent", "left", "right", "prev", "next"] {
            let tag = format!("{prefix}:{name}");
            for sp in scan_top_level(inner, &tag)? {
                if let Some(v) = get_attr(sp.attrs(inner, &tag), "value").and_then(|s| s.parse::<i64>().ok()) {
                    vals.push(v);
                }
            }
        }
    }
    Ok(!vals.is_empty() && vals.iter().all(|v| *v == 0))
}

#[allow(clippy::too_many_arguments)]
pub fn ensure_clean_para_pr(
    doc: &mut HwpxDocument,
    align: Option<&str>,
    indent_hu: Option<HwpUnit>,
    margin_left_hu: Option<HwpUnit>,
    margin_right_hu: Option<HwpUnit>,
    spacing_before_hu: Option<HwpUnit>,
    spacing_after_hu: Option<HwpUnit>,
    line_spacing_percent: Option<i64>,
) -> Result<Option<String>, HwpxError> {
    let header_name = doc.require_header_name()?;
    let mut header = doc.archive.read_text(&header_name)?;
    let has_box = indent_hu.is_some()
        || margin_left_hu.is_some()
        || margin_right_hu.is_some()
        || spacing_before_hu.is_some()
        || spacing_after_hu.is_some()
        || line_spacing_percent.is_some();

    if let (Some(al), false) = (align, has_box) {
        for pp in scan_top_level(&header, "hh:paraPr")? {
            let inner = pp.inner(&header);
            let h_type = scan_top_level(inner, "hh:heading")?
                .first()
                .and_then(|h| get_attr(h.attrs(inner, "hh:heading"), "type"));
            let h_align = scan_top_level(inner, "hh:align")?
                .first()
                .and_then(|a| get_attr(a.attrs(inner, "hh:align"), "horizontal"));
            if h_type.as_deref() == Some("NONE")
                && h_align.as_deref() == Some(al)
                && para_pr_is_plain(inner)?
            {
                return Ok(get_attr(pp.attrs(&header, "hh:paraPr"), "id"));
            }
        }
    }

    let Ok(stub) = HwpxArchive::from_bytes(HANCOM_NATIVE_STUB.to_vec()) else {
        return Ok(None);
    };
    let Ok(stub_header) = stub.read_text_ro("Contents/header.xml") else {
        return Ok(None);
    };
    let Some(tmpl) = scan_top_level(&stub_header, "hh:paraPr")?
        .into_iter()
        .find(|p| get_attr(p.attrs(&stub_header, "hh:paraPr"), "id").as_deref() == Some("2"))
    else {
        return Ok(None);
    };

    let pps = scan_top_level(&header, "hh:paraPr")?;
    let max_id = pps
        .iter()
        .filter_map(|p| {
            get_attr(p.attrs(&header, "hh:paraPr"), "id").and_then(|s| s.parse::<usize>().ok())
        })
        .max()
        .unwrap_or(0);
    let new_pp_id = (max_id + 1).to_string();

    let mut clean_inner = tmpl.inner(&stub_header).to_string();
    if let Some(h) = scan_top_level(&clean_inner, "hh:heading")?.first().cloned() {
        clean_inner = splice_span(
            &clean_inner,
            &h,
            r#"<hh:heading type="NONE" idRef="0" level="0"/>"#,
        );
    }
    if let Some(al) = align {
        if let Some(a) = scan_top_level(&clean_inner, "hh:align")?.first().cloned() {
            clean_inner = splice_span(
                &clean_inner,
                &a,
                &format!(r#"<hh:align horizontal="{al}" vertical="BASELINE"/>"#),
            );
        }
    }
    if has_box {
        let conv = |v: Option<HwpUnit>, mult: f64| -> i64 {
            ((v.unwrap_or(0) as f64 / HWPUNIT_PER_MM) * mult).round() as i64
        };
        let mk_margin = |mult: f64| {
            format!(
                r#"<hh:margin><hc:intent value="{}" unit="HWPUNIT"/><hc:left value="{}" unit="HWPUNIT"/><hc:right value="{}" unit="HWPUNIT"/><hc:prev value="{}" unit="HWPUNIT"/><hc:next value="{}" unit="HWPUNIT"/></hh:margin>"#,
                conv(indent_hu, mult),
                conv(margin_left_hu, mult),
                conv(margin_right_hu, mult),
                conv(spacing_before_hu, mult),
                conv(spacing_after_hu, mult),
            )
        };
        if let Some(sw) = scan_top_level(&clean_inner, "hp:switch")?.first().cloned() {
            let mut sw_inner = sw.inner(&clean_inner).to_string();
            if let Some(case_el) = scan_top_level(&sw_inner, "hp:case")?.first().cloned() {
                let mut c_inner = case_el.inner(&sw_inner).to_string();
                if let Some(mg) = scan_top_level(&c_inner, "hh:margin")?.first().cloned() {
                    c_inner = splice_span(&c_inner, &mg, &mk_margin(100.0));
                }
                if let Some(ls) = line_spacing_percent {
                    if let Some(ls_el) = scan_top_level(&c_inner, "hh:lineSpacing")?.first().cloned() {
                        c_inner = splice_span(
                            &c_inner,
                            &ls_el,
                            &format!(r#"<hh:lineSpacing type="PERCENT" value="{ls}" unit="HWPUNIT"/>"#),
                        );
                    }
                }
                let c_attrs = case_el.attrs(&sw_inner, "hp:case");
                sw_inner = splice_span(
                    &sw_inner,
                    &case_el,
                    &format!("<hp:case{c_attrs}>{c_inner}</hp:case>"),
                );
            }
            if let Some(def_el) = scan_top_level(&sw_inner, "hp:default")?.first().cloned() {
                let mut d_inner = def_el.inner(&sw_inner).to_string();
                if let Some(mg) = scan_top_level(&d_inner, "hh:margin")?.first().cloned() {
                    d_inner = splice_span(&d_inner, &mg, &mk_margin(200.0));
                }
                if let Some(ls) = line_spacing_percent {
                    if let Some(ls_el) = scan_top_level(&d_inner, "hh:lineSpacing")?.first().cloned() {
                        d_inner = splice_span(
                            &d_inner,
                            &ls_el,
                            &format!(r#"<hh:lineSpacing type="PERCENT" value="{ls}" unit="HWPUNIT"/>"#),
                        );
                    }
                }
                sw_inner = splice_span(
                    &sw_inner,
                    &def_el,
                    &format!("<hp:default>{d_inner}</hp:default>"),
                );
            }
            clean_inner = splice_span(&clean_inner, &sw, &format!("<hp:switch>{sw_inner}</hp:switch>"));
        }
    }

    let pp_attrs = set_or_add_attr(tmpl.attrs(&stub_header, "hh:paraPr"), "id", &new_pp_id);
    let clean_pp = format!("<hh:paraPr{pp_attrs}>{clean_inner}</hh:paraPr>");
    header = header.replacen("</hh:paraProperties>", &format!("{clean_pp}</hh:paraProperties>"), 1);
    header = sync_item_count(&header, "hh:paraProperties", "hh:paraPr")?;
    if !header.contains("xmlns:hwpunitchar=") {
        header = header.replacen(
            r#"xmlns:ooxmlchart="http://www.hancom.co.kr/hwpml/2016/ooxmlchart""#,
            r#"xmlns:ooxmlchart="http://www.hancom.co.kr/hwpml/2016/ooxmlchart" xmlns:hwpunitchar="http://www.hancom.co.kr/hwpml/2016/HwpUnitChar""#,
            1,
        );
    }
    doc.archive.write_text(header_name, header);
    Ok(Some(new_pp_id))
}

fn align_str(al: ParagraphAlign) -> &'static str {
    match al {
        ParagraphAlign::Left => "LEFT",
        ParagraphAlign::Center => "CENTER",
        ParagraphAlign::Right => "RIGHT",
        ParagraphAlign::Justify => "JUSTIFY",
        ParagraphAlign::Distribute => "DISTRIBUTE",
    }
}

#[allow(clippy::too_many_arguments)]
pub fn op_apply_paragraph_style(
    doc: &mut HwpxDocument,
    para_index: Option<ParaIndexTarget>,
    target_text: Option<&str>,
    align: Option<ParagraphAlign>,
    indent_hu: Option<HwpUnit>,
    line_spacing_percent: Option<f64>,
    spacing_before_hu: Option<HwpUnit>,
    spacing_after_hu: Option<HwpUnit>,
) -> Result<(), HwpxError> {
    let idx_hint = para_index.map(|t| match t {
        ParaIndexTarget::Index(i) => i,
        ParaIndexTarget::Last => -1,
    });
    let paras_initial = doc.paragraphs()?;
    let resolved = resolve_para_idx(doc, &paras_initial, idx_hint, target_text)?;
    let found_in_body = match (resolved, target_text) {
        (Some(i), Some(t)) if !t.is_empty() => {
            let xml = doc.archive.read_text_ro(&paras_initial[i].section)?;
            extract_t_text(paras_initial[i].span.inner(&xml))?.contains(t)
        }
        _ => false,
    };

    let line_sp = line_spacing_percent.map(|v| v.round() as i64);
    let align_s = align.map(align_str);

    // Search inside table cells if target_text was provided and not found in body paragraphs.
    if let (Some(t), false) = (target_text.filter(|s| !s.is_empty()), found_in_body) {
        for s_name in doc.archive.section_names() {
            let xml = doc.archive.read_text_ro(&s_name)?;
            for p in find_all_paragraphs_any_depth(&xml)? {
                if extract_t_text(p.inner(&xml))?.contains(t) {
                    let ppr_id = ensure_clean_para_pr(
                        doc,
                        align_s,
                        indent_hu,
                        None,
                        None,
                        spacing_before_hu,
                        spacing_after_hu,
                        line_sp,
                    )?
                    .unwrap_or_else(|| "0".to_string());
                    let new_attrs = set_or_add_attr(p.attrs(&xml, "hp:p"), "paraPrIDRef", &ppr_id);
                    let updated = drop_linesegs(&splice_span(
                        &xml,
                        &p,
                        &format!("<hp:p{new_attrs}>{}</hp:p>", p.inner(&xml)),
                    ))?;
                    doc.archive.write_text(s_name, updated);
                    return Ok(());
                }
            }
        }
    }

    let Some(idx) = resolved else {
        return Err(HwpxError::OutOfRange(
            "apply_paragraph_style: document has no paragraphs".to_string(),
        ));
    };
    let cur_para_id = {
        let xml = doc.archive.read_text_ro(&paras_initial[idx].section)?;
        get_attr(paras_initial[idx].span.attrs(&xml, "hp:p"), "paraPrIDRef")
    };
    let cur_vals = match cur_para_id.as_deref() {
        Some(id) => read_para_pr_vals(doc, id)?,
        None => ParaPrVals::default(),
    };

    let eff_align = align_s.map(str::to_string).or(cur_vals.align);
    let eff_indent = indent_hu.or(cur_vals.indent);
    let eff_prev = spacing_before_hu.or(cur_vals.spacing_before);
    let eff_next = spacing_after_hu.or(cur_vals.spacing_after);
    let eff_ls = line_sp.or(cur_vals.line_spacing);

    let ppr_id = if let Some(id) = ensure_clean_para_pr(
        doc,
        eff_align.as_deref(),
        eff_indent,
        cur_vals.margin_left,
        cur_vals.margin_right,
        eff_prev,
        eff_next,
        eff_ls,
    )? {
        id
    } else {
        let header_name = doc.require_header_name()?;
        let header = doc.archive.read_text(&header_name)?;
        let pps = scan_top_level(&header, "hh:paraPr")?;
        let base = pps
            .first()
            .ok_or_else(|| HwpxError::MissingPart("no <hh:paraPr> in header.xml".to_string()))?;
        let max_id = pps
            .iter()
            .filter_map(|p| {
                get_attr(p.attrs(&header, "hh:paraPr"), "id").and_then(|s| s.parse::<usize>().ok())
            })
            .max()
            .unwrap_or(0);
        let new_id = (max_id + 1).to_string();
        let mut inner = base.inner(&header).to_string();
        if let Some(al) = align_s {
            inner = replace_or_prepend_tag(
                &inner,
                "hh:align",
                &format!(r#"<hh:align horizontal="{al}" vertical="BASELINE"/>"#),
            )?;
        }
        if let Some(ls) = line_sp {
            inner = replace_or_prepend_tag(
                &inner,
                "hh:lineSpacing",
                &format!(r#"<hh:lineSpacing type="PERCENT" value="{ls}" unit="HWPUNIT"/>"#),
            )?;
        }
        if let Some(ind) = indent_hu {
            if let Some(sp) = scan_top_level(&inner, "hh:intent")?.first().cloned() {
                let updated_tag = set_tag_attr(sp.full(&inner), "value", &ind.to_string());
                inner = splice_span(&inner, &sp, &updated_tag);
            }
        }
        let attrs = set_or_add_attr(base.attrs(&header, "hh:paraPr"), "id", &new_id);
        let new_pp = format!("<hh:paraPr{attrs}>{inner}</hh:paraPr>");
        let combined = format!("{}{new_pp}", base.full(&header));
        let h2 = sync_item_count(&splice_span(&header, base, &combined), "hh:paraProperties", "hh:paraPr")?;
        doc.archive.write_text(header_name, h2);
        new_id
    };

    let paras_now = doc.paragraphs()?;
    let loc = &paras_now[idx];
    let sec_xml = doc.archive.read_text(&loc.section)?;
    let new_attrs = set_or_add_attr(loc.span.attrs(&sec_xml, "hp:p"), "paraPrIDRef", &ppr_id);
    let rebuilt = format!("<hp:p{new_attrs}>{}</hp:p>", loc.span.inner(&sec_xml));
    let updated = drop_linesegs(&splice_span(&sec_xml, &loc.span, &rebuilt))?;
    doc.archive.write_text(loc.section.clone(), updated);
    Ok(())
}

fn find_all_paragraphs_any_depth(xml: &str) -> Result<Vec<XmlElementSpan>, HwpxError> {
    let mut reader = Reader::from_str(xml);
    let mut out = Vec::new();
    let mut stack: Vec<(usize, usize)> = Vec::new();
    loop {
        let pos_before = reader.buffer_position() as usize;
        match reader.read_event()? {
            Event::Eof => break,
            Event::Start(ref e) if e.name().as_ref() == "hp:p" => {
                let pos_after = reader.buffer_position() as usize;
                stack.push((pos_before, pos_after));
            }
            Event::End(ref e) if e.name().as_ref() == "hp:p" => {
                let pos_after = reader.buffer_position() as usize;
                if let Some((start, open_end)) = stack.pop() {
                    out.push(XmlElementSpan {
                        start,
                        open_end,
                        close_start: pos_before,
                        end: pos_after,
                        self_closing: false,
                    });
                }
            }
            _ => {}
        }
    }
    Ok(out)
}

pub fn op_set_list_style(
    doc: &mut HwpxDocument,
    para_index: Option<usize>,
    target_text: Option<&str>,
    mode: ListStyleMode,
) -> Result<(), HwpxError> {
    let kind = match mode {
        ListStyleMode::Numbered => "NUMBER",
        ListStyleMode::Bullet => "BULLET",
    };
    let paras = doc.paragraphs()?;
    let idx = resolve_para_idx(
        doc,
        &paras,
        para_index.map(|i| i as isize),
        target_text,
    )?
    .ok_or_else(|| HwpxError::OutOfRange("set_list_style: document has no paragraphs".to_string()))?;

    let mut reused = doc.reuse_existing_list_para_pr(kind, 0)?;
    if reused.is_none() && doc.inject_hancom_list_infra()? {
        reused = doc.reuse_existing_list_para_pr(kind, 0)?;
    }

    let loc = &paras[idx];
    let sec_xml = doc.archive.read_text(&loc.section)?;
    if let Some(ppr_id) = reused {
        let new_attrs = set_or_add_attr(loc.span.attrs(&sec_xml, "hp:p"), "paraPrIDRef", &ppr_id);
        let updated = splice_span(
            &sec_xml,
            &loc.span,
            &format!("<hp:p{new_attrs}>{}</hp:p>", loc.span.inner(&sec_xml)),
        );
        doc.archive.write_text(loc.section.clone(), updated);
        return Ok(());
    }

    // Fallback: insert literal glyph prefix into first <hp:t>
    let prefix = match mode {
        ListStyleMode::Bullet => "▶ ",
        ListStyleMode::Numbered => "1. ",
    };
    let p_inner = loc.span.inner(&sec_xml);
    if let Some(t_span) = find_all_t_elements(p_inner)?.first() {
        let new_t = format!(
            "{}{}{}</hp:t>",
            &p_inner[t_span.start..t_span.open_end],
            xml_escape_text(prefix),
            t_span.inner(p_inner)
        );
        let new_p_inner = splice_span(p_inner, t_span, &new_t);
        let updated = splice_span(
            &sec_xml,
            &loc.span,
            &format!("<hp:p{}>{new_p_inner}</hp:p>", loc.span.attrs(&sec_xml, "hp:p")),
        );
        doc.archive.write_text(loc.section.clone(), updated);
    }
    Ok(())
}
