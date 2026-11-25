use std::sync::LazyLock;

use hopclaw_types::{ParaIndexTarget, ParagraphAlign, UnifiedOp};
use regex::Regex;

#[derive(Debug, Clone)]
pub struct OfficialOutlineLevel {
    pub level: u8,
    pub example: &'static str,
    pub indent_pt: u32,
    pub indent_hu: i32,
}

#[derive(Debug, Clone, Copy)]
pub struct OfficialDocTypography {
    pub body_font: &'static str,
    pub heading_font: &'static str,
    pub body_size_pt: f64,
    pub line_spacing_percent: f64,
    pub char_ratio: f64,
    pub letter_spacing: f64,
    pub align: ParagraphAlign,
}

pub const OFFICIAL_DOC_TYPOGRAPHY: OfficialDocTypography = OfficialDocTypography {
    body_font: "함초롬바탕",
    heading_font: "함초롬바탕",
    body_size_pt: 11.0,
    line_spacing_percent: 160.0,
    char_ratio: 100.0,
    letter_spacing: 0.0,
    align: ParagraphAlign::Justify,
};

struct OutlineRule {
    meta: OfficialOutlineLevel,
    regex: LazyLock<Regex>,
}

static OUTLINE_RULES: [OutlineRule; 8] = [
    OutlineRule {
        meta: OfficialOutlineLevel {
            level: 1,
            example: "1. ",
            indent_pt: 0,
            indent_hu: 0,
        },
        regex: LazyLock::new(|| Regex::new(r"^\s*[0-9]+\.\s+").unwrap()),
    },
    OutlineRule {
        meta: OfficialOutlineLevel {
            level: 2,
            example: "가. ",
            indent_pt: 10,
            indent_hu: 1000,
        },
        regex: LazyLock::new(|| Regex::new(r"^\s*[가-하]\.\s+").unwrap()),
    },
    OutlineRule {
        meta: OfficialOutlineLevel {
            level: 3,
            example: "1) ",
            indent_pt: 20,
            indent_hu: 2000,
        },
        regex: LazyLock::new(|| Regex::new(r"^\s*[0-9]+\)\s+").unwrap()),
    },
    OutlineRule {
        meta: OfficialOutlineLevel {
            level: 4,
            example: "가) ",
            indent_pt: 30,
            indent_hu: 3000,
        },
        regex: LazyLock::new(|| Regex::new(r"^\s*[가-하]\)\s+").unwrap()),
    },
    OutlineRule {
        meta: OfficialOutlineLevel {
            level: 5,
            example: "(1) ",
            indent_pt: 40,
            indent_hu: 4000,
        },
        regex: LazyLock::new(|| Regex::new(r"^\s*\([0-9]+\)\s+").unwrap()),
    },
    OutlineRule {
        meta: OfficialOutlineLevel {
            level: 6,
            example: "(가) ",
            indent_pt: 50,
            indent_hu: 5000,
        },
        regex: LazyLock::new(|| Regex::new(r"^\s*\([가-하]\)\s+").unwrap()),
    },
    OutlineRule {
        meta: OfficialOutlineLevel {
            level: 7,
            example: "① ",
            indent_pt: 60,
            indent_hu: 6000,
        },
        regex: LazyLock::new(|| Regex::new(r"^\s*[①-⑮]\s+").unwrap()),
    },
    OutlineRule {
        meta: OfficialOutlineLevel {
            level: 8,
            example: "㉮ ",
            indent_pt: 70,
            indent_hu: 7000,
        },
        regex: LazyLock::new(|| Regex::new(r"^\s*[㉮-㉻]\s+").unwrap()),
    },
];

/// Returns the 8 Korean official document outline hierarchy levels (`1. -> 가. -> 1) -> 가) -> (1) -> (가) -> ① -> ㉮`).
pub fn official_outline_levels() -> Vec<OfficialOutlineLevel> {
    OUTLINE_RULES.iter().map(|r| r.meta.clone()).collect()
}

/// Generates [`UnifiedOp`] pairs (`apply_paragraph_style` + `apply_text_style`) to standardize
/// paragraphs into Korean official document typography (`함초롬바탕`, 11pt, 160% line spacing, JUSTIFY, outline indent).
pub fn build_official_doc_standard_ops(paragraphs: &[String]) -> Vec<UnifiedOp> {
    let mut ops = Vec::new();

    for (idx, text) in paragraphs.iter().enumerate() {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            continue;
        }

        let matched = OUTLINE_RULES.iter().find(|rule| rule.regex.is_match(trimmed));
        let indent_hu = matched.map_or(0, |r| r.meta.indent_hu) as f64;
        // Slice by UTF-16 code units (20 chars) matching JS `trimmed.slice(0, Math.min(20, trimmed.length))`.
        let target_snippet: String = trimmed.chars().take(20).collect();

        ops.push(UnifiedOp::ApplyParagraphStyle {
            para_index: Some(ParaIndexTarget::Index(idx as isize)),
            target_text: Some(target_snippet.clone()),
            align: Some(OFFICIAL_DOC_TYPOGRAPHY.align),
            indent_hu: Some(indent_hu),
            line_spacing_percent: Some(OFFICIAL_DOC_TYPOGRAPHY.line_spacing_percent),
            spacing_before_hu: None,
            spacing_after_hu: None,
        });

        ops.push(UnifiedOp::ApplyTextStyle {
            target_text: target_snippet,
            font_family: Some(OFFICIAL_DOC_TYPOGRAPHY.body_font.to_string()),
            size_pt: Some(OFFICIAL_DOC_TYPOGRAPHY.body_size_pt),
            bold: None,
            italic: None,
            underline: None,
            color: None,
            highlight: None,
            letter_spacing: None,
            char_ratio: None,
        });
    }

    ops
}
