use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

use crate::adapters::ProviderAdapter;
use crate::error::AiError;
use crate::proxy::{LlmProxyRequest, LlmTransport, DEFAULT_TIMEOUT_MS};
use crate::types::{
    AgentTurnRequest, AgentTurnResponse, BoxFuture, NeutralMessage, NeutralToolCall,
    ProviderConfig, ProviderId, ToolChoice, ToolChoiceMode,
};

pub struct ClaudeAdapter {
    cfg: ProviderConfig,
    transport: Arc<dyn LlmTransport>,
}

impl ClaudeAdapter {
    pub fn new(cfg: ProviderConfig, transport: Arc<dyn LlmTransport>) -> Self {
        Self { cfg, transport }
    }
}

#[derive(Debug, Clone)]
enum ClaudeRole {
    User,
    Assistant,
}

impl ClaudeRole {
    fn as_str(&self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
        }
    }
}

fn is_transient_message(message: &str) -> bool {
    message.contains("fetch failed")
        || message.contains("timeout")
        || message.contains("Network")
        || message.contains("ECONNREFUSED")
        || message.contains("Local bridge error")
}

impl ProviderAdapter for ClaudeAdapter {
    fn id(&self) -> ProviderId {
        ProviderId::Claude
    }

    fn supports_tools(&self) -> bool {
        true
    }

