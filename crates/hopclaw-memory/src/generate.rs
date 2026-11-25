use chrono::Utc;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::error::MemoryError;
use crate::frontmatter::normalize_memory_body;
use crate::types::{MemoryDoc, MemoryLlmClient, MemoryMeta, PiiRedactor, SourceDocRef};

pub const MAX_INPUT_DOCS: usize = 10;
pub const MAX_CHARS_PER_DOC: usize = 30_000;
pub const MAX_TOTAL_INPUT_CHARS: usize = 80_000;

pub const MEMORY_GENERATION_SYSTEM_PROMPT: &str = "당신은 한국어 공문서·보고서·업무 양식의 작성 스타일과 규칙을 추출하는 전문가입니다.
제공된 예시 문서들의 공통 문체, 어투, 용어 표기, 서식·표 구성 규칙, 제목 번호 체계, 지양 표현을 분석하여 재사용 가능한 마크다운 스타일 가이드(메모리)를 작성하십시오.

[절대 준수 규칙]
1. 반드시 아래 7개 H2(##) 섹션 제목을 정확한 이름과 순서로 모두 출력하십시오(YAML frontmatter나 코드펜스는 출력하지 마십시오):
## 문서 유형·양식
## 어투·문체
## 용어·표기
## 서식·표 규칙
## 구조(제목 체계)
## 금지·지양
## 예시 문구(일반화됨)
2. 개인정보(실명, 주민번호, 전화번호, 이메일, 계좌번호, 주소, 기관 고유 사건번호 등)는 절대 원문 그대로 인용하지 마십시오. 예시 문구에는 반드시 <이름>, <부서명>, <일자>, <금액>, <주소>, <전화번호> 같은 일반화된 플레이스홀더만 사용하십시오.
3. 문서의 구체적 사실관계(특정 사업명·수치)가 아니라 '반복 적용할 작성 양식·문체·서식 규칙'만 간결한 개조식(- 항목)으로 정리하십시오.";

pub fn compute_sha256_8(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    let hex_str = hex::encode(hasher.finalize());
    hex_str[..8].to_string()
}

#[derive(Debug, Clone)]
pub struct GenerateMemoryInputDoc {
    pub name: String,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct GenerateMemoryArgs {
    pub name: String,
    pub description: Option<String>,
    pub docs: Vec<GenerateMemoryInputDoc>,
}

#[derive(Debug, Clone)]
pub struct GenerateMemoryResult {
    pub doc: MemoryDoc,
    pub warnings: Vec<String>,
    pub input_redaction_count: usize,
    pub output_redaction_count: usize,
}

pub async fn generate_memory(
    args: GenerateMemoryArgs,
    llm: &dyn MemoryLlmClient,
    redactor: &dyn PiiRedactor,
) -> Result<GenerateMemoryResult, MemoryError> {
    let trimmed_name = args.name.trim();
    if trimmed_name.is_empty() {
        return Err(MemoryError::Validation(
            "작성 선호사항 이름을 입력해 주세요.".to_string(),
        ));
    }

    let valid_docs: Vec<&GenerateMemoryInputDoc> = args
        .docs
        .iter()
        .filter(|d| !d.text.trim().is_empty())
        .collect();
    if valid_docs.is_empty() {
        return Err(MemoryError::Validation(
            "분석할 문서 텍스트가 없습니다. 내용이 있는 문서를 선택해 주세요.".to_string(),
        ));
    }

    let mut warnings = Vec::new();
    let input_docs = if valid_docs.len() > MAX_INPUT_DOCS {
        warnings.push("최대 10개 문서까지만 반영되어 나머지 문서는 제외되었습니다.".to_string());
        &valid_docs[..MAX_INPUT_DOCS]
    } else {
        &valid_docs[..]
    };

    let mut source_docs = Vec::new();
    let mut processed_docs: Vec<(String, String)> = Vec::new();
    let mut input_redaction_count = 0usize;
    let mut accumulated_chars = 0usize;

    for doc in input_docs {
        let sha256_8 = compute_sha256_8(&doc.text);
        source_docs.push(SourceDocRef {
            name: doc.name.clone(),
            sha256_8,
        });

        let sliced: String = if doc.text.chars().count() > MAX_CHARS_PER_DOC {
            warnings.push(format!(
                "\"{}\" 문서가 30,000자를 초과하여 앞부분만 반영되었습니다.",
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
                "전체 입력 텍스트가 80,000자 상한에 도달하여 일부 내용이 절단되었습니다."
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
            "입력 문서에서 개인정보 패턴 {input_redaction_count}건을 사전 비식별화하여 전송했습니다."
        ));
    }

    let docs_text = processed_docs
        .iter()
        .enumerate()
        .map(|(i, (name, text))| format!("### [문서 {}: {name}]\n{text}", i + 1))
        .collect::<Vec<_>>()
        .join("\n\n");
    let user_prompt = format!(
        "아래 {}개 예시 문서들의 공통 작성 스타일, 어투, 용어 표기, 서식/표 규칙, 제목 체계, 지양 표현을 분석하여 마크다운 스타일 가이드(메모리)를 작성해 주십시오.\n\n{docs_text}",
        processed_docs.len()
    );

    let content = llm
        .complete_text(MEMORY_GENERATION_SYSTEM_PROMPT, &user_prompt)
        .await?;
    if content.trim().is_empty() {
        return Err(MemoryError::Llm(
            "AI 모델로부터 작성 선호사항 응답을 받지 못했습니다.".to_string(),
        ));
    }

    let normalized = normalize_memory_body(&content);
    let output_scan = redactor.scan_and_redact(&normalized);
    let output_redaction_count = output_scan.total_count;

    if output_redaction_count > 0 {
        warnings.push(format!(
            "생성 결과에 남은 개인정보 패턴 {output_redaction_count}건({})을 플레이스홀더로 자동 치환했습니다.",
            output_scan.hits.join(", ")
        ));
    }

    let now = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let doc = MemoryDoc {
        meta: MemoryMeta {
            schema_version: 1,
            id: format!("mem_{}", Uuid::new_v4()),
            name: trimmed_name.to_string(),
            description: args.description.unwrap_or_default().trim().to_string(),
            revision: 1,
            created_at: now.clone(),
            updated_at: now,
            source_docs,
            masked: true,
        },
        body: output_scan.text,
    };

    Ok(GenerateMemoryResult {
        doc,
        warnings,
        input_redaction_count,
        output_redaction_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{BoxFuture, NoopRedactor, RedactOutcome};

    struct StubLlm(String);

    impl MemoryLlmClient for StubLlm {
        fn complete_text<'a>(
            &'a self,
            _system_prompt: &'a str,
            _user_prompt: &'a str,
        ) -> BoxFuture<'a, Result<String, MemoryError>> {
            Box::pin(async move { Ok(self.0.clone()) })
        }
    }

    struct SimpleRedactor;
    impl PiiRedactor for SimpleRedactor {
        fn scan_and_redact(&self, input: &str) -> RedactOutcome {
            let mut text = input.to_string();
            let mut count = 0usize;
            for (needle, repl) in [("홍길동", "<이름>"), ("010-1234-5678", "<전화번호>")] {
                if text.contains(needle) {
                    text = text.replace(needle, repl);
                    count += 1;
                }
            }
            RedactOutcome {
                text,
                hits: if count > 0 { vec!["성명".into()] } else { vec![] },
                total_count: count,
            }
        }
    }

    #[tokio::test]
    async fn validates_name_and_docs() {
        let llm = StubLlm("".into());
        let err1 = generate_memory(
            GenerateMemoryArgs {
                name: "   ".into(),
                description: None,
                docs: vec![GenerateMemoryInputDoc {
                    name: "d1".into(),
                    text: "내용".into(),
                }],
            },
            &llm,
            &NoopRedactor,
        )
        .await
        .unwrap_err();
        assert!(err1.to_string().contains("이름을 입력해 주세요"));

        let err2 = generate_memory(
            GenerateMemoryArgs {
                name: "테스트".into(),
                description: None,
                docs: vec![],
            },
            &llm,
            &NoopRedactor,
        )
        .await
        .unwrap_err();
        assert!(err2.to_string().contains("분석할 문서 텍스트가 없습니다"));
    }

    #[tokio::test]
    async fn enforces_10_doc_cap_and_redacts() {
        let llm = StubLlm("## 문서 유형·양식\n- 기안문 (담당: 홍길동)\n## 어투·문체\n- 개조식".into());
        let many_docs: Vec<GenerateMemoryInputDoc> = (1..=12)
            .map(|i| GenerateMemoryInputDoc {
                name: format!("doc_{i}.txt"),
                text: format!("문서 {i} 담당자 홍길동"),
            })
            .collect();

        let res = generate_memory(
            GenerateMemoryArgs {
                name: "스타일".into(),
                description: None,
                docs: many_docs,
            },
            &llm,
            &SimpleRedactor,
        )
        .await
        .unwrap();

        assert_eq!(res.doc.meta.source_docs.len(), 10);
        assert!(res.warnings.iter().any(|w| w.contains("최대 10개 문서")));
        assert!(!res.doc.body.contains("홍길동"));
        assert!(res.doc.body.contains("<이름>"));
        assert!(res.input_redaction_count > 0);
        assert!(res.output_redaction_count > 0);
    }
}
