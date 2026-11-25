use chrono::Utc;
use regex::Regex;
use std::collections::BTreeMap;
use std::sync::OnceLock;
use uuid::Uuid;

use crate::types::{MemoryDoc, MemoryMeta, SourceDocRef, FIXED_MEMORY_SECTIONS};

pub const EMPTY_SECTION_PLACEHOLDER: &str = "- (특별한 규칙 없음)";
pub const DEFAULT_SECTION_PLACEHOLDER: &str = "- (내용 없음)";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedMemoryMarkdown {
    pub doc: MemoryDoc,
    pub had_frontmatter: bool,
}

pub fn create_default_memory_body() -> String {
    FIXED_MEMORY_SECTIONS
        .iter()
        .map(|title| format!("## {title}\n{DEFAULT_SECTION_PLACEHOLDER}"))
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn match_section_title(raw_title: &str) -> Option<&'static str> {
    static NUM_PREFIX_RE: OnceLock<Regex> = OnceLock::new();
    let re = NUM_PREFIX_RE.get_or_init(|| Regex::new(r"^\d+[.)]\s*").unwrap());
    let cleaned = re.replace(raw_title.trim(), "").trim().to_string();
    let normalized: String = cleaned.chars().filter(|c| !c.is_whitespace()).collect();

    for fixed in FIXED_MEMORY_SECTIONS {
        let fixed_norm: String = fixed.chars().filter(|c| !c.is_whitespace()).collect();
        if cleaned == fixed || normalized == fixed_norm {
            return Some(fixed);
        }
    }

    if normalized.contains("문서유형") || normalized.contains("양식") {
        return Some("문서 유형·양식");
    }
    if normalized.contains("어투") || normalized.contains("문체") {
        return Some("어투·문체");
    }
    if normalized.contains("용어") || normalized.contains("표기") {
        return Some("용어·표기");
    }
    if normalized.contains("서식") || normalized.contains("표규칙") {
        return Some("서식·표 규칙");
    }
    if normalized.contains("구조") || normalized.contains("제목체계") {
        return Some("구조(제목 체계)");
    }
    if normalized.contains("금지") || normalized.contains("지양") {
        return Some("금지·지양");
    }
    if normalized.contains("예시문구") || normalized.contains("예시") {
        return Some("예시 문구(일반화됨)");
    }

    None
}

pub fn parse_memory_sections(body: &str) -> BTreeMap<String, String> {
    static FENCE_RE: OnceLock<Regex> = OnceLock::new();
    static FM_STRIP_RE: OnceLock<Regex> = OnceLock::new();
    static H1_RE: OnceLock<Regex> = OnceLock::new();
    static H2_RE: OnceLock<Regex> = OnceLock::new();

    let fence_re = FENCE_RE
        .get_or_init(|| Regex::new(r"(?s)^```(?:markdown)?\s*\r?\n(.*?)\r?\n```\s*$").unwrap());
    let fm_strip_re =
        FM_STRIP_RE.get_or_init(|| Regex::new(r"(?s)^---\r?\n.*?\r?\n---\r?\n?").unwrap());
    let h1_re = H1_RE.get_or_init(|| Regex::new(r"^#\s+").unwrap());
    let h2_re = H2_RE.get_or_init(|| Regex::new(r"^##\s+(.+?)\s*$").unwrap());

    let mut cleaned = body.trim().to_string();
    if let Some(caps) = fence_re.captures(&cleaned) {
        cleaned = caps[1].to_string();
    }
    cleaned = fm_strip_re.replace(&cleaned, "").to_string();

    let mut sections: BTreeMap<&'static str, Vec<String>> = BTreeMap::new();
    for title in FIXED_MEMORY_SECTIONS {
        sections.insert(title, Vec::new());
    }

    let mut current_title: Option<&'static str> = None;
    let mut unmapped_lines: Vec<String> = Vec::new();

    for line in cleaned.split('\n').map(|l| l.trim_end_matches('\r')) {
        if h1_re.is_match(line) {
            continue;
        }
        if let Some(caps) = h2_re.captures(line) {
            if let Some(parsed_title) = match_section_title(&caps[1]) {
                current_title = Some(parsed_title);
            } else {
                current_title = None;
                unmapped_lines.push(line.to_string());
            }
        } else if let Some(title) = current_title {
            sections.get_mut(title).unwrap().push(line.to_string());
        } else if !line.trim().is_empty() {
            unmapped_lines.push(line.to_string());
        }
    }

    if !unmapped_lines.is_empty() {
        let extra = unmapped_lines.join("\n").trim().to_string();
        if !extra.is_empty() {
            sections.get_mut("서식·표 규칙").unwrap().push(extra);
        }
    }

    let mut result = BTreeMap::new();
    for title in FIXED_MEMORY_SECTIONS {
        let content = sections[title].join("\n").trim().to_string();
        result.insert(
            title.to_string(),
            if content.is_empty() {
                EMPTY_SECTION_PLACEHOLDER.to_string()
            } else {
                content
            },
        );
    }
    result
}

