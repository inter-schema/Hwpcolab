use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The 7 fixed Markdown section headings for Writing Memory (`src/memory/types.ts`).
pub const FIXED_MEMORY_SECTIONS: [&str; 7] = [
    "문서 유형·양식",
    "어투·문체",
    "용어·표기",
    "서식·표 규칙",
    "구조(제목 체계)",
    "금지·지양",
    "예시 문구(일반화됨)",
];

/// Reference to a source document used to generate/refine Writing Memory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceDocRef {
    pub name: String,
    #[serde(rename = "sha256_8")]
    pub sha256_8: String,
}

/// YAML frontmatter metadata matching `MemoryMeta` in `src/memory/types.ts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryMeta {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub description: String,
    pub revision: u64,
    pub created_at: String,
    pub updated_at: String,
    pub source_docs: Vec<SourceDocRef>,
    pub masked: bool,
}

/// Full Writing Memory document (`MemoryDoc` in `src/memory/types.ts`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryDoc {
    pub meta: MemoryMeta,
    pub body: String,
}

/// Token-level diff operation (`-1 | 0 | 1` in `SectionDiffToken`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionDiffOp {
    Delete = -1,
    Equal = 0,
    Insert = 1,
}

impl Serialize for SectionDiffOp {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_i8(*self as i8)
    }
}

impl<'de> Deserialize<'de> for SectionDiffOp {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = i8::deserialize(deserializer)?;
        match value {
            -1 => Ok(Self::Delete),
            0 => Ok(Self::Equal),
            1 => Ok(Self::Insert),
            _ => Err(serde::de::Error::custom(format!(
                "invalid SectionDiffOp {value}; expected -1, 0, or 1"
            ))),
        }
    }
}

/// Single diff token in a `SectionDiff` (`src/memory/types.ts`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SectionDiffToken {
    pub op: SectionDiffOp,
    pub text: String,
}

/// Per-section diff comparing existing vs proposed memory (`SectionDiff` in `src/memory/types.ts`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SectionDiff {
    pub section_title: String,
    pub before_content: String,
    pub after_content: String,
    pub changed: bool,
    pub tokens: Vec<SectionDiffToken>,
}

/// PII category matching `PiiCategory` in `src/memory/types.ts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PiiCategory {
    MaskToken,
    Rrn,
    CorpReg,
    BizReg,
    Card,
    Phone,
    Email,
    Account,
    PassportDriver,
    Vehicle,
    Address,
    PersonName,
}

/// Single category hit count from PII redaction scan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PiiRedactionHit {
    pub category: PiiCategory,
    pub placeholder: String,
    pub count: usize,
}

/// Result of scanning and redacting PII (`ScanAndRedactResult` in `src/memory/types.ts`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanAndRedactResult {
    pub text: String,
    pub hits: Vec<String>,
    pub hit_details: Vec<PiiRedactionHit>,
    pub total_count: usize,
}
