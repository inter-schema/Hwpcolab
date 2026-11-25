use crate::doc::SelectionRect;
use crate::ops::DiffItem;
use serde::{Deserialize, Serialize};

/// Agent step event kind matching `AgentStepEvent.type` in `src/services/aiOrchestrator.ts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentStepType {
    Inspect,
    Trial,
    Verify,
    Guard,
    Thinking,
    Propose,
}

/// Real-time progress event streamed from `ai_edit_start` (`Channel<AgentStepEvent>`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentStepEvent {
    #[serde(rename = "type")]
    pub step_type: AgentStepType,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// Final result returned by `orchestrateAiEdit` / `ai_edit_start`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrchestrateAiEditResult {
    pub summary: String,
    pub diff_items: Vec<DiffItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steps: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doc_checks: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trials_run: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_passed: Option<bool>,
}

/// Target kind of an AI-excluded privacy region (`src/types/privacy.ts`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExcludedRegionTargetType {
    Cell,
    BodyText,
}

/// User-locked exclusion region matching `ExcludedRegion` in `src/types/privacy.ts`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExcludedRegion {
    pub id: String,
    pub label: String,
    pub target_type: ExcludedRegionTargetType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table_index: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub col: Option<usize>,
    pub raw_text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rects: Option<Vec<SelectionRect>>,
}

/// Reasoning effort setting matching `src/services/aiOrchestrator.ts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningEffort {
    None,
    Low,
    Medium,
    High,
    Xhigh,
}

/// LLM provider identifier matching `src/services/agent/types.ts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderId {
    Claude,
    Openai,
    Gemini,
    Ollama,
    Baze,
    Custom,
}

/// Agent loop execution mode matching `src/services/agent/types.ts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentLoopMode {
    Legacy,
    V2,
}
