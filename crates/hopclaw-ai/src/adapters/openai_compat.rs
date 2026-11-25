use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

use crate::adapters::ProviderAdapter;
use crate::endpoint_policy::{validate_custom_endpoint, EndpointValidationResult};
use crate::error::AiError;
use crate::proxy::{LlmProxyRequest, LlmTransport, DEFAULT_TIMEOUT_MS};
use crate::types::{
    AgentTurnRequest, AgentTurnResponse, BoxFuture, NeutralMessage, NeutralToolCall,
    ProviderConfig, ProviderId, ReasoningEffortOption, ToolChoice, ToolChoiceMode,
};

pub struct OpenAiCompatAdapter {
    id: ProviderId,
    supports_tools: bool,
    cfg: ProviderConfig,
    transport: Arc<dyn LlmTransport>,
}

impl OpenAiCompatAdapter {
    pub fn new(
        cfg: ProviderConfig,
        id: Option<ProviderId>,
        transport: Arc<dyn LlmTransport>,
    ) -> Self {
        let resolved_id = id.unwrap_or(cfg.provider);
        let supports_tools = if resolved_id == ProviderId::Custom {
            cfg.custom_supports_tools.unwrap_or(true)
        } else {
            true
        };
        Self {
            id: resolved_id,
            supports_tools,
            cfg,
            transport,
        }
    }
}

fn normalize_reasoning_effort(effort: Option<ReasoningEffortOption>) -> Option<&'static str> {
    let effort = effort?;
    match effort {
        ReasoningEffortOption::Max => Some("xhigh"),
        ReasoningEffortOption::None => Some("none"),
        ReasoningEffortOption::Low => Some("low"),
        ReasoningEffortOption::Medium => Some("medium"),
        ReasoningEffortOption::High => Some("high"),
        ReasoningEffortOption::Xhigh => Some("xhigh"),
    }
}

fn parse_status_from_error_message(msg: &str) -> Option<u16> {
    let marker = "LLM API Error (";
    let start = msg.find(marker)? + marker.len();
    let end = msg[start..].find(')')?;
    msg[start..start + end].parse::<u16>().ok()
}

fn is_transient_network_message(message: &str) -> bool {
    message.contains("fetch failed")
        || message.contains("timeout")
        || message.contains("Network")
        || message.contains("ECONNREFUSED")
        || message.contains("Local bridge error")
}

impl ProviderAdapter for OpenAiCompatAdapter {
    fn id(&self) -> ProviderId {
        self.id
    }

    fn supports_tools(&self) -> bool {
        self.supports_tools
    }

