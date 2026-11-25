pub mod claude;
pub mod openai_compat;

pub use claude::ClaudeAdapter;
pub use openai_compat::OpenAiCompatAdapter;

use std::sync::Arc;

use crate::endpoint_policy::{validate_custom_endpoint, EndpointValidationResult};
use crate::error::AiError;
use crate::proxy::LlmTransport;
use crate::types::{AgentTurnRequest, AgentTurnResponse, BoxFuture, ProviderConfig, ProviderId};

pub trait ProviderAdapter: Send + Sync {
    fn id(&self) -> ProviderId;
    fn supports_tools(&self) -> bool;
    fn turn(&self, req: AgentTurnRequest) -> BoxFuture<'_, Result<AgentTurnResponse, AiError>>;
}

/// Constructs a `ProviderAdapter` for the configured provider, matching
/// `src/services/agent/adapters/index.ts`.
pub fn create_provider_adapter(
    mut cfg: ProviderConfig,
    transport: Arc<dyn LlmTransport>,
) -> Result<Box<dyn ProviderAdapter>, AiError> {
    match cfg.provider {
        ProviderId::Claude => Ok(Box::new(ClaudeAdapter::new(cfg, transport))),
        ProviderId::Openai => {
            if cfg.endpoint.as_deref().unwrap_or("").is_empty() {
                cfg.endpoint = Some("https://api.openai.com/v1/chat/completions".to_string());
            }
            Ok(Box::new(OpenAiCompatAdapter::new(
                cfg,
                Some(ProviderId::Openai),
                transport,
            )))
        }
        ProviderId::Baze => {
            if cfg.endpoint.as_deref().unwrap_or("").is_empty() {
                cfg.endpoint =
                    Some("https://api.baze.co/v1/gateway/chat/completions".to_string());
            }
            Ok(Box::new(OpenAiCompatAdapter::new(
                cfg,
                Some(ProviderId::Baze),
                transport,
            )))
        }
        ProviderId::Ollama => {
            if cfg.endpoint.as_deref().unwrap_or("").is_empty() {
                cfg.endpoint = Some("http://127.0.0.1:11434/v1/chat/completions".to_string());
            }
            Ok(Box::new(OpenAiCompatAdapter::new(
                cfg,
                Some(ProviderId::Ollama),
                transport,
            )))
        }
        ProviderId::Gemini => {
            if cfg.endpoint.as_deref().unwrap_or("").is_empty() {
                cfg.endpoint = Some(
                    "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions"
                        .to_string(),
                );
            }
            Ok(Box::new(OpenAiCompatAdapter::new(
                cfg,
                Some(ProviderId::Gemini),
                transport,
            )))
        }
        ProviderId::Custom => {
            let endpoint = cfg.endpoint.as_deref().filter(|s| !s.is_empty()).ok_or_else(|| {
                AiError::EndpointPolicy(
                    "사용자 지정 공급자(custom)는 엔드포인트 URL이 필수입니다.".to_string(),
                )
            })?;
            match validate_custom_endpoint(endpoint) {
                EndpointValidationResult::Ok { normalized } => {
                    cfg.endpoint = Some(normalized);
                    if cfg.custom_supports_tools.is_none() {
                        cfg.custom_supports_tools = Some(true);
                    }
                    Ok(Box::new(OpenAiCompatAdapter::new(
                        cfg,
                        Some(ProviderId::Custom),
                        transport,
                    )))
                }
                EndpointValidationResult::Err { reason } => Err(AiError::EndpointPolicy(format!(
                    "사용자 지정 공급자 엔드포인트 검증 실패: {reason}"
                ))),
            }
        }
    }
}