    fn turn(&self, req: AgentTurnRequest) -> BoxFuture<'_, Result<AgentTurnResponse, AiError>> {
        Box::pin(async move {
        let url = self
            .cfg
            .endpoint
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or("https://api.anthropic.com/v1/messages")
            .to_string();

        let mut headers = HashMap::new();
        headers.insert("anthropic-version".to_string(), "2023-06-01".to_string());
        headers.insert("content-type".to_string(), "application/json".to_string());
        if !self.cfg.api_key.is_empty() {
            headers.insert("x-api-key".to_string(), self.cfg.api_key.clone());
        }

        // System prompt collection
        let mut system_prompt = req.system_prompt.clone();
        let extra_system: Vec<&str> = req
            .messages
            .iter()
            .filter_map(|m| match m {
                NeutralMessage::System { content } => Some(content.as_str()),
                _ => None,
            })
            .collect();
        if !extra_system.is_empty() {
            let extra = extra_system.join("\n\n");
            if system_prompt.is_empty() {
                system_prompt = extra;
            } else {
                system_prompt = format!("{system_prompt}\n\n{extra}");
            }
        }

        // Convert neutral messages into intermediate (role, content blocks)
        let mut intermediate: Vec<(ClaudeRole, Vec<Value>)> = Vec::new();
        for m in &req.messages {
            match m {
                NeutralMessage::System { .. } => {}
                NeutralMessage::User { content } => {
                    intermediate.push((
                        ClaudeRole::User,
                        vec![json!({ "type": "text", "text": content })],
                    ));
                }
                NeutralMessage::Assistant {
                    content,
                    tool_calls,
                } => {
                    let mut blocks = Vec::new();
                    if let Some(text) = content.as_deref().filter(|s| !s.is_empty()) {
                        blocks.push(json!({ "type": "text", "text": text }));
                    }
                    if let Some(calls) = tool_calls {
                        for tc in calls {
                            let input: Value = serde_json::from_str(&tc.arguments_json)
                                .unwrap_or_else(|_| json!({}));
                            blocks.push(json!({
                                "type": "tool_use",
                                "id": tc.id,
                                "name": tc.name,
                                "input": input,
                            }));
                        }
                    }
                    if blocks.is_empty() {
                        blocks.push(json!({ "type": "text", "text": "" }));
                    }
                    intermediate.push((ClaudeRole::Assistant, blocks));
                }
                NeutralMessage::Tool {
                    tool_call_id,
                    content,
                    ..
                } => {
                    intermediate.push((
                        ClaudeRole::User,
                        vec![json!({
                            "type": "tool_result",
                            "tool_use_id": tool_call_id,
                            "content": content,
                        })],
                    ));
                }
            }
        }

        // Merge consecutive messages with the same role (e.g., multiple tool_result into one user message)
        let mut merged: Vec<(ClaudeRole, Vec<Value>)> = Vec::new();
        for (role, blocks) in intermediate {
            if let Some(last) = merged.last_mut() {
                if last.0.as_str() == role.as_str() {
                    last.1.extend(blocks);
                    continue;
                }
            }
            merged.push((role, blocks));
        }

        // First message must be user
        if merged.is_empty() {
            merged.push((
                ClaudeRole::User,
                vec![json!({ "type": "text", "text": "시작" })],
            ));
        } else if !matches!(merged[0].0, ClaudeRole::User) {
            merged.insert(
                0,
                (
                    ClaudeRole::User,
                    vec![json!({ "type": "text", "text": "진행" })],
                ),
            );
        }

        let merged_messages: Vec<Value> = merged
            .into_iter()
            .map(|(role, content)| json!({ "role": role.as_str(), "content": content }))
            .collect();

        let claude_tools: Vec<Value> = req
            .tools
            .iter()
            .map(|t| {
                json!({
                    "name": t.name,
                    "description": t.description,
                    "input_schema": t.parameters,
                })
            })
            .collect();

        let model = if self.cfg.model.is_empty() {
            "claude-sonnet-4-20250514"
        } else {
            self.cfg.model.as_str()
        };

        let mut body_map = Map::new();
        body_map.insert("model".to_string(), json!(model));
        body_map.insert("max_tokens".to_string(), json!(8192));
        body_map.insert("messages".to_string(), Value::Array(merged_messages));

        if !system_prompt.is_empty() {
            body_map.insert("system".to_string(), json!(system_prompt));
        }

        if !matches!(req.tool_choice, ToolChoice::Mode(ToolChoiceMode::None))
            && !claude_tools.is_empty()
        {
            body_map.insert("tools".to_string(), Value::Array(claude_tools));
            let tc_json = match &req.tool_choice {
                ToolChoice::Mode(ToolChoiceMode::Auto) => json!({ "type": "auto" }),
                ToolChoice::Named { name } => json!({ "type": "tool", "name": name }),
                ToolChoice::Mode(ToolChoiceMode::None) => unreachable!(),
            };
            body_map.insert("tool_choice".to_string(), tc_json);
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

        let call_result = self
            .transport
            .call_http(LlmProxyRequest {
                url,
                headers,
                body_json: Value::Object(body_map),
                timeout_ms: Some(timeout_ms),
                request_id: Some(request_id.clone()),
            })
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

        let resp = match call_result {
            Ok(r) => r,
            Err(e @ (AiError::ToolsUnsupported(_) | AiError::TransientProvider { .. } | AiError::Aborted)) => {
                return Err(e);
            }
            Err(err) => {
                let msg = err.to_string();
                if is_transient_message(&msg) {
                    return Err(AiError::TransientProvider {
                        message: msg,
                        status: None,
                    });
                }
                return Err(err);
            }
        };

        if resp.status < 200 || resp.status >= 300 {
            let err_text = match &resp.body {
                Value::String(s) => s.clone(),
                Value::Null => "{}".to_string(),
                other => serde_json::to_string(other).unwrap_or_else(|_| "{}".to_string()),
            };

            if resp.status == 429 || resp.status >= 500 {
                return Err(AiError::TransientProvider {
                    message: format!("Claude API Error ({}): {}", resp.status, err_text),
                    status: Some(resp.status),
                });
            }

            if resp.status == 400 {
                let lower = err_text.to_ascii_lowercase();
                if (lower.contains("tool") || lower.contains("function"))
                    && (lower.contains("not support")
                        || lower.contains("unsupported")
                        || lower.contains("not enabled")
                        || lower.contains("does not support"))
                {
                    return Err(AiError::ToolsUnsupported(format!(
                        "Claude API Error ({}): {}",
                        resp.status, err_text
                    )));
                }
            }

            return Err(AiError::Provider(format!(
                "Claude API Error ({}): {}",
                resp.status, err_text
            )));
        }

        let content_blocks = resp
            .body
            .get("content")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let mut text_content = String::new();
        let mut neutral_tool_calls = Vec::new();

        for block in content_blocks {
            let block_type = block.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if block_type == "text" {
                if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
                    if !text.is_empty() {
                        if !text_content.is_empty() {
                            text_content.push('\n');
                        }
                        text_content.push_str(text);
                    }
                }
            } else if block_type == "tool_use" {
                let id = block
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let name = block
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let arguments_json = match block.get("input") {
                    Some(Value::String(s)) => s.clone(),
                    Some(other) if !other.is_null() => {
                        serde_json::to_string(other).unwrap_or_else(|_| "{}".to_string())
                    }
                    _ => "{}".to_string(),
                };
                neutral_tool_calls.push(NeutralToolCall {
                    id,
                    name,
                    arguments_json,
                });
            }
        }

        let stop_reason = resp
            .body
            .get("stop_reason")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        Ok(AgentTurnResponse {
            content: text_content,
            tool_calls: neutral_tool_calls,
            reasoning_tokens: Some(0),
            stop_reason,
        })
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

        fn with_hook<F>(
            responses: Vec<Result<LlmProxyResponse, AiError>>,
            hook: F,
        ) -> Self
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

    fn base_cfg() -> ProviderConfig {
        ProviderConfig {
            provider: ProviderId::Claude,
            api_key: "sk-ant-test".to_string(),
            model: "claude-sonnet-4-20250514".to_string(),
            endpoint: None,
            custom_name: None,
            custom_supports_tools: None,
            custom_send_reasoning: None,
        }
    }

    #[tokio::test]
    async fn merges_consecutive_tool_results_and_enforces_role_alternation() {
        let transport = Arc::new(MockTransport::new(vec![Ok(LlmProxyResponse {
            status: 200,
            body: json!({
                "id": "msg_123",
                "type": "message",
                "role": "assistant",
                "content": [
                    { "type": "text", "text": "I have inspected the layout." },
                    {
                        "type": "tool_use",
                        "id": "toolu_456",
                        "name": "propose_hwp_edits",
                        "input": { "diffItems": [] }
                    }
                ],
                "stop_reason": "tool_use"
            }),
        })]));

        let adapter = create_provider_adapter(base_cfg(), transport.clone()).unwrap();

        let res = adapter
            .turn(AgentTurnRequest {
                system_prompt: "You are a document editor.".to_string(),
                messages: vec![
                    NeutralMessage::System {
                        content: "Extra system instruction".to_string(),
                    },
                    NeutralMessage::User {
                        content: "Please fix document".to_string(),
                    },
                    NeutralMessage::Assistant {
                        content: Some("Let me check the layout first".to_string()),
                        tool_calls: Some(vec![
                            NeutralToolCall {
                                id: "call_layout".to_string(),
                                name: "inspect_layout".to_string(),
                                arguments_json: "{}".to_string(),
                            },
                            NeutralToolCall {
                                id: "call_read".to_string(),
                                name: "read_document".to_string(),
                                arguments_json: r#"{"start":0}"#.to_string(),
                            },
                        ]),
                    },
                    NeutralMessage::Tool {
                        tool_call_id: "call_layout".to_string(),
                        name: None,
                        content: r#"{"status":"ok"}"#.to_string(),
                    },
                    NeutralMessage::Tool {
                        tool_call_id: "call_read".to_string(),
                        name: None,
                        content: r#"{"paragraphs":["line1"]}"#.to_string(),
                    },
                ],
                tools: vec![NeutralToolSpec {
                    name: "propose_hwp_edits".to_string(),
                    description: "Propose edits".to_string(),
                    parameters: json!({ "type": "object", "properties": {} }),
                }],
                tool_choice: ToolChoice::named("propose_hwp_edits"),
                reasoning_effort: None,
                request_id: Some("claude-req-1".to_string()),
                timeout_ms: None,
                cancel_token: None,
            })
            .await
            .unwrap();

        let calls = transport.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].url, "https://api.anthropic.com/v1/messages");
        assert_eq!(
            calls[0].headers.get("x-api-key").map(String::as_str),
            Some("sk-ant-test")
        );
        assert_eq!(
            calls[0]
                .headers
                .get("anthropic-version")
                .map(String::as_str),
            Some("2023-06-01")
        );

        let body = &calls[0].body_json;
        assert_eq!(body["max_tokens"], 8192);
        assert!(body["system"]
            .as_str()
            .unwrap()
            .contains("You are a document editor."));
        assert!(body["system"]
            .as_str()
            .unwrap()
            .contains("Extra system instruction"));

        let msgs = body["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0]["role"], "user");
        assert_eq!(msgs[1]["role"], "assistant");
        assert_eq!(msgs[2]["role"], "user");
        assert_eq!(
            msgs[2]["content"],
            json!([
                { "type": "tool_result", "tool_use_id": "call_layout", "content": r#"{"status":"ok"}"# },
                { "type": "tool_result", "tool_use_id": "call_read", "content": r#"{"paragraphs":["line1"]}"# }
            ])
        );

        assert_eq!(
            body["tool_choice"],
            json!({ "type": "tool", "name": "propose_hwp_edits" })
        );

        assert_eq!(res.content, "I have inspected the layout.");
        assert_eq!(
            res.tool_calls,
            vec![NeutralToolCall {
                id: "toolu_456".to_string(),
                name: "propose_hwp_edits".to_string(),
                arguments_json: r#"{"diffItems":[]}"#.to_string(),
            }]
        );
        assert_eq!(res.stop_reason.as_deref(), Some("tool_use"));
    }

    #[tokio::test]
    async fn omits_tools_and_tool_choice_when_none() {
        let transport = Arc::new(MockTransport::new(vec![Ok(LlmProxyResponse {
            status: 200,
            body: json!({ "content": [{ "type": "text", "text": "Answer without tools" }] }),
        })]));

        let adapter = ClaudeAdapter::new(base_cfg(), transport.clone());
        adapter
            .turn(AgentTurnRequest {
                system_prompt: "sys".to_string(),
                messages: vec![NeutralMessage::User {
                    content: "hi".to_string(),
                }],
                tools: vec![NeutralToolSpec {
                    name: "test_tool".to_string(),
                    description: "desc".to_string(),
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
        assert!(calls[0].body_json.get("tools").is_none());
        assert!(calls[0].body_json.get("tool_choice").is_none());
    }

    #[tokio::test]
    async fn sets_tool_choice_auto() {
        let transport = Arc::new(MockTransport::new(vec![Ok(LlmProxyResponse {
            status: 200,
            body: json!({ "content": [{ "type": "text", "text": "Auto tool turn" }] }),
        })]));

        let adapter = ClaudeAdapter::new(base_cfg(), transport.clone());
        adapter
            .turn(AgentTurnRequest {
                system_prompt: "sys".to_string(),
                messages: vec![NeutralMessage::User {
                    content: "hi".to_string(),
                }],
                tools: vec![NeutralToolSpec {
                    name: "test_tool".to_string(),
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
            .unwrap();

        let calls = transport.calls.lock().unwrap();
        assert!(calls[0].body_json.get("tools").is_some());
        assert_eq!(calls[0].body_json["tool_choice"], json!({ "type": "auto" }));
    }

    #[tokio::test]
    async fn returns_transient_error_on_429() {
        let transport = Arc::new(MockTransport::new(vec![Ok(LlmProxyResponse {
            status: 429,
            body: json!({ "error": { "message": "Claude rate limit exceeded" } }),
        })]));

        let adapter = ClaudeAdapter::new(base_cfg(), transport);
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
                status: Some(429),
                ..
            }
        ));
    }

    #[tokio::test]
    async fn returns_tools_unsupported_on_400() {
        let transport = Arc::new(MockTransport::new(vec![Ok(LlmProxyResponse {
            status: 400,
            body: json!({ "error": { "message": "tools is not supported for this model" } }),
        })]));

        let adapter = ClaudeAdapter::new(base_cfg(), transport);
        let err = adapter
            .turn(AgentTurnRequest {
                system_prompt: "sys".to_string(),
                messages: vec![NeutralMessage::User {
                    content: "hi".to_string(),
                }],
                tools: vec![NeutralToolSpec {
                    name: "t".to_string(),
                    description: "d".to_string(),
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
    async fn cancels_request_when_cancel_token_triggers() {
        let token = CancelToken::new();
        let hook_token = token.clone();
        let transport = Arc::new(MockTransport::with_hook(
            vec![Ok(LlmProxyResponse {
                status: 200,
                body: json!({ "content": [{ "type": "text", "text": "done" }] }),
            })],
            move |_| {
                hook_token.cancel();
            },
        ));

        let adapter = ClaudeAdapter::new(base_cfg(), transport.clone());
        let _ = adapter
            .turn(AgentTurnRequest {
                system_prompt: "sys".to_string(),
                messages: vec![NeutralMessage::User {
                    content: "hi".to_string(),
                }],
                tools: vec![],
                tool_choice: ToolChoice::auto(),
                reasoning_effort: None,
                request_id: Some("claude-abort-test".to_string()),
                timeout_ms: None,
                cancel_token: Some(token),
            })
            .await
            .unwrap();

        let cancelled = transport.cancelled_ids.lock().unwrap();
        assert_eq!(cancelled.as_slice(), &["claude-abort-test".to_string()]);
    }
}