    fn turn(&self, req: AgentTurnRequest) -> BoxFuture<'_, Result<AgentTurnResponse, AiError>> {
        Box::pin(async move {
        if !req.tools.is_empty() && !self.supports_tools {
            return Err(AiError::ToolsUnsupported(format!(
                "Provider {:?} does not support tools.",
                self.id
            )));
        }

        let endpoint_url = match self.id {
            ProviderId::Baze => self
                .cfg
                .endpoint
                .as_deref()
                .filter(|s| !s.is_empty())
                .unwrap_or("https://api.baze.co/v1/gateway/chat/completions")
                .to_string(),
            ProviderId::Openai => self
                .cfg
                .endpoint
                .as_deref()
                .filter(|s| !s.is_empty())
                .unwrap_or("https://api.openai.com/v1/chat/completions")
                .to_string(),
            ProviderId::Ollama => self
                .cfg
                .endpoint
                .as_deref()
                .filter(|s| !s.is_empty())
                .unwrap_or("http://127.0.0.1:11434/v1/chat/completions")
                .to_string(),
            ProviderId::Gemini => self
                .cfg
                .endpoint
                .as_deref()
                .filter(|s| !s.is_empty())
                .unwrap_or("https://generativelanguage.googleapis.com/v1beta/openai/chat/completions")
                .to_string(),
            ProviderId::Custom => {
                let raw = self
                    .cfg
                    .endpoint
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| {
                        AiError::EndpointPolicy(
                            "사용자 지정 공급자(custom)의 엔드포인트 URL이 지정되지 않았습니다."
                                .to_string(),
                        )
                    })?;
                match validate_custom_endpoint(raw) {
                    EndpointValidationResult::Ok { normalized } => normalized,
                    EndpointValidationResult::Err { reason } => {
                        return Err(AiError::EndpointPolicy(format!(
                            "유효하지 않은 사용자 지정 엔드포인트: {reason}"
                        )));
                    }
                }
            }
            ProviderId::Claude => self
                .cfg
                .endpoint
                .as_deref()
                .filter(|s| !s.is_empty())
                .unwrap_or("https://api.openai.com/v1/chat/completions")
                .to_string(),
        };

        let mut target_url = endpoint_url;
        if !target_url.contains("/chat/completions") && !target_url.contains("/responses") {
            target_url = format!("{}/chat/completions", target_url.trim_end_matches('/'));
        }

        let api_key = if self.id == ProviderId::Ollama {
            ""
        } else {
            self.cfg.api_key.as_str()
        };

        let mut mapped_effort = req.reasoning_effort;
        match self.id {
            ProviderId::Gemini => {
                if matches!(
                    mapped_effort,
                    Some(ReasoningEffortOption::Xhigh | ReasoningEffortOption::Max)
                ) {
                    mapped_effort = Some(ReasoningEffortOption::High);
                }
            }
            ProviderId::Ollama => {
                mapped_effort = None;
            }
            ProviderId::Custom if !self.cfg.custom_send_reasoning.unwrap_or(false) => {
                mapped_effort = None;
            }
            _ => {}
        }

        // NeutralMessage -> OpenAI messages
        let has_system_msg = req
            .messages
            .iter()
            .any(|m| matches!(m, NeutralMessage::System { .. }));
        let mut body_messages = Vec::new();
        if !req.system_prompt.is_empty() && !has_system_msg {
            body_messages.push(json!({
                "role": "system",
                "content": req.system_prompt,
            }));
        }

        for m in &req.messages {
            match m {
                NeutralMessage::System { content } => {
                    body_messages.push(json!({ "role": "system", "content": content }));
                }
                NeutralMessage::User { content } => {
                    body_messages.push(json!({ "role": "user", "content": content }));
                }
                NeutralMessage::Assistant {
                    content,
                    tool_calls,
                } => {
                    let mut obj = Map::new();
                    obj.insert("role".to_string(), json!("assistant"));
                    obj.insert(
                        "content".to_string(),
                        match content {
                            Some(c) => json!(c),
                            None => Value::Null,
                        },
                    );
                    if let Some(calls) = tool_calls.as_ref().filter(|c| !c.is_empty()) {
                        let tc_arr: Vec<Value> = calls
                            .iter()
                            .map(|tc| {
                                json!({
                                    "id": tc.id,
                                    "type": "function",
                                    "function": {
                                        "name": tc.name,
                                        "arguments": tc.arguments_json,
                                    }
                                })
                            })
                            .collect();
                        obj.insert("tool_calls".to_string(), Value::Array(tc_arr));
                    }
                    body_messages.push(Value::Object(obj));
                }
                NeutralMessage::Tool {
                    tool_call_id,
                    name,
                    content,
                } => {
                    let mut obj = Map::new();
                    obj.insert("role".to_string(), json!("tool"));
                    obj.insert("tool_call_id".to_string(), json!(tool_call_id));
                    obj.insert("content".to_string(), json!(content));
                    if let Some(n) = name.as_ref().filter(|s| !s.is_empty()) {
                        obj.insert("name".to_string(), json!(n));
                    }
                    body_messages.push(Value::Object(obj));
                }
            }
        }

        let tools: Option<Vec<Value>> = if self.supports_tools && !req.tools.is_empty() {
            Some(
                req.tools
                    .iter()
                    .map(|t| {
                        json!({
                            "type": "function",
                            "function": {
                                "name": t.name,
                                "description": t.description,
                                "parameters": t.parameters,
                            }
                        })
                    })
                    .collect(),
            )
        } else {
            None
        };

        let tool_choice: Option<Value> = if tools.as_ref().is_some_and(|t| !t.is_empty()) {
            Some(match &req.tool_choice {
                ToolChoice::Mode(ToolChoiceMode::None) => json!("none"),
                ToolChoice::Mode(ToolChoiceMode::Auto) => json!("auto"),
                ToolChoice::Named { name } => json!({
                    "type": "function",
                    "function": { "name": name }
                }),
            })
        } else {
            None
        };

        let mut headers = HashMap::new();
        headers.insert("content-type".to_string(), "application/json".to_string());
        if !api_key.is_empty() {
            headers.insert("Authorization".to_string(), format!("Bearer {api_key}"));
            headers.insert("x-api-key".to_string(), api_key.to_string());
        }

        let model = if self.cfg.model.is_empty() {
            "gpt-6-luna"
        } else {
            self.cfg.model.as_str()
        };

        let mut body_map = Map::new();
        body_map.insert("model".to_string(), json!(model));
        body_map.insert("messages".to_string(), Value::Array(body_messages));
        if let Some(t) = tools {
            body_map.insert("tools".to_string(), Value::Array(t));
            body_map.insert(
                "tool_choice".to_string(),
                tool_choice.unwrap_or_else(|| json!("auto")),
            );
        }
        if let Some(effort) = normalize_reasoning_effort(mapped_effort) {
            if effort != "none" {
                body_map.insert("reasoning_effort".to_string(), json!(effort));
            }
        }

        let request_id = req
            .request_id
            .clone()
            .unwrap_or_else(|| format!("req_{}", Uuid::new_v4().simple()));
        let timeout_ms = req.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS);

