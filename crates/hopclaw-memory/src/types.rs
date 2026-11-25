use serde::{Deserialize, Serialize};
use std::future::Future;
use std::pin::Pin;

use crate::error::MemoryError;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub const FIXED_MEMORY_SECTIONS: [&str; 7] = [
    "문서 유형·양식",
    "어투·문체",
    "용어·표기",
    "서식·표 규칙",
    "구조(제목 체계)",
    "금지·지양",
    "예시 문구(일반화됨)",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceDocRef {
    pub name: String,
    pub sha256_8: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryMeta {
    pub schema_version: u8,
    pub id: String,
    pub name: String,
    pub description: String,
    pub revision: u32,
    pub created_at: String,
    pub updated_at: String,
    pub source_docs: Vec<SourceDocRef>,
    pub masked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryDoc {
    pub meta: MemoryMeta,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SectionDiffToken {
    pub op: i8,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SectionDiff {
    pub section_title: String,
    pub before_content: String,
    pub after_content: String,
    pub changed: bool,
    pub tokens: Vec<SectionDiffToken>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedactOutcome {
    pub text: String,
    pub hits: Vec<String>,
    pub total_count: usize,
}

/// Trait boundary to `hopclaw-privacy` so `hopclaw-memory` can redact PII
/// without taking a direct compile-time dependency during parallel development.
pub trait PiiRedactor: Send + Sync {
    fn scan_and_redact(&self, input: &str) -> RedactOutcome;

    fn mask_memory_source(&self, input: &str) -> RedactOutcome {
        self.scan_and_redact(input)
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct NoopRedactor;

impl PiiRedactor for NoopRedactor {
    fn scan_and_redact(&self, input: &str) -> RedactOutcome {
        RedactOutcome {
            text: input.to_string(),
            hits: Vec::new(),
            total_count: 0,
        }
    }
}

/// Trait boundary for executing single-shot memory generation/refinement turns.
pub trait MemoryLlmClient: Send + Sync {
    fn complete_text<'a>(
        &'a self,
        system_prompt: &'a str,
        user_prompt: &'a str,
    ) -> BoxFuture<'a, Result<String, MemoryError>>;
}
