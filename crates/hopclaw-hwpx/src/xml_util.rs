use std::borrow::Cow;
use std::sync::atomic::{AtomicU64, Ordering};

use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, Writer, XmlVersion};

use crate::error::HwpxError;

static ID_COUNTER: AtomicU64 = AtomicU64::new(100_000);

/// Generate a unique positive OWPML integer ID string.
pub fn fresh_id() -> String {
    let next = ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{}", (next % 1_999_999_999) + 1)
}

/// Byte span of a top-level element with tag `tag` inside an XML slice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XmlElementSpan {
    pub start: usize,
    pub open_end: usize,
    pub close_start: usize,
    pub end: usize,
    pub self_closing: bool,
}

impl XmlElementSpan {
    /// Raw attributes slice (after tag name and before `>` or `/>`), e.g. ` id="0" ...`.
    pub fn attrs<'a>(&self, xml: &'a str, tag: &str) -> &'a str {
        let prefix_len = 1 + tag.len(); // `<` + tag
        let raw_open = &xml[self.start..self.open_end];
        if self.self_closing {
            &raw_open[prefix_len..raw_open.len().saturating_sub(2)]
        } else {
            &raw_open[prefix_len..raw_open.len().saturating_sub(1)]
        }
    }

    /// Inner XML slice between opening and closing tags (empty for self-closing).
    pub fn inner<'a>(&self, xml: &'a str) -> &'a str {
        if self.self_closing {
            ""
        } else {
            &xml[self.open_end..self.close_start]
        }
    }

    /// Full element XML slice.
    pub fn full<'a>(&self, xml: &'a str) -> &'a str {
        &xml[self.start..self.end]
    }
}