        if let Some(token) = req.cancel_token.as_ref() {
            if token.is_cancelled() {
                let _ = self.transport.cancel(&request_id).await;
                return Err(AiError::Aborted);
            }
        }

        let cancel_handle = req.cancel_token.as_ref().map(|token| {
            let token = token.clone();
            let transport = Arc::clone(&self.transport);
            let req_id = request_id.clone();
            tokio::spawn(async move {
                token.cancelled().await;
                let _ = transport.cancel(&req_id).await;
            })
        });

        let exec_res = async {
            let mut resp = self
                .transport
                .call_http(LlmProxyRequest {
                    url: target_url.clone(),
                    headers: headers.clone(),
                    body_json: Value::Object(body_map.clone()),
                    timeout_ms: Some(timeout_ms),
                    request_id: Some(request_id.clone()),
                })
                .await?;

            // Fallback retry when 400 rejects reasoning_effort
            if resp.status == 400 && body_map.contains_key("reasoning_effort") {
                let err_text = serde_json::to_string(&resp.body).unwrap_or_default();
                if err_text.contains("reasoning_effort") {
                    body_map.remove("reasoning_effort");
                    resp = self
                        .transport
                        .call_http(LlmProxyRequest {
                            url: target_url,
                            headers,
                            body_json: Value::Object(body_map),
                            timeout_ms: Some(timeout_ms),
                            request_id: Some(request_id.clone()),
                        })
                        .await?;
                }
            }

            if resp.status < 200 || resp.status >= 300 {
                let err_json = serde_json::to_string(&resp.body).unwrap_or_default();
                return Err(AiError::Provider(format!(
                    "LLM API Error ({}): {}",
                    resp.status, err_json
                )));
            }

            let choice = resp
                .body
                .get("choices")
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.first())
                .ok_or_else(|| {
                    AiError::Provider(format!(
                        "LLM returned empty choices: {}",
                        serde_json::to_string(&resp.body).unwrap_or_default()
                    ))
                })?;

            let message = choice.get("message").cloned().unwrap_or_else(|| json!({}));
            let raw_tool_calls = message
                .get("tool_calls")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();

