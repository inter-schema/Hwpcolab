use chrono::Utc;
use std::collections::{BTreeMap, HashMap, HashSet};

use crate::diff::compute_section_diffs;
use crate::error::MemoryError;
use crate::frontmatter::{
    normalize_memory_body, parse_memory_sections, serialize_memory_sections,
};
use crate::generate::{
    compute_sha256_8, GenerateMemoryInputDoc, MAX_CHARS_PER_DOC, MAX_INPUT_DOCS,
    MAX_TOTAL_INPUT_CHARS,
};
use crate::types::{
    MemoryDoc, MemoryLlmClient, MemoryMeta, PiiRedactor, SectionDiff, SourceDocRef,
    FIXED_MEMORY_SECTIONS,
};

pub const REFINE_SYSTEM_PROMPT: &str = "당신은 한국어 공문서·보고서·업무 양식의 작성 스타일 가이드(메모리)를 정밀 개선하는 전문가입니다.
기존 메모리 내용을 바탕으로 사용자의 다듬기 지시사항 및 추가 참고 문서를 분석하여, 개선된 전체 마크다운 스타일 가이드를 작성하십시오.

[절대 준수 규칙]
1. 반드시 아래 7개 H2(##) 섹션 제목을 정확한 이름과 순서로 모두 출력하십시오(YAML frontmatter나 코드펜스는 출력하지 마십시오):
## 문서 유형·양식
## 어투·문체
## 용어·표기
## 서식·표 규칙
## 구조(제목 체계)
## 금지·지양
## 예시 문구(일반화됨)
2. 개인정보(실명, 주민번호, 전화번호, 이메일, 계좌번호, 주소 등)는 절대 인용하지 말고 일반화된 플레이스홀더(<이름>, <일자>, <금액> 등)만 사용하십시오.
3. 수정이 필요 없는 섹션도 기존 내용을 유지하여 7개 섹션 전체를 온전히 출력하십시오.";

#[derive(Debug, Clone)]
pub struct ProposeRefinementArgs {
    pub doc: MemoryDoc,
    pub instruction: Option<String>,
    pub extra_docs: Vec<GenerateMemoryInputDoc>,
}

#[derive(Debug, Clone)]
pub struct ProposeRefinementResult {
    pub proposed_body: String,
    pub section_diffs: Vec<SectionDiff>,
    pub warnings: Vec<String>,
    pub extra_source_docs: Vec<SourceDocRef>,
}

pub async fn propose_refinement(
    args: ProposeRefinementArgs,
    llm: &dyn MemoryLlmClient,
    redactor: &dyn PiiRedactor,
) -> Result<ProposeRefinementResult, MemoryError> {
    let has_instruction = args
        .instruction
        .as_deref()
        .is_some_and(|s| !s.trim().is_empty());
    let valid_extra_docs: Vec<&GenerateMemoryInputDoc> = args
        .extra_docs
        .iter()
        .filter(|d| !d.text.trim().is_empty())
        .collect();

    if !has_instruction && valid_extra_docs.is_empty() {
        return Err(MemoryError::Validation(
            "다듬기 지시사항을 입력하거나 추가 학습할 문서를 선택해 주세요.".to_string(),
        ));
    }

    let mut warnings = Vec::new();
    let mut sanitized_instruction = String::new();
    if let Some(instr) = args.instruction.as_deref().filter(|s| !s.trim().is_empty()) {
        let scan = redactor.scan_and_redact(instr);
        sanitized_instruction = scan.text;
        if scan.total_count > 0 {
            warnings.push(format!(
                "지시사항에서 개인정보 패턴 {}건을 사전 비식별화했습니다.",
                scan.total_count
            ));
        }
    }

    let docs_to_use = if valid_extra_docs.len() > MAX_INPUT_DOCS {
        warnings.push("추가 학습 문서는 최대 10개까지만 반영되었습니다.".to_string());
        &valid_extra_docs[..MAX_INPUT_DOCS]
    } else {
        &valid_extra_docs[..]
    };

    let mut extra_source_docs = Vec::new();
    let mut processed_docs: Vec<(String, String)> = Vec::new();
    let mut input_redaction_count = 0usize;
    let mut accumulated_chars = 0usize;

    for doc in docs_to_use {
        let sha256_8 = compute_sha256_8(&doc.text);
        extra_source_docs.push(SourceDocRef {
            name: doc.name.clone(),
            sha256_8,
        });

        let sliced: String = if doc.text.chars().count() > MAX_CHARS_PER_DOC {
            warnings.push(format!(
                "추가 문서 \"{}\"가 30,000자를 초과하여 앞부분만 반영되었습니다.",
                doc.name
            ));
            doc.text.chars().take(MAX_CHARS_PER_DOC).collect()
        } else {
            doc.text.clone()
        };

        let masked = redactor.mask_memory_source(&sliced);
        input_redaction_count += masked.total_count;

        let masked_len = masked.text.chars().count();
        if accumulated_chars + masked_len > MAX_TOTAL_INPUT_CHARS {
            let allowed = MAX_TOTAL_INPUT_CHARS.saturating_sub(accumulated_chars);
            if allowed > 0 {
                processed_docs.push((
                    doc.name.clone(),
                    masked.text.chars().take(allowed).collect(),
                ));
            }
            warnings.push(
                "추가 입력 텍스트가 80,000자 상한에 도달하여 일부 내용이 절단되었습니다."
                    .to_string(),
            );
            break;
        } else {
            accumulated_chars += masked_len;
            processed_docs.push((doc.name.clone(), masked.text));
        }
    }

    if input_redaction_count > 0 {
        warnings.push(format!(
            "추가 학습 문서에서 개인정보 패턴 {input_redaction_count}건을 사전 비식별화했습니다."
        ));
    }

    let mut prompt_parts = vec![format!(
        "### [현재 작성 선호사항 가이드]\n{}",
        args.doc.body
    )];

    if !sanitized_instruction.is_empty() {
        prompt_parts.push(format!(
            "### [사용자 다듬기 지시사항]\n{sanitized_instruction}"
        ));
    }

    if !processed_docs.is_empty() {
        let extra_docs_text = processed_docs
            .iter()
            .enumerate()
            .map(|(i, (name, text))| format!("#### [추가 문서 {}: {name}]\n{text}", i + 1))
            .collect::<Vec<_>>()
            .join("\n\n");
        prompt_parts.push(format!("### [추가 학습 예시 문서]\n{extra_docs_text}"));
    }

    prompt_parts.push(
        "위 내용을 종합하여 현재 가이드의 7개 섹션을 개선한 완성본 마크다운을 작성해 주십시오."
            .to_string(),
    );

    let user_prompt = prompt_parts.join("\n\n");
    let content = llm.complete_text(REFINE_SYSTEM_PROMPT, &user_prompt).await?;
    if content.trim().is_empty() {
        return Err(MemoryError::Llm(
            "AI 모델로부터 다듬기 제안 응답을 받지 못했습니다.".to_string(),
        ));
    }

    let normalized = normalize_memory_body(&content);
    let output_scan = redactor.scan_and_redact(&normalized);
    if output_scan.total_count > 0 {
        warnings.push(format!(
            "수정안 결과에 남은 개인정보 패턴 {}건({})을 플레이스홀더로 자동 치환했습니다.",
            output_scan.total_count,
            output_scan.hits.join(", ")
        ));
    }

    let proposed_body = output_scan.text;
    let section_diffs = compute_section_diffs(&args.doc.body, &proposed_body);

    Ok(ProposeRefinementResult {
        proposed_body,
        section_diffs,
        warnings,
        extra_source_docs,
    })
}

pub fn apply_section_decisions(
    doc: &MemoryDoc,
    diffs: &[SectionDiff],
    accepted_section_titles: &[String],
    extra_source_docs: &[SourceDocRef],
    redactor: &dyn PiiRedactor,
) -> MemoryDoc {
    let accepted_set: HashSet<&str> = accepted_section_titles.iter().map(|s| s.as_str()).collect();
    let diff_map: HashMap<&str, &SectionDiff> = diffs
        .iter()
        .map(|d| (d.section_title.as_str(), d))
        .collect();
    let existing_sections = parse_memory_sections(&doc.body);

    let mut selected_map = BTreeMap::new();
    for title in FIXED_MEMORY_SECTIONS {
        let chosen = if let Some(d) = diff_map.get(title) {
            if accepted_set.contains(title) {
                d.after_content.clone()
            } else {
                d.before_content.clone()
            }
        } else {
            existing_sections[title].clone()
        };
        selected_map.insert(title.to_string(), chosen);
    }

    let serialized = serialize_memory_sections(&selected_map);
    let merged_body = redactor.scan_and_redact(&serialized).text;

    let mut merged_sources = Vec::new();
    let mut seen_keys = HashSet::new();
    for d in doc.meta.source_docs.iter().chain(extra_source_docs.iter()) {
        let key = format!("{}::{}", d.name, d.sha256_8);
        if seen_keys.insert(key) {
            merged_sources.push(d.clone());
        }
    }

    let now = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    MemoryDoc {
        meta: MemoryMeta {
            source_docs: merged_sources,
            updated_at: now,
            masked: true,
            ..doc.meta.clone()
        },
        body: merged_body,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{BoxFuture, NoopRedactor, RedactOutcome};
    use std::sync::Mutex;

    fn base_doc() -> MemoryDoc {
        MemoryDoc {
            meta: MemoryMeta {
                schema_version: 1,
                id: "mem_refine_1".into(),
                name: "공문서 스타일".into(),
                description: "기본 설명".into(),
                revision: 2,
                created_at: "2026-11-05T00:00:00.000Z".into(),
                updated_at: "2026-11-05T01:00:00.000Z".into(),
                source_docs: vec![SourceDocRef {
                    name: "원문1.hwpx".into(),
                    sha256_8: "11223344".into(),
                }],
                masked: true,
            },
            body: "## 문서 유형·양식\n- 일반 기안문\n\n## 어투·문체\n- 표준 공문서체\n\n## 용어·표기\n- 행정 용어\n\n## 서식·표 규칙\n- 표 기본 여백\n\n## 구조(제목 체계)\n- 1. -> 가.\n\n## 금지·지양\n- 모호한 표현 금지\n\n## 예시 문구(일반화됨)\n- 붙임 문서를 참조 바람".into(),
        }
    }

    #[test]
    fn compute_section_diffs_detects_changes() {
        let base = base_doc();
        let after = base
            .body
            .replace("## 어투·문체\n- 표준 공문서체", "## 어투·문체\n- 간결한 개조식 공문서체");
        let diffs = compute_section_diffs(&base.body, &after);
        assert_eq!(diffs.len(), 7);

        let tone = diffs.iter().find(|d| d.section_title == "어투·문체").unwrap();
        assert!(tone.changed);
        assert!(tone.tokens.iter().any(|t| t.op == 1));

        let doc_type = diffs
            .iter()
            .find(|d| d.section_title == "문서 유형·양식")
            .unwrap();
        assert!(!doc_type.changed);
    }

    struct CaptureLlm {
        captured: Mutex<String>,
        response: String,
    }

    impl MemoryLlmClient for CaptureLlm {
        fn complete_text<'a>(
            &'a self,
            _system_prompt: &'a str,
            user_prompt: &'a str,
        ) -> BoxFuture<'a, Result<String, MemoryError>> {
            Box::pin(async move {
                *self.captured.lock().unwrap() = user_prompt.to_string();
                Ok(self.response.clone())
            })
        }
    }

    struct TestRedactor;
    impl PiiRedactor for TestRedactor {
        fn scan_and_redact(&self, input: &str) -> RedactOutcome {
            let mut text = input.to_string();
            let mut count = 0usize;
            for (needle, repl) in [
                ("김유신", "<이름>"),
                ("홍길동", "<이름>"),
                ("이순신", "<이름>"),
                ("010-1111-2222", "<전화번호>"),
                ("010-5555-6666", "<전화번호>"),
                ("110-123-456789", "<계좌번호>"),
            ] {
                if text.contains(needle) {
                    text = text.replace(needle, repl);
                    count += 1;
                }
            }
            RedactOutcome {
                text,
                hits: if count > 0 { vec!["PII".into()] } else { vec![] },
                total_count: count,
            }
        }
    }

    #[tokio::test]
    async fn propose_refinement_redacts_input_and_output() {
        let base = base_doc();
        let llm = CaptureLlm {
            captured: Mutex::new(String::new()),
            response: "## 문서 유형·양식\n- 일반 기안문 및 보고서 (신청인: 이순신)\n\n## 어투·문체\n- 극도로 간결한 개조식\n\n## 예시 문구(일반화됨)\n- 연락처: 010-5555-6666".into(),
        };

        let res = propose_refinement(
            ProposeRefinementArgs {
                doc: base,
                instruction: Some("기안자: 김유신 010-1111-2222의 요청에 맞춰 다듬어줘".into()),
                extra_docs: vec![GenerateMemoryInputDoc {
                    name: "참고문서.txt".into(),
                    text: "참고인: 홍길동 계좌 110-123-456789 적용 요망".into(),
                }],
            },
            &llm,
            &TestRedactor,
        )
        .await
        .unwrap();

        let captured = llm.captured.lock().unwrap();
        assert!(!captured.contains("김유신"));
        assert!(!captured.contains("010-1111-2222"));
        assert!(!captured.contains("홍길동"));
        assert!(!captured.contains("110-123-456789"));
        assert!(captured.contains("<이름>"));
        assert!(captured.contains("<전화번호>"));
        assert!(captured.contains("<계좌번호>"));

        assert!(!res.proposed_body.contains("이순신"));
        assert!(!res.proposed_body.contains("010-5555-6666"));
        assert!(res.proposed_body.contains("<이름>"));
        assert!(res.proposed_body.contains("<전화번호>"));
        assert_eq!(res.section_diffs.len(), 7);
        assert_eq!(res.extra_source_docs.len(), 1);
    }

    #[test]
    fn apply_section_decisions_merges_partially() {
        let base = base_doc();
        let after = base
            .body
            .replace("## 문서 유형·양식\n- 일반 기안문", "## 문서 유형·양식\n- 개정된 기안문 양식")
            .replace("## 어투·문체\n- 표준 공문서체", "## 어투·문체\n- 개조식 어투 적용");
        let diffs = compute_section_diffs(&base.body, &after);

        let updated = apply_section_decisions(
            &base,
            &diffs,
            &["어투·문체".to_string()],
            &[SourceDocRef {
                name: "학습2.hwpx".into(),
                sha256_8: "55667788".into(),
            }],
            &NoopRedactor,
        );

        assert!(updated.body.contains("- 개조식 어투 적용"));
        assert!(updated.body.contains("- 일반 기안문"));
        assert!(!updated.body.contains("- 개정된 기안문 양식"));
        assert_eq!(updated.meta.revision, base.meta.revision);
        assert_eq!(updated.meta.source_docs.len(), 2);
    }
}