/// Depth-aware `quick-xml` scan of top-level `tag` elements inside `xml`.
/// Correctly skips nested same-name tags (e.g. `<hp:tbl>` inside `<hp:tbl>`).
pub fn scan_top_level(xml: &str, tag: &str) -> Result<Vec<XmlElementSpan>, HwpxError> {
    let mut reader = Reader::from_str(xml);
    let mut out = Vec::new();
    let mut depth: usize = 0;
    let mut start: usize = 0;
    let mut open_end: usize = 0;

    loop {
        let pos_before = reader.buffer_position() as usize;
        match reader.read_event()? {
            Event::Eof => break,
            Event::Start(ref e) if e.name().as_ref() == tag => {
                let pos_after = reader.buffer_position() as usize;
                if depth == 0 {
                    start = pos_before;
                    open_end = pos_after;
                }
                depth += 1;
            }
            Event::Empty(ref e) if e.name().as_ref() == tag => {
                let pos_after = reader.buffer_position() as usize;
                if depth == 0 {
                    out.push(XmlElementSpan {
                        start: pos_before,
                        open_end: pos_after,
                        close_start: pos_after,
                        end: pos_after,
                        self_closing: true,
                    });
                }
            }
            Event::End(ref e) if e.name().as_ref() == tag => {
                let pos_after = reader.buffer_position() as usize;
                if depth > 0 {
                    depth -= 1;
                    if depth == 0 {
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

/// Replace `[span.start..span.end)` in `xml` with `replacement`.
pub fn splice_span(xml: &str, span: &XmlElementSpan, replacement: &str) -> String {
    let mut out = String::with_capacity(xml.len() - (span.end - span.start) + replacement.len());
    out.push_str(&xml[..span.start]);
    out.push_str(replacement);
    out.push_str(&xml[span.end..]);
    out
}

/// Remove all `<hp:linesegarray>...</hp:linesegarray>` (and self-closing `<hp:linesegarray/>`)
/// blocks using `quick-xml` span scanning so unaffected XML stays byte-identical.
pub fn drop_linesegs(xml: &str) -> Result<String, HwpxError> {
    let mut reader = Reader::from_str(xml);
    let target = "hp:linesegarray";
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    let mut depth: usize = 0;
    let mut start: usize = 0;

    loop {
        let pos_before = reader.buffer_position() as usize;
        match reader.read_event()? {
            Event::Eof => break,
            Event::Start(ref e) if e.name().as_ref() == target => {
                if depth == 0 {
                    start = pos_before;
                }
                depth += 1;
            }
            Event::Empty(ref e) if e.name().as_ref() == target => {
                let pos_after = reader.buffer_position() as usize;
                if depth == 0 {
                    ranges.push((pos_before, pos_after));
                }
            }
            Event::End(ref e) if e.name().as_ref() == target => {
                let pos_after = reader.buffer_position() as usize;
                if depth > 0 {
                    depth -= 1;
                    if depth == 0 {
                        ranges.push((start, pos_after));
                    }
                }
            }
            _ => {}
        }
    }

    if ranges.is_empty() {
        return Ok(xml.to_string());
    }
    let mut out = String::with_capacity(xml.len());
    let mut cursor = 0;
    for (s, e) in ranges {
        out.push_str(&xml[cursor..s]);
        cursor = e;
    }
    out.push_str(&xml[cursor..]);
    Ok(out)
}

/// Escape text content for OWPML XML (`&`, `<`, `>`).
pub fn xml_escape_text(s: &str) -> Cow<'_, str> {
    quick_xml::escape::partial_escape(s)
}

/// Escape attribute value for OWPML XML (`&`, `<`, `>`, `"`, `'`).
pub fn xml_escape_attr(s: &str) -> Cow<'_, str> {
    quick_xml::escape::escape(s)
}

/// Unescape OWPML text entities (`&lt;`, `&gt;`, `&amp;`, `&quot;`, `&apos;`, numeric refs).
pub fn xml_unescape(s: &str) -> String {
    quick_xml::escape::unescape(s)
        .map(Cow::into_owned)
        .unwrap_or_else(|_| s.to_string())
}

/// Read an attribute value from a raw opening-tag attribute string (` id="0" ...`).
pub fn get_attr(attrs: &str, name: &str) -> Option<String> {
    let snippet = format!("<x{attrs}/>");
    let mut reader = Reader::from_str(&snippet);
    if let Ok(Event::Empty(e)) = reader.read_event() {
        for attr in e.attributes().flatten() {
            if attr.key.as_ref() == name {
                return attr
                    .normalized_value(XmlVersion::default())
                    .ok()
                    .map(Cow::into_owned);
            }
        }
    }
    None
}

fn rebuild_start_with_attr(
    e: &BytesStart<'_>,
    tag: &str,
    attr_name: &str,
    attr_value: &str,
    empty: bool,
) -> Result<String, HwpxError> {
    let mut rebuilt = BytesStart::new(tag);
    let mut found = false;
    for attr in e.attributes().flatten() {
        if attr.key.as_ref() == attr_name {
            rebuilt.push_attribute((attr_name, attr_value));
            found = true;
        } else if let Ok(v) = attr.normalized_value(XmlVersion::default()) {
            rebuilt.push_attribute((attr.key.as_ref(), v.as_ref()));
        }
    }
    if !found {
        rebuilt.push_attribute((attr_name, attr_value));
    }
    let mut writer = Writer::new(Vec::new());
    if empty {
        writer.write_event(Event::Empty(rebuilt))?;
    } else {
        writer.write_event(Event::Start(rebuilt))?;
    }
    Ok(String::from_utf8(writer.into_inner())?)
}

/// Set or add an attribute on a raw attribute string (` id="0" ...`), returning
/// an attribute string starting with a single space if non-empty.
pub fn set_or_add_attr(attrs: &str, name: &str, value: &str) -> String {
    let snippet = format!("<x{attrs}/>");
    let mut reader = Reader::from_str(&snippet);
    if let Ok(Event::Empty(e)) = reader.read_event() {
        if let Ok(rendered) = rebuild_start_with_attr(&e, "x", name, value, true) {
            if let Some(inner) = rendered.strip_prefix("<x").and_then(|s| s.strip_suffix("/>")) {
                return inner.to_string();
            }
        }
    }
    format!(" {name}=\"{}\"{attrs}", xml_escape_attr(value))
}

/// Synchronize an attribute (such as `itemCnt` or `secCnt`) on the first
/// element matching `tag` in `xml` using `quick-xml`.
pub fn set_element_attr(
    xml: &str,
    tag: &str,
    attr_name: &str,
    attr_value: &str,
) -> Result<String, HwpxError> {
    let mut reader = Reader::from_str(xml);
    loop {
        let pos_before = reader.buffer_position() as usize;
        match reader.read_event()? {
            Event::Eof => return Ok(xml.to_string()),
            Event::Start(ref e) if e.name().as_ref() == tag => {
                let pos_after = reader.buffer_position() as usize;
                let tag_str = rebuild_start_with_attr(e, tag, attr_name, attr_value, false)?;
                let mut out = String::with_capacity(xml.len() + 16);
                out.push_str(&xml[..pos_before]);
                out.push_str(&tag_str);
                out.push_str(&xml[pos_after..]);
                return Ok(out);
            }
            Event::Empty(ref e) if e.name().as_ref() == tag => {
                let pos_after = reader.buffer_position() as usize;
                let tag_str = rebuild_start_with_attr(e, tag, attr_name, attr_value, true)?;
                let mut out = String::with_capacity(xml.len() + 16);
                out.push_str(&xml[..pos_before]);
                out.push_str(&tag_str);
                out.push_str(&xml[pos_after..]);
                return Ok(out);
            }
            _ => {}
        }
    }
}

/// Synchronize `itemCnt` on a `<hh:*>` wrapper element to equal the exact count
/// of `item_tag` elements inside it.
pub fn sync_item_count(header_xml: &str, list_tag: &str, item_tag: &str) -> Result<String, HwpxError> {
    let lists = scan_top_level(header_xml, list_tag)?;
    let Some(list_span) = lists.first() else {
        return Ok(header_xml.to_string());
    };
    let items = scan_top_level(list_span.inner(header_xml), item_tag)?;
    let count_str = items.len().to_string();
    let new_attrs = set_or_add_attr(list_span.attrs(header_xml, list_tag), "itemCnt", &count_str);
    let rebuilt = if list_span.self_closing {
        format!("<{list_tag}{new_attrs}/>")
    } else {
        format!(
            "<{list_tag}{new_attrs}>{}</{list_tag}>",
            list_span.inner(header_xml)
        )
    };
    Ok(splice_span(header_xml, list_span, &rebuilt))
}

/// Synchronize `secCnt` on `<hh:head>` to match the section count.
pub fn sync_sec_count(header_xml: &str, section_count: usize) -> Result<String, HwpxError> {
    set_element_attr(header_xml, "hh:head", "secCnt", &section_count.to_string())
}

/// Set or replace an attribute on a single self-closing tag string (`<hp:sz .../>`).
pub fn set_tag_attr(tag_xml: &str, name: &str, value: &str) -> String {
    let mut reader = Reader::from_str(tag_xml);
    if let Ok(Event::Empty(e)) = reader.read_event() {
        let tag_name = e.name().into_inner();
        if let Ok(rendered) = rebuild_start_with_attr(&e, tag_name, name, value, true) {
            return rendered;
        }
    }
    tag_xml.to_string()
}

/// Extract concatenated unescaped text across all `<hp:t>` elements in an XML fragment.
pub fn extract_t_text(xml: &str) -> Result<String, HwpxError> {
    let mut reader = Reader::from_str(xml);
    let mut out = String::new();
    let mut in_t: usize = 0;
    loop {
        match reader.read_event()? {
            Event::Eof => break,
            Event::Start(ref e) if e.name().as_ref() == "hp:t" => in_t += 1,
            Event::End(ref e) if e.name().as_ref() == "hp:t" => in_t = in_t.saturating_sub(1),
            Event::Text(ref e) if in_t > 0 => {
                let decoded = e.xml_content(XmlVersion::default());
                out.push_str(&xml_unescape(&decoded));
            }
            _ => {}
        }
    }
    Ok(out)
}

/// Replace all `id="<digits>"` attributes with fresh IDs and drop `<hp:linesegarray>` blocks.
pub fn freshen_ids(xml: &str) -> Result<String, HwpxError> {
    let cleaned = drop_linesegs(xml)?;
    let mut reader = Reader::from_str(&cleaned);
    let mut out = String::with_capacity(cleaned.len() + 32);
    let mut cursor = 0usize;

    loop {
        let pos_before = reader.buffer_position() as usize;
        match reader.read_event()? {
            Event::Eof => break,
            Event::Start(ref e) => {
                let pos_after = reader.buffer_position() as usize;
                let has_numeric_id = e.attributes().flatten().any(|a| {
                    a.key.as_ref() == "id"
                        && a.normalized_value(XmlVersion::default())
                            .map(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit()))
                            .unwrap_or(false)
                });
                if has_numeric_id {
                    out.push_str(&cleaned[cursor..pos_before]);
                    let tag_name = e.name();
                    let tag_str = tag_name.into_inner();
                    out.push_str(&rebuild_start_with_attr(e, tag_str, "id", &fresh_id(), false)?);
                    cursor = pos_after;
                }
            }
            Event::Empty(ref e) => {
                let pos_after = reader.buffer_position() as usize;
                let has_numeric_id = e.attributes().flatten().any(|a| {
                    a.key.as_ref() == "id"
                        && a.normalized_value(XmlVersion::default())
                            .map(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit()))
                            .unwrap_or(false)
                });
                if has_numeric_id {
                    out.push_str(&cleaned[cursor..pos_before]);
                    let tag_name = e.name();
                    let tag_str = tag_name.into_inner();
                    out.push_str(&rebuild_start_with_attr(e, tag_str, "id", &fresh_id(), true)?);
                    cursor = pos_after;
                }
            }
            _ => {}
        }
    }
    out.push_str(&cleaned[cursor..]);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_top_level_skips_nested_same_tag() {
        let xml = r#"<hs:sec><hp:tbl id="1"><hp:tr><hp:tc><hp:subList><hp:p><hp:run><hp:tbl id="2"><hp:tr/></hp:tbl></hp:run></hp:p></hp:subList></hp:tc></hp:tr></hp:tbl><hp:tbl id="3"/></hs:sec>"#;
        let tbls = scan_top_level(xml, "hp:tbl").unwrap();
        assert_eq!(tbls.len(), 2);
        assert_eq!(
            get_attr(tbls[0].attrs(xml, "hp:tbl"), "id").as_deref(),
            Some("1")
        );
        assert_eq!(
            get_attr(tbls[1].attrs(xml, "hp:tbl"), "id").as_deref(),
            Some("3")
        );
        assert!(tbls[1].self_closing);

        let nested = scan_top_level(tbls[0].inner(xml), "hp:tbl").unwrap();
        assert_eq!(nested.len(), 1);
        assert_eq!(
            get_attr(nested[0].attrs(tbls[0].inner(xml), "hp:tbl"), "id").as_deref(),
            Some("2")
        );
    }

    #[test]
    fn drop_linesegs_preserves_surrounding_xml() {
        let xml = r#"<hp:p id="1"><hp:run><hp:t>Hello</hp:t></hp:run><hp:linesegarray><hp:lineseg/></hp:linesegarray></hp:p>"#;
        let cleaned = drop_linesegs(xml).unwrap();
        assert_eq!(
            cleaned,
            r#"<hp:p id="1"><hp:run><hp:t>Hello</hp:t></hp:run></hp:p>"#
        );
    }

    #[test]
    fn sync_item_and_sec_counts() {
        let hdr = r#"<hh:head version="1.5" secCnt="1"><hh:refList><hh:borderFills itemCnt="1"><hh:borderFill id="1"/><hh:borderFill id="2"/></hh:borderFills></hh:refList></hh:head>"#;
        let s1 = sync_item_count(hdr, "hh:borderFills", "hh:borderFill").unwrap();
        assert!(s1.contains(r#"itemCnt="2""#));
        let s2 = sync_sec_count(&s1, 3).unwrap();
        assert!(s2.contains(r#"secCnt="3""#));
    }
}
