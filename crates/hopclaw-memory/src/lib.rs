pub mod diff;
pub mod error;
pub mod frontmatter;
pub mod generate;
pub mod injection;
pub mod refine;
pub mod store;
pub mod types;

pub use diff::compute_section_diffs;
pub use error::MemoryError;
pub use frontmatter::{
    create_default_memory_body, normalize_memory_body, parse_memory_markdown,
    parse_memory_sections, serialize_memory_markdown, serialize_memory_sections,
    ParsedMemoryMarkdown,
};
pub use generate::{
    compute_sha256_8, generate_memory, GenerateMemoryArgs, GenerateMemoryInputDoc,
    GenerateMemoryResult, MAX_CHARS_PER_DOC, MAX_INPUT_DOCS, MAX_TOTAL_INPUT_CHARS,
    MEMORY_GENERATION_SYSTEM_PROMPT,
};
pub use injection::{
    build_memory_context, MemoryContextResult, MAX_SINGLE_MEMORY_CHARS, MAX_TOTAL_MEMORY_CHARS,
};
pub use refine::{
    apply_section_decisions, propose_refinement, ProposeRefinementArgs, ProposeRefinementResult,
    REFINE_SYSTEM_PROMPT,
};
pub use store::{
    FileMemoryStorage, InMemoryStorage, LegacyImportReport, MemoryStorageBackend, MemoryStore,
    MAX_REVISIONS, MEM_DOC_PREFIX, MEM_INDEX_KEY,
};
pub use types::{
    MemoryDoc, MemoryLlmClient, MemoryMeta, NoopRedactor, PiiRedactor, RedactOutcome, SectionDiff,
    SectionDiffToken, SourceDocRef, FIXED_MEMORY_SECTIONS,
};
