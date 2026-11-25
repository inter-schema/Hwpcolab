use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Notify;

pub use hopclaw_types::{
    AgentLoopMode, AgentStepEvent, AgentStepType, BoxFuture, DiffItem, DiffMetricsDelta,
    DiffScope, DiffStatus, DiffTargetCoord, DiffVerificationStatus, DocFormat, ExcludedRegion,
    LayoutComparison, LayoutIssue, LayoutReport, OrchestrateAiEditResult, ProviderId,
    ReasoningEffort, SelectionTarget, TableCoordinateMeta, TrialStore, TrialWorkspace, UnifiedOp,
};

pub type LocalBoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Lightweight, cloneable cancellation token backed by `AtomicBool` + `Notify`.
#[derive(Debug, Clone)]
pub struct CancelToken {
    cancelled: Arc<AtomicBool>,
    notify: Arc<Notify>,
}

impl Default for CancelToken {
    fn default() -> Self {
        Self::new()
    }
}

impl CancelToken {
    pub fn new() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
            notify: Arc::new(Notify::new()),
        }
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        self.notify.notify_waiters();
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    pub async fn cancelled(&self) {
        if self.is_cancelled() {
            return;
        }
        let notified = self.notify.notified();
        if self.is_cancelled() {
            return;
        }
        notified.await;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningEffortOption {
    None,
    Low,
    Medium,
    High,
    Xhigh,
    Max,
}

impl ReasoningEffortOption {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Xhigh => "xhigh",
            Self::Max => "max",
        }
    }

    pub fn from_effort(effort: Option<ReasoningEffort>) -> Option<Self> {
        effort.map(|e| match e {
            ReasoningEffort::None => Self::None,
            ReasoningEffort::Low => Self::Low,
            ReasoningEffort::Medium => Self::Medium,
            ReasoningEffort::High => Self::High,
            ReasoningEffort::Xhigh => Self::Xhigh,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NeutralToolCall {
    pub id: String,
    pub name: String,
    pub arguments_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "lowercase")]
pub enum NeutralMessage {
    System {
        content: String,
    },
    User {
        content: String,
    },
    Assistant {
        content: Option<String>,
        #[serde(rename = "toolCalls", skip_serializing_if = "Option::is_none")]
        tool_calls: Option<Vec<NeutralToolCall>>,
    },
    Tool {
        #[serde(rename = "toolCallId")]
        tool_call_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        content: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NeutralToolSpec {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ToolChoice {
    Named { name: String },
    Mode(ToolChoiceMode),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolChoiceMode {
    Auto,
    None,
}

impl ToolChoice {
    pub fn auto() -> Self {
        Self::Mode(ToolChoiceMode::Auto)
    }

    pub fn none() -> Self {
        Self::Mode(ToolChoiceMode::None)
    }

    pub fn named(name: impl Into<String>) -> Self {
        Self::Named { name: name.into() }
    }
}

#[derive(Debug, Clone)]
pub struct AgentTurnRequest {
    pub system_prompt: String,
    pub messages: Vec<NeutralMessage>,
    pub tools: Vec<NeutralToolSpec>,
    pub tool_choice: ToolChoice,
    pub reasoning_effort: Option<ReasoningEffortOption>,
    pub request_id: Option<String>,
    pub timeout_ms: Option<u64>,
    pub cancel_token: Option<CancelToken>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTurnResponse {
    pub content: String,
    pub tool_calls: Vec<NeutralToolCall>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConfig {
    pub provider: ProviderId,
    pub api_key: String,
    pub model: String,
    pub endpoint: Option<String>,
    pub custom_name: Option<String>,
    pub custom_supports_tools: Option<bool>,
    pub custom_send_reasoning: Option<bool>,
}