            let neutral_tool_calls: Vec<NeutralToolCall> = raw_tool_calls
                .into_iter()
                .map(|tc| NeutralToolCall {
                    id: tc
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    name: tc
                        .get("function")
                        .and_then(|f| f.get("name"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    arguments_json: tc
                        .get("function")
                        .and_then(|f| f.get("arguments"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("{}")
                        .to_string(),
                })
                .collect();

            let content = message
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let reasoning_tokens = resp
                .body
                .get("usage")
                .and_then(|u| u.get("completion_tokens_details"))
                .and_then(|d| d.get("reasoning_tokens"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0);

            let stop_reason = message
                .get("finish_reason")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            Ok(AgentTurnResponse {
                content,
                tool_calls: neutral_tool_calls,
                reasoning_tokens: Some(reasoning_tokens),
                stop_reason,
            })
        }
        .await;

        if let Some(token) = req.cancel_token.as_ref() {
            if token.is_cancelled() {
                if let Some(handle) = cancel_handle {
                    let _ = handle.await;
                }
            } else if let Some(handle) = cancel_handle {
                handle.abort();
            }
        }

        match exec_res {
            Ok(ok) => Ok(ok),
            Err(
                e @ (AiError::ToolsUnsupported(_)
                | AiError::TransientProvider { .. }
                | AiError::Aborted),
            ) => Err(e),
            Err(err) => {
                let message = err.to_string();
                let status = parse_status_from_error_message(&message);

                if status == Some(429) || status.is_some_and(|s| (500..600).contains(&s)) {
                    return Err(AiError::TransientProvider { message, status });
                }
                if status.is_none() && is_transient_network_message(&message) {
                    return Err(AiError::TransientProvider {
                        message,
                        status: None,
                    });
                }
                if status == Some(400) {
                    let lower = message.to_ascii_lowercase();
                    let mentions_tool = lower.contains("tool") || lower.contains("function");
                    let mentions_unsupported = lower.contains("not support")
                        || lower.contains("unsupported")
                        || lower.contains("not enabled")
                        || lower.contains("does not support");
                    if mentions_tool && mentions_unsupported {
                        return Err(AiError::ToolsUnsupported(message));
                    }
                }
                Err(err)
            }
        }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::create_provider_adapter;
    use crate::proxy::LlmProxyResponse;
    use crate::types::{CancelToken, NeutralToolSpec};
    use std::collections::VecDeque;
    use std::sync::Mutex;

    type CallHook = Box<dyn Fn(&LlmProxyRequest) + Send + Sync>;

    struct MockTransport {
        responses: Mutex<VecDeque<Result<LlmProxyResponse, AiError>>>,
        calls: Mutex<Vec<LlmProxyRequest>>,
        cancelled_ids: Mutex<Vec<String>>,
        on_call: Option<CallHook>,
    }

    impl MockTransport {
        fn new(responses: Vec<Result<LlmProxyResponse, AiError>>) -> Self {
            Self {
                responses: Mutex::new(VecDeque::from(responses)),
                calls: Mutex::new(Vec::new()),
                cancelled_ids: Mutex::new(Vec::new()),
                on_call: None,
            }
        }

        fn with_hook<F>(responses: Vec<Result<LlmProxyResponse, AiError>>, hook: F) -> Self
        where
            F: Fn(&LlmProxyRequest) + Send + Sync + 'static,
        {
            Self {
                responses: Mutex::new(VecDeque::from(responses)),
                calls: Mutex::new(Vec::new()),
                cancelled_ids: Mutex::new(Vec::new()),
                on_call: Some(Box::new(hook)),
            }
        }
    }

    impl LlmTransport for MockTransport {
        fn call_http(&self, req: LlmProxyRequest) -> BoxFuture<'_, Result<LlmProxyResponse, AiError>> {
            Box::pin(async move {
                self.calls.lock().unwrap().push(req.clone());
                if let Some(hook) = &self.on_call {
                    hook(&req);
                }
                self.responses
                    .lock()
                    .unwrap()
                    .pop_front()
                    .expect("unexpected call_http")
            })
        }

        fn cancel<'a>(&'a self, request_id: &'a str) -> BoxFuture<'a, Result<bool, AiError>> {
            Box::pin(async move {
                self.cancelled_ids
                    .lock()
                    .unwrap()
                    .push(request_id.to_string());
                Ok(true)
            })
        }
    }

    fn cfg_for(provider: ProviderId, api_key: &str, model: &str) -> ProviderConfig {
        ProviderConfig {
            provider,
            api_key: api_key.to_string(),
            model: model.to_string(),
            endpoint: None,
            custom_name: None,
            custom_supports_tools: None,
            custom_send_reasoning: None,
        }
    }

    #[tokio::test]
    async fn converts_neutral_messages_and_parses_openai_response() {
        let transport = Arc::new(MockTransport::new(vec![Ok(LlmProxyResponse {
            status: 200,
            body: json!({
                "choices": [
                    {
                        "message": {
                            "role": "assistant",
                            "content": "Here is your response",
                            "tool_calls": [
                                {
                                    "id": "call_123",
                                    "type": "function",
                                    "function": {
                                        "name": "propose_hwp_edits",
                                        "arguments": r#"{"diffItems":[]}"#
                                    }
                                }
                            ]
                        }
                    }
                ],
                "usage": {
                    "completion_tokens_details": {
                        "reasoning_tokens": 42
                    }
                }
            }),
        })]));

        let adapter = OpenAiCompatAdapter::new(
            cfg_for(ProviderId::Openai, "test-key", "gpt-4o"),
            None,
            transport.clone(),
        );

        let res = adapter
            .turn(AgentTurnRequest {
                system_prompt: "System instruction".to_string(),
                messages: vec![
                    NeutralMessage::User {
                        content: "Hello".to_string(),
                    },
                    NeutralMessage::Assistant {
                        content: None,
                        tool_calls: Some(vec![NeutralToolCall {
                            id: "call_000".to_string(),
                            name: "inspect_layout".to_string(),
                            arguments_json: "{}".to_string(),
                        }]),
                    },
                    NeutralMessage::Tool {
                        tool_call_id: "call_000".to_string(),
                        name: None,
                        content: r#"{"status":"ok"}"#.to_string(),
                    },
                ],
                tools: vec![NeutralToolSpec {
                    name: "propose_hwp_edits".to_string(),
                    description: "Propose edits".to_string(),
                    parameters: json!({ "type": "object", "properties": {} }),
                }],
                tool_choice: ToolChoice::named("propose_hwp_edits"),
                reasoning_effort: None,
                request_id: Some("test-req-1".to_string()),
                timeout_ms: None,
                cancel_token: None,
            })
            .await
            .unwrap();

        let calls = transport.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].url, "https://api.openai.com/v1/chat/completions");
        assert_eq!(
            calls[0].headers.get("Authorization").map(String::as_str),
            Some("Bearer test-key")
        );
        assert_eq!(
            calls[0].body_json["tool_choice"],
            json!({ "type": "function", "function": { "name": "propose_hwp_edits" } })
        );
        assert_eq!(
            calls[0].body_json["messages"],
            json!([
                { "role": "system", "content": "System instruction" },
                { "role": "user", "content": "Hello" },
                {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [
                        {
                            "id": "call_000",
                            "type": "function",
                            "function": { "name": "inspect_layout", "arguments": "{}" }
                        }
                    ]
                },
                { "role": "tool", "tool_call_id": "call_000", "content": r#"{"status":"ok"}"# }
            ])
        );

        assert_eq!(res.content, "Here is your response");
        assert_eq!(
            res.tool_calls,
            vec![NeutralToolCall {
                id: "call_123".to_string(),
                name: "propose_hwp_edits".to_string(),
                arguments_json: r#"{"diffItems":[]}"#.to_string(),
            }]
        );
        assert_eq!(res.reasoning_tokens, Some(42));
    }

    #[tokio::test]
    async fn handles_tool_choice_none() {
        let transport = Arc::new(MockTransport::new(vec![Ok(LlmProxyResponse {
            status: 200,
            body: json!({
                "choices": [{ "message": { "role": "assistant", "content": "No tools used" } }]
            }),
        })]));

        let adapter = OpenAiCompatAdapter::new(
            cfg_for(ProviderId::Baze, "baze-key", "gpt-6-luna"),
            None,
            transport.clone(),
        );

        adapter
            .turn(AgentTurnRequest {
                system_prompt: "sys".to_string(),
                messages: vec![NeutralMessage::User {
                    content: "hi".to_string(),
                }],
                tools: vec![NeutralToolSpec {
                    name: "test_tool".to_string(),
                    description: "d".to_string(),
                    parameters: json!({}),
                }],
                tool_choice: ToolChoice::none(),
                reasoning_effort: None,
                request_id: None,
                timeout_ms: None,
                cancel_token: None,
            })
            .await
            .unwrap();

        let calls = transport.calls.lock().unwrap();
        assert_eq!(calls[0].body_json["tool_choice"], "none");
    }

    #[tokio::test]
    async fn maps_gemini_xhigh_reasoning_effort_to_high() {
        let transport = Arc::new(MockTransport::new(vec![Ok(LlmProxyResponse {
            status: 200,
            body: json!({ "choices": [{ "message": { "content": "ok" } }] }),
        })]));

        let adapter = create_provider_adapter(
            cfg_for(ProviderId::Gemini, "gem-key", "gemini-2.5-pro"),
            transport.clone(),
        )
        .unwrap();

        adapter
            .turn(AgentTurnRequest {
                system_prompt: "sys".to_string(),
                messages: vec![NeutralMessage::User {
                    content: "hi".to_string(),
                }],
                tools: vec![],
                tool_choice: ToolChoice::auto(),
                reasoning_effort: Some(ReasoningEffortOption::Xhigh),
                request_id: None,
                timeout_ms: None,
                cancel_token: None,
            })
            .await
            .unwrap();

        let calls = transport.calls.lock().unwrap();
        assert_eq!(calls[0].body_json["reasoning_effort"], "high");
    }

    #[tokio::test]
    async fn handles_ollama_without_api_key_or_reasoning_effort() {
        let transport = Arc::new(MockTransport::new(vec![Ok(LlmProxyResponse {
            status: 200,
            body: json!({ "choices": [{ "message": { "content": "ok" } }] }),
        })]));

        let adapter = create_provider_adapter(
            cfg_for(ProviderId::Ollama, "ignored-key", "qwen2.5:14b"),
            transport.clone(),
        )
        .unwrap();

        adapter
            .turn(AgentTurnRequest {
                system_prompt: "sys".to_string(),
                messages: vec![NeutralMessage::User {
                    content: "hi".to_string(),
                }],
                tools: vec![],
                tool_choice: ToolChoice::auto(),
                reasoning_effort: Some(ReasoningEffortOption::High),
                request_id: None,
                timeout_ms: None,
                cancel_token: None,
            })
            .await
            .unwrap();

        let calls = transport.calls.lock().unwrap();
        assert!(!calls[0].headers.contains_key("Authorization"));
        assert!(calls[0].body_json.get("reasoning_effort").is_none());
    }

    #[tokio::test]
    async fn handles_custom_provider_reasoning_flag() {
        let transport = Arc::new(MockTransport::new(vec![
            Ok(LlmProxyResponse {
                status: 200,
                body: json!({ "choices": [{ "message": { "content": "ok" } }] }),
            }),
            Ok(LlmProxyResponse {
                status: 200,
                body: json!({ "choices": [{ "message": { "content": "ok" } }] }),
            }),
        ]));

        let mut cfg_no = cfg_for(ProviderId::Custom, "custom-key", "local-model");
        cfg_no.endpoint = Some("http://127.0.0.1:8000/v1".to_string());
        cfg_no.custom_send_reasoning = Some(false);
        let adapter_no = create_provider_adapter(cfg_no, transport.clone()).unwrap();

        adapter_no
            .turn(AgentTurnRequest {
                system_prompt: "sys".to_string(),
                messages: vec![NeutralMessage::User {
                    content: "hi".to_string(),
                }],
                tools: vec![],
                tool_choice: ToolChoice::auto(),
                reasoning_effort: Some(ReasoningEffortOption::Medium),
                request_id: None,
                timeout_ms: None,
                cancel_token: None,
            })
            .await
            .unwrap();

        let mut cfg_yes = cfg_for(ProviderId::Custom, "custom-key", "local-model");
        cfg_yes.endpoint = Some("http://127.0.0.1:8000/v1".to_string());
        cfg_yes.custom_send_reasoning = Some(true);
        let adapter_yes = create_provider_adapter(cfg_yes, transport.clone()).unwrap();

        adapter_yes
            .turn(AgentTurnRequest {
                system_prompt: "sys".to_string(),
                messages: vec![NeutralMessage::User {
                    content: "hi".to_string(),
                }],
                tools: vec![],
                tool_choice: ToolChoice::auto(),
                reasoning_effort: Some(ReasoningEffortOption::Medium),
                request_id: None,
                timeout_ms: None,
                cancel_token: None,
            })
            .await
            .unwrap();

        let calls = transport.calls.lock().unwrap();
        assert!(calls[0].body_json.get("reasoning_effort").is_none());
        assert_eq!(calls[1].body_json["reasoning_effort"], "medium");
    }

    #[tokio::test]
    async fn throws_transient_on_429_and_503() {
        for status in [429u16, 503u16] {
            let transport = Arc::new(MockTransport::new(vec![Ok(LlmProxyResponse {
                status,
                body: json!({ "error": "error" }),
            })]));
            let adapter =
                create_provider_adapter(cfg_for(ProviderId::Openai, "test", "gpt-4o"), transport)
                    .unwrap();
            let err = adapter
                .turn(AgentTurnRequest {
                    system_prompt: "sys".to_string(),
                    messages: vec![NeutralMessage::User {
                        content: "hi".to_string(),
                    }],
                    tools: vec![],
                    tool_choice: ToolChoice::auto(),
                    reasoning_effort: None,
                    request_id: None,
                    timeout_ms: None,
                    cancel_token: None,
                })
                .await
                .unwrap_err();
            assert!(matches!(
                err,
                AiError::TransientProvider {
                    status: Some(s),
                    ..
                } if s == status
            ));
        }
    }

    #[tokio::test]
    async fn throws_tools_unsupported_on_400() {
        let transport = Arc::new(MockTransport::new(vec![Ok(LlmProxyResponse {
            status: 400,
            body: json!({ "error": { "message": "This model does not support tools or function calling" } }),
        })]));
        let adapter = create_provider_adapter(
            cfg_for(ProviderId::Openai, "test", "gpt-3.5-turbo"),
            transport,
        )
        .unwrap();
        let err = adapter
            .turn(AgentTurnRequest {
                system_prompt: "sys".to_string(),
                messages: vec![NeutralMessage::User {
                    content: "hi".to_string(),
                }],
                tools: vec![NeutralToolSpec {
                    name: "test".to_string(),
                    description: "desc".to_string(),
                    parameters: json!({}),
                }],
                tool_choice: ToolChoice::auto(),
                reasoning_effort: None,
                request_id: None,
                timeout_ms: None,
                cancel_token: None,
            })
            .await
            .unwrap_err();
        assert!(matches!(err, AiError::ToolsUnsupported(_)));
    }

    #[tokio::test]
    async fn retries_on_400_reasoning_effort_error() {
        let transport = Arc::new(MockTransport::new(vec![
            Ok(LlmProxyResponse {
                status: 400,
                body: json!({ "error": "Unknown parameter: reasoning_effort is not supported" }),
            }),
            Ok(LlmProxyResponse {
                status: 200,
                body: json!({ "choices": [{ "message": { "content": "Fallback success" } }] }),
            }),
        ]));

        let adapter = create_provider_adapter(
            cfg_for(ProviderId::Openai, "test", "gpt-4o"),
            transport.clone(),
        )
        .unwrap();

        let res = adapter
            .turn(AgentTurnRequest {
                system_prompt: "sys".to_string(),
                messages: vec![NeutralMessage::User {
                    content: "hi".to_string(),
                }],
                tools: vec![],
                tool_choice: ToolChoice::auto(),
                reasoning_effort: Some(ReasoningEffortOption::Medium),
                request_id: None,
                timeout_ms: None,
                cancel_token: None,
            })
            .await
            .unwrap();

        let calls = transport.calls.lock().unwrap();
        assert_eq!(calls.len(), 2);
        assert!(calls[1].body_json.get("reasoning_effort").is_none());
        assert_eq!(res.content, "Fallback success");
    }

    #[tokio::test]
    async fn cancels_request_when_cancel_token_triggers() {
        let token = CancelToken::new();
        let hook_token = token.clone();
        let transport = Arc::new(MockTransport::with_hook(
            vec![Ok(LlmProxyResponse {
                status: 200,
                body: json!({ "choices": [{ "message": { "content": "done" } }] }),
            })],
            move |_| {
                hook_token.cancel();
            },
        ));

        let adapter = create_provider_adapter(
            cfg_for(ProviderId::Openai, "test", "gpt-4o"),
            transport.clone(),
        )
        .unwrap();

        adapter
            .turn(AgentTurnRequest {
                system_prompt: "sys".to_string(),
                messages: vec![NeutralMessage::User {
                    content: "hi".to_string(),
                }],
                tools: vec![],
                tool_choice: ToolChoice::auto(),
                reasoning_effort: None,
                request_id: Some("req-abort-test".to_string()),
                timeout_ms: None,
                cancel_token: Some(token),
            })
            .await
            .unwrap();

        let cancelled = transport.cancelled_ids.lock().unwrap();
        assert_eq!(cancelled.as_slice(), &["req-abort-test".to_string()]);
    }
}
