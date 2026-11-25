use std::future::Future;
use std::pin::Pin;

pub use hopclaw_types::{
    MemoryDoc, MemoryMeta, SectionDiff, SectionDiffOp, SectionDiffToken, SourceDocRef,
    FIXED_MEMORY_SECTIONS,
};

use crate::error::MemoryError;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

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