pub fn serialize_memory_sections(sections: &BTreeMap<String, String>) -> String {
    FIXED_MEMORY_SECTIONS
        .iter()
        .map(|title| {
            let content = sections
                .get(*title)
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .unwrap_or(EMPTY_SECTION_PLACEHOLDER);
            format!("## {title}\n{content}")
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

pub fn normalize_memory_body(raw_body: &str) -> String {
    serialize_memory_sections(&parse_memory_sections(raw_body))
}

pub fn serialize_memory_markdown(doc: &MemoryDoc) -> String {
    let norm_body = normalize_memory_body(&doc.body);
    let id_json = serde_json::to_string(&doc.meta.id).unwrap_or_else(|_| "\"\"".to_string());
    let name_json = serde_json::to_string(&doc.meta.name).unwrap_or_else(|_| "\"\"".to_string());
    let desc_json =
        serde_json::to_string(&doc.meta.description).unwrap_or_else(|_| "\"\"".to_string());
    let created_json =
        serde_json::to_string(&doc.meta.created_at).unwrap_or_else(|_| "\"\"".to_string());
    let updated_json =
        serde_json::to_string(&doc.meta.updated_at).unwrap_or_else(|_| "\"\"".to_string());
    let source_docs_json =
        serde_json::to_string(&doc.meta.source_docs).unwrap_or_else(|_| "[]".to_string());

    format!(
        "---\nschemaVersion: 1\nid: {id_json}\nname: {name_json}\ndescription: {desc_json}\nrevision: {}\ncreatedAt: {created_json}\nupdatedAt: {updated_json}\nsourceDocs: {source_docs_json}\nmasked: true\n---\n\n{norm_body}",
        doc.meta.revision
    )
}

pub fn parse_memory_markdown(raw_md: &str) -> ParsedMemoryMarkdown {
    static FM_RE: OnceLock<Regex> = OnceLock::new();
    static H1_CAPTURE_RE: OnceLock<Regex> = OnceLock::new();

    let fm_re = FM_RE.get_or_init(|| {
        Regex::new(r"(?s)^---\r?\n(.*?)\r?\n---\r?\n?(.*)$").unwrap()
    });
    let h1_re = H1_CAPTURE_RE.get_or_init(|| Regex::new(r"^#\s+(.+?)\s*$").unwrap());

    let trimmed = raw_md.trim();
    if let Some(caps) = fm_re.captures(trimmed) {
        let fm_block = &caps[1];
        let body_part = &caps[2];

        let mut parsed_data: BTreeMap<String, serde_json::Value> = BTreeMap::new();
        for line in fm_block.split('\n').map(|l| l.trim_end_matches('\r')) {
            let Some(colon_idx) = line.find(':') else {
                continue;
            };
            let key = line[..colon_idx].trim().to_string();
            let val_str = line[colon_idx + 1..].trim();

            let val = if val_str.starts_with('"')
                || val_str.starts_with('[')
                || val_str.starts_with('{')
            {
                serde_json::from_str::<serde_json::Value>(val_str).unwrap_or_else(|_| {
                    let stripped = val_str
                        .trim_start_matches(['"', '\''])
                        .trim_end_matches(['"', '\'']);
                    serde_json::Value::String(stripped.to_string())
                })
            } else if val_str == "true" {
                serde_json::Value::Bool(true)
            } else if val_str == "false" {
                serde_json::Value::Bool(false)
            } else if !val_str.is_empty() && val_str.chars().all(|c| c.is_ascii_digit()) {
                val_str
                    .parse::<u64>()
                    .map(|n| serde_json::Value::Number(n.into()))
                    .unwrap_or_else(|_| serde_json::Value::String(val_str.to_string()))
            } else {
                serde_json::Value::String(val_str.to_string())
            };

            parsed_data.insert(key, val);
        }

        let now = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let id = parsed_data
            .get("id")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(ToString::to_string)
            .unwrap_or_else(|| format!("mem_{}", Uuid::new_v4()));
        let name = parsed_data
            .get("name")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or("가져온 작성 선호사항")
            .to_string();
        let description = parsed_data
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let revision = parsed_data
            .get("revision")
            .and_then(|v| v.as_u64())
            .unwrap_or(1);
        let created_at = parsed_data
            .get("createdAt")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(ToString::to_string)
            .unwrap_or_else(|| now.clone());
        let updated_at = parsed_data
            .get("updatedAt")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(ToString::to_string)
            .unwrap_or(now);
        let source_docs = parsed_data
            .get("sourceDocs")
            .and_then(|v| serde_json::from_value::<Vec<SourceDocRef>>(v.clone()).ok())
            .unwrap_or_default();

        return ParsedMemoryMarkdown {
            doc: MemoryDoc {
                meta: MemoryMeta {
                    schema_version: 1,
                    id,
                    name,
                    description,
                    revision,
                    created_at,
                    updated_at,
                    source_docs,
                    masked: true,
                },
                body: normalize_memory_body(body_part),
            },
            had_frontmatter: true,
        };
    }

    // No frontmatter
    let mut detected_name = "가져온 작성 선호사항".to_string();
    for line in trimmed.split('\n').map(|l| l.trim_end_matches('\r')) {
        if let Some(caps) = h1_re.captures(line) {
            detected_name = caps[1].trim().to_string();
            break;
        }
    }

    let now = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    ParsedMemoryMarkdown {
        doc: MemoryDoc {
            meta: MemoryMeta {
                schema_version: 1,
                id: format!("mem_{}", Uuid::new_v4()),
                name: detected_name,
                description: String::new(),
                revision: 1,
                created_at: now.clone(),
                updated_at: now,
                source_docs: Vec::new(),
                masked: true,
            },
            body: normalize_memory_body(trimmed),
        },
        had_frontmatter: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_default_memory_body_has_7_fixed_sections() {
        let body = create_default_memory_body();
        for title in FIXED_MEMORY_SECTIONS {
            assert!(body.contains(&format!("## {title}")));
        }
    }

    #[test]
    fn round_trip_serialization_and_parsing_lossless() {
        let doc = MemoryDoc {
            meta: MemoryMeta {
                schema_version: 1,
                id: "mem_test_123".to_string(),
                name: "서울시 \"공문서\": 표준 [스타일]".to_string(),
                description: "보고서: 결재 양식, '기안용'".to_string(),
                revision: 3,
                created_at: "2026-11-05T09:00:00.000Z".to_string(),
                updated_at: "2026-11-05T10:00:00.000Z".to_string(),
                source_docs: vec![SourceDocRef {
                    name: "공문서 \"예시\".hwpx".to_string(),
                    sha256_8: "1a2b3c4d".to_string(),
                }],
                masked: true,
            },
            body: "## 문서 유형·양식\n- 기안문 및 보고서\n\n## 어투·문체\n- 명확한 개조식\n\n## 용어·표기\n- 행정용어 준수\n\n## 서식·표 규칙\n- 표 상단 중앙 정렬\n\n## 구조(제목 체계)\n- 1. -> 가. -> 1)\n\n## 금지·지양\n- 모호한 피동형 지양\n\n## 예시 문구(일반화됨)\n- 추진 계획을 다음과 같이 보고함".to_string(),
        };

        let serialized = serialize_memory_markdown(&doc);
        let parsed = parse_memory_markdown(&serialized);

        assert!(parsed.had_frontmatter);
        assert_eq!(parsed.doc.meta, doc.meta);
        assert_eq!(parsed.doc.body, normalize_memory_body(&doc.body));
    }

    #[test]
    fn parses_raw_markdown_without_frontmatter_extracts_h1_and_fills_sections() {
        let raw_md = "# 업무 매뉴얼 스타일 가이드\n\n## 어투·문체\n- 친절하고 명확한 문체 사용\n\n## 금지·지양\n- 비속어 및 줄임말 사용 금지\n";
        let parsed = parse_memory_markdown(raw_md);
        assert!(!parsed.had_frontmatter);
        assert_eq!(parsed.doc.meta.name, "업무 매뉴얼 스타일 가이드");
        assert_eq!(parsed.doc.meta.revision, 1);
        assert!(parsed.doc.meta.source_docs.is_empty());
        assert!(parsed.doc.meta.masked);

        let sections = parse_memory_sections(&parsed.doc.body);
        assert_eq!(sections["어투·문체"], "- 친절하고 명확한 문체 사용");
        assert_eq!(sections["금지·지양"], "- 비속어 및 줄임말 사용 금지");
        assert_eq!(sections["문서 유형·양식"], EMPTY_SECTION_PLACEHOLDER);
        assert_eq!(sections["서식·표 규칙"], EMPTY_SECTION_PLACEHOLDER);
    }

    #[test]
    fn removes_markdown_code_fences() {
        let wrapped = "```markdown\n## 문서 유형·양식\n- 회의록\n\n## 어투·문체\n- 간결체\n```";
        let sections = parse_memory_sections(wrapped);
        assert_eq!(sections["문서 유형·양식"], "- 회의록");
        assert_eq!(sections["어투·문체"], "- 간결체");
    }

    #[test]
    fn merges_unrecognized_sections_into_format_section() {
        let raw = "## 어투·문체\n- 간결체\n\n## 기타 참고사항\n- 특별 부록 표 서식 참고할 것\n";
        let sections = parse_memory_sections(raw);
        assert_eq!(sections["어투·문체"], "- 간결체");
        assert!(sections["서식·표 규칙"].contains("기타 참고사항"));
        assert!(sections["서식·표 규칙"].contains("- 특별 부록 표 서식 참고할 것"));
    }
}
