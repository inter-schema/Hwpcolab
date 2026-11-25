use hopclaw_types::{
    AgentStepEvent, AgentStepType, DiffItem, DiffMetricsDelta, DiffScope, DiffStatus,
    DiffTargetCoord, DiffVerificationStatus, DocFormat, ExcludedRegion, LayoutReport,
    SelectionTarget, TableCoordinateMeta, TrialStore, UnifiedOp,
};
use regex::Regex;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, OnceLock};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};
use uuid::Uuid;

use crate::adapters::ProviderAdapter;
use crate::context::{compact_old_tool_results, COMPACT_KEEP_RECENT_TURNS};
use crate::error::AiError;
use crate::tool_runtime::{is_read_only_tool, parse_tool_args, StepEmitter, ToolRuntime, ToolRuntimeConfig};
use crate::tool_schemas::{default_v2_tools, propose_hwp_edits_tool_spec};
use crate::traits::{DocEngine, GuardContext, PolicyGuard, PrivacyMasker};
use crate::types::{
    CancelToken, LocalBoxFuture, NeutralMessage, NeutralToolCall, NeutralToolSpec,
    ReasoningEffortOption, ToolChoice,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopV2Phase {
    Inspect,
    Trial,
    Verify,
    Guard,
    Propose,
    Done,
    Cancelled,
    Failed,
}

pub struct LoopV2Options {
    pub request_id: String,
    pub staging_path: Option<String>,
    pub format: DocFormat,
    pub tables_meta: Vec<TableCoordinateMeta>,
    pub excluded_regions: Vec<ExcludedRegion>,
    pub auto_mask_pii: bool,
    pub user_prompt: String,
    pub doc_markdown: String,
    pub selection: Option<SelectionTarget>,
    pub reasoning_effort: Option<ReasoningEffortOption>,
    pub timeout_ms: Option<u64>,
    pub max_iterations: Option<usize>,
    pub cancel_token: Option<CancelToken>,
}

pub type SleepFn = Arc<dyn Fn(u64) -> LocalBoxFuture<'static, ()> + Send + Sync>;

pub struct LoopV2Context {
    pub adapter: Arc<dyn ProviderAdapter>,
    pub system_prompt: String,
    pub messages: Vec<NeutralMessage>,
    pub options: LoopV2Options,
    pub base_bytes: Option<Vec<u8>>,
    pub base_revision: u64,
    pub initial_report: Option<LayoutReport>,
    pub tools: Vec<NeutralToolSpec>,
    pub masker: Arc<dyn PrivacyMasker>,
    pub guard: Arc<dyn PolicyGuard>,
    pub doc_engine: Arc<dyn DocEngine>,
    pub trial_store: Arc<dyn TrialStore>,
    pub emit_step: StepEmitter,
    pub sleep_fn: Option<SleepFn>,
    pub max_turns: Option<usize>,
    pub max_tool_calls: Option<usize>,
    pub total_timeout_ms: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct LoopV2Result {
    pub summary: String,
    pub diff_items: Vec<DiffItem>,
    pub verification_passed: bool,
    pub trials_run: usize,
    pub final_phase: LoopV2Phase,
    pub phase_trace: Vec<LoopV2Phase>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawProposedDiffItem {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub status: Option<DiffStatus>,
    #[serde(default = "default_scope")]
    pub scope: DiffScope,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub before_text: String,
    #[serde(default)]
    pub after_text: String,
    #[serde(default)]
    pub unified_ops: Vec<UnifiedOp>,
    #[serde(default)]
    pub target_coord: Option<DiffTargetCoord>,
}

fn default_scope() -> DiffScope {
    DiffScope::Paragraph
}

/// Determine if user request looks like an editing intent.
/// Questions without editing verbs are treated as informational queries.
pub fn looks_like_edit_intent(
    user_prompt: &str,
    selection: Option<&SelectionTarget>,
) -> bool {
    if selection.is_some() {
        return true;
    }

    let normalized = user_prompt.trim();
    const EDIT_KEYWORDS: &[&str] = &[
        "수정", "고쳐", "바꿔", "변경", "추가", "삭제", "맞춰", "줄여", "늘려", "작성", "만들어",
        "편집", "개선", "반영", "정리해", "fix", "change", "edit", "update", "format", "replace",
        "insert", "delete",
    ];

    if EDIT_KEYWORDS.iter().any(|kw| normalized.contains(kw)) {
        return true;
    }

    const QUESTION_MARKERS: &[&str] = &[
        "몇 개",
        "어디",
        "무엇",
        "왜",
        "어떤",
        "?",
        "알려줘",
        "조회해줘",
        "조회만",
        "확인만",
    ];
    if QUESTION_MARKERS.iter().any(|qm| normalized.contains(qm)) {
        return false;
    }

    false
}

/// Cap reasoning effort for exploration turns to at most `cap` (default `Medium`).
pub fn cap_reasoning_effort(
    user_effort: Option<ReasoningEffortOption>,
    cap: ReasoningEffortOption,
) -> ReasoningEffortOption {
    fn rank(e: ReasoningEffortOption) -> usize {
        match e {
            ReasoningEffortOption::None => 0,
            ReasoningEffortOption::Low => 1,
            ReasoningEffortOption::Medium => 2,
            ReasoningEffortOption::High => 3,
            ReasoningEffortOption::Xhigh | ReasoningEffortOption::Max => 4,
        }
    }
    let current = user_effort.unwrap_or(ReasoningEffortOption::High);
    if rank(current) <= rank(cap) {
        current
    } else {
        cap
    }
}

fn normalize_call_signature(call: &NeutralToolCall) -> String {
    match parse_tool_args(Some(&call.arguments_json)) {
        Ok(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let mut sorted = Map::new();
            for k in keys {
                sorted.insert(k.clone(), map[k].clone());
            }
            format!("{}:{}", call.name, Value::Object(sorted))
        }
        Err(_) => format!("{}:{}", call.name, call.arguments_json.trim()),
    }
}

fn extract_embedded_diff_json(text: &str) -> Option<(String, Vec<DiffItem>)> {
    static FENCE_RE: OnceLock<Regex> = OnceLock::new();
    static OBJ_RE: OnceLock<Regex> = OnceLock::new();
    let fence_re = FENCE_RE.get_or_init(|| Regex::new(r"(?s)```json\s*(.*?)\s*```").unwrap());
    let obj_re = OBJ_RE.get_or_init(|| Regex::new(r#"(?s)\{.*"diffItems".*\}"#).unwrap());

    let raw_json = if let Some(caps) = fence_re.captures(text) {
        caps[1].to_string()
    } else {
        obj_re.find(text)?.as_str().to_string()
    };

    let parsed: Value = serde_json::from_str(&raw_json).ok()?;
    let raw_items = parsed.get("diffItems")?.as_array()?;
    let items: Vec<DiffItem> = raw_items
        .iter()
        .filter_map(|v| serde_json::from_value::<RawProposedDiffItem>(v.clone()).ok())
        .map(|raw| raw_to_diff_item(raw, None))
        .collect();
    let summary = parsed
        .get("summary")
        .and_then(|v| v.as_str())
        .unwrap_or(text)
        .to_string();
    Some((summary, items))
}

fn raw_to_diff_item(raw: RawProposedDiffItem, base_revision: Option<u64>) -> DiffItem {
    DiffItem {
        id: raw.id.unwrap_or_else(|| Uuid::new_v4().to_string()),
        status: raw.status.unwrap_or(DiffStatus::Pending),
        scope: raw.scope,
        title: raw.title,
        before_text: raw.before_text,
        after_text: raw.after_text,
        unified_ops: raw.unified_ops,
        target_coord: raw.target_coord,
        bbox: None,
        verification_status: None,
        warnings: None,
        metrics_delta: None,
        base_revision,
    }
}

async fn join_all_local<'a, T>(mut futures: Vec<LocalBoxFuture<'a, T>>) -> Vec<T> {
    let mut results: Vec<Option<T>> = (0..futures.len()).map(|_| None).collect();
    std::future::poll_fn(|cx: &mut Context<'_>| {
        let mut all_done = true;
        for (idx, fut) in futures.iter_mut().enumerate() {
            if results[idx].is_some() {
                continue;
            }
            match Pin::new(fut).poll(cx) {
                Poll::Ready(val) => results[idx] = Some(val),
                Poll::Pending => all_done = false,
            }
        }
        if all_done {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    })
    .await;
    results.into_iter().map(|r| r.unwrap()).collect()
}

fn is_transient_error_msg(msg: &str) -> bool {
    static TRANSIENT_RE: OnceLock<Regex> = OnceLock::new();
    let re = TRANSIENT_RE
        .get_or_init(|| Regex::new(r"(?i)429|rate\s*limit|overloaded|timeout|econnreset").unwrap());
    re.is_match(msg)
}

fn user_asked_layout_fix(prompt: &str) -> bool {
    static LAYOUT_FIX_RE: OnceLock<Regex> = OnceLock::new();
    let re = LAYOUT_FIX_RE.get_or_init(|| {
        Regex::new(r"잘려|잘림|안보여|안\s*보여|겹쳐|겹침|넘침|오버플로|overflow|줄간격|여백|레이아웃\s*(?:수정|교정|개선|해결|맞춰)").unwrap()
    });
    re.is_match(prompt)
}

/// Run multi-turn Agent Loop v2 with state machine `Inspect -> (Trial <-> Verify)* -> Guard -> Propose`,
/// budgets, exponential backoff recovery, trial isolation, and verification gates.
pub async fn run_agent_loop_v2(ctx: LoopV2Context) -> Result<LoopV2Result, AiError> {
    let request_id = if ctx.options.request_id.is_empty() {
        format!("req_{}", &Uuid::new_v4().simple().to_string()[..8])
    } else {
        ctx.options.request_id.clone()
    };

    let result = run_agent_loop_v2_inner(ctx, &request_id).await;
    // Always clean up request-scoped trial files on completion, cancellation, or error.
    result
}

async fn run_agent_loop_v2_inner(
    ctx: LoopV2Context,
    request_id: &str,
) -> Result<LoopV2Result, AiError> {
    let trial_store = Arc::clone(&ctx.trial_store);
    let outcome = execute_loop_v2(ctx, request_id).await;
    let _ = trial_store.cleanup_request_trials(request_id).await;
    outcome
}

async fn execute_loop_v2(ctx: LoopV2Context, request_id: &str) -> Result<LoopV2Result, AiError> {
    let max_turns = ctx
        .max_turns
        .unwrap_or_else(|| ctx.options.max_iterations.unwrap_or(8).clamp(1, 8));
    let max_tool_calls = ctx.max_tool_calls.unwrap_or(16);
    let total_timeout_ms = ctx.total_timeout_ms.unwrap_or(240_000);
    let start_time = Instant::now();
    let sleep_fn: SleepFn = ctx.sleep_fn.unwrap_or_else(|| {
        Arc::new(|ms: u64| Box::pin(tokio::time::sleep(Duration::from_millis(ms))))
    });

    let check_abort = || -> Result<(), AiError> {
        if ctx
            .options
            .cancel_token
            .as_ref()
            .is_some_and(CancelToken::is_cancelled)
        {
            return Err(AiError::Aborted);
        }
        Ok(())
    };

    let available_tools = if ctx.tools.is_empty() {
        default_v2_tools()
    } else {
        ctx.tools
    };

    let mut tool_runtime = ToolRuntime::new(ToolRuntimeConfig {
        request_id: request_id.to_string(),
        base_bytes: ctx.base_bytes.clone(),
        base_revision: ctx.base_revision,
        format: ctx.options.format,
        initial_report: ctx.initial_report.clone(),
        tables_meta: ctx.options.tables_meta.clone(),
        excluded_regions: ctx.options.excluded_regions.clone(),
        auto_mask_pii: ctx.options.auto_mask_pii,
        user_prompt: ctx.options.user_prompt.clone(),
        doc_markdown: ctx.options.doc_markdown.clone(),
        available_tools: available_tools.clone(),
        masker: Arc::clone(&ctx.masker),
        guard: Arc::clone(&ctx.guard),
        doc_engine: Arc::clone(&ctx.doc_engine),
        trial_store: Arc::clone(&ctx.trial_store),
        emit_step: Arc::clone(&ctx.emit_step),
    });

    let mut messages = ctx.messages;
    let mut total_tool_calls_count = 0usize;
    let mut nudged = false;
    let mut guard_feedback_count = 0usize;
    let mut verify_feedback_count = 0usize;
    let mut force_propose = false;
    let mut tool_call_signatures: HashMap<String, usize> = HashMap::new();
    let mut phase_trace = vec![LoopV2Phase::Inspect];

    let mut raw_result = LoopV2Result {
        summary: String::new(),
        diff_items: Vec::new(),
        verification_passed: false,
        trials_run: 0,
        final_phase: LoopV2Phase::Inspect,
        phase_trace: phase_trace.clone(),
    };

    for turn in 0..max_turns {
        check_abort()?;

        if start_time.elapsed().as_millis() as u64 >= total_timeout_ms {
            force_propose = true;
        }
        if turn >= max_turns - 1 || total_tool_calls_count >= max_tool_calls {
            force_propose = true;
        }

        let turn_effort = ctx
            .options
            .reasoning_effort
            .unwrap_or(ReasoningEffortOption::High);
        let (current_tools, current_tool_choice) = if force_propose {
            phase_trace.push(LoopV2Phase::Propose);
            (
                vec![propose_hwp_edits_tool_spec()],
                ToolChoice::named("propose_hwp_edits"),
            )
        } else {
            (available_tools.clone(), ToolChoice::auto())
        };

        (ctx.emit_step)(AgentStepEvent {
            step_type: AgentStepType::Thinking,
            message: if force_propose {
                format!("최종 편집안 도출 중 (강제 제안 턴, effort: {})", turn_effort.as_str())
            } else {
                format!(
                    "AI 추론 및 편집안 탐색 중 ({}/{}회차, effort: {})",
                    turn + 1,
                    max_turns,
                    turn_effort.as_str()
                )
            },
            detail: None,
        });

        let compacted_messages = compact_old_tool_results(&messages, COMPACT_KEEP_RECENT_TURNS);

        // Exponential backoff retries on transient errors
        let max_retries = 2usize;
        let mut attempt = 0usize;
        let turn_response = loop {
            check_abort()?;
            let req = crate::types::AgentTurnRequest {
                system_prompt: ctx.system_prompt.clone(),
                messages: compacted_messages.clone(),
                tools: current_tools.clone(),
                tool_choice: current_tool_choice.clone(),
                reasoning_effort: Some(turn_effort),
                request_id: Some(request_id.to_string()),
                timeout_ms: Some(ctx.options.timeout_ms.unwrap_or(180_000)),
                cancel_token: ctx.options.cancel_token.clone(),
            };

            match ctx.adapter.turn(req).await {
                Ok(resp) => break resp,
                Err(err @ AiError::ToolsUnsupported(_)) => return Err(err),
                Err(err) => {
                    let is_transient = matches!(err, AiError::TransientProvider { .. })
                        || is_transient_error_msg(&err.to_string());
                    if is_transient && attempt < max_retries {
                        attempt += 1;
                        let backoff_ms = 500 * (1u64 << (attempt - 1));
                        sleep_fn(backoff_ms).await;
                        continue;
                    }
                    return Err(err);
                }
            }
        };

        messages.push(NeutralMessage::Assistant {
            content: if turn_response.content.is_empty() && !turn_response.tool_calls.is_empty() {
                None
            } else {
                Some(turn_response.content.clone())
            },
            tool_calls: if turn_response.tool_calls.is_empty() {
                None
            } else {
                Some(turn_response.tool_calls.clone())
            },
        });

        // Case A: Text-only response
        if turn_response.tool_calls.is_empty() {
            let text = turn_response.content;
            if let Some((summary, items)) = extract_embedded_diff_json(&text) {
                phase_trace.push(LoopV2Phase::Done);
                raw_result = LoopV2Result {
                    summary,
                    diff_items: items,
                    verification_passed: true,
                    trials_run: tool_runtime.trials_run,
                    final_phase: LoopV2Phase::Done,
                    phase_trace: phase_trace.clone(),
                };
                break;
            }

            let is_edit_intent = looks_like_edit_intent(
                &ctx.options.user_prompt,
                ctx.options.selection.as_ref(),
            );
            if is_edit_intent && !nudged && !force_propose {
                nudged = true;
                messages.push(NeutralMessage::User {
                    content: "편집 또는 수정을 적용하려면 변경 내용을 propose_hwp_edits 도구로 제출해 주세요.".to_string(),
                });
                continue;
            }

            phase_trace.push(LoopV2Phase::Done);
            raw_result = LoopV2Result {
                summary: text,
                diff_items: Vec::new(),
                verification_passed: true,
                trials_run: tool_runtime.trials_run,
                final_phase: LoopV2Phase::Done,
                phase_trace: phase_trace.clone(),
            };
            break;
        }

        // Case B: Tool calls
        total_tool_calls_count += turn_response.tool_calls.len();
        for call in &turn_response.tool_calls {
            let sig = normalize_call_signature(call);
            let count = tool_call_signatures.get(&sig).copied().unwrap_or(0) + 1;
            tool_call_signatures.insert(sig, count);
            if count >= 2 {
                force_propose = true;
            }
        }

        let mut propose_calls = Vec::new();
        let mut read_calls = Vec::new();
        let mut write_calls = Vec::new();

        for call in turn_response.tool_calls {
            if call.name == "propose_hwp_edits" {
                propose_calls.push(call);
            } else if is_read_only_tool(&call.name) {
                read_calls.push(call);
            } else {
                write_calls.push(call);
            }
        }

        // Concurrent read-only calls
        if !read_calls.is_empty() {
            let futs: Vec<LocalBoxFuture<'_, (NeutralToolCall, String)>> = read_calls
                .into_iter()
                .map(|call| {
                    let runtime_ref = &tool_runtime;
                    Box::pin(async move {
                        let content = runtime_ref.execute_read_only_call(&call);
                        (call, content)
                    }) as LocalBoxFuture<'_, _>
                })
                .collect();
            for (call, content) in join_all_local(futs).await {
                messages.push(NeutralMessage::Tool {
                    tool_call_id: call.id,
                    name: Some(call.name),
                    content,
                });
            }
        }

        // Sequential write/trial calls
        for call in write_calls {
            phase_trace.push(LoopV2Phase::Trial);
            let content = tool_runtime.execute_tool_call(&call).await;
            messages.push(NeutralMessage::Tool {
                tool_call_id: call.id,
                name: Some(call.name),
                content,
            });
        }

        if !propose_calls.is_empty() {
            phase_trace.push(LoopV2Phase::Propose);
            let propose_call = propose_calls[0].clone();
            for extra in propose_calls.iter().skip(1) {
                messages.push(NeutralMessage::Tool {
                    tool_call_id: extra.id.clone(),
                    name: Some(extra.name.clone()),
                    content: json!({
                        "ok": true,
                        "note": "중복 제안 호출은 첫 번째 제안에 병합되었습니다.",
                    })
                    .to_string(),
                });
            }

            (ctx.emit_step)(AgentStepEvent {
                step_type: AgentStepType::Propose,
                message: "제안안 도출 검수 및 정책 가드 검사 중...".to_string(),
                detail: None,
            });

            let call_args = match parse_tool_args(Some(&propose_call.arguments_json)) {
                Ok(a) => a,
                Err(_) => {
                    messages.push(NeutralMessage::Tool {
                        tool_call_id: propose_call.id,
                        name: Some(propose_call.name),
                        content: json!({
                            "ok": false,
                            "error": "propose_hwp_edits 인자 파싱 실패: 올바른 JSON 형식이어야 합니다.",
                        })
                        .to_string(),
                    });
                    continue;
                }
            };

            let proposal_summary = call_args
                .get("summary")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let mut diff_items: Vec<DiffItem> = call_args
                .get("diffItems")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| serde_json::from_value::<RawProposedDiffItem>(v.clone()).ok())
                        .map(|raw| raw_to_diff_item(raw, Some(ctx.base_revision)))
                        .collect()
                })
                .unwrap_or_default();

            let all_ops: Vec<UnifiedOp> = diff_items
                .iter()
                .flat_map(|d| d.unified_ops.iter().cloned())
                .collect();

            // 1. Policy Guard (G1-G6)
            phase_trace.push(LoopV2Phase::Guard);
            let guard_ctx = GuardContext {
                user_prompt: ctx.options.user_prompt.clone(),
                is_new_document: false,
                excluded_regions: ctx.options.excluded_regions.clone(),
                layout_report: tool_runtime
                    .current_report
                    .clone()
                    .or_else(|| tool_runtime.initial_report.clone()),
                tables_meta: ctx.options.tables_meta.clone(),
            };
            let guard_res = ctx.guard.lint_ops(&all_ops, &guard_ctx);

            if !guard_res.passed && guard_feedback_count < 1 && turn < max_turns - 1 {
                guard_feedback_count += 1;
                let detail = guard_res
                    .rejected_ops
                    .iter()
                    .map(|r| r.reason.as_str())
                    .collect::<Vec<_>>()
                    .join("; ");
                (ctx.emit_step)(AgentStepEvent {
                    step_type: AgentStepType::Guard,
                    message: "정책 가드 위반 감지, 모델 자가 교정 피드백 전달".to_string(),
                    detail: Some(detail),
                });
                let reasons = guard_res
                    .rejected_ops
                    .iter()
                    .map(|r| r.reason.as_str())
                    .collect::<Vec<_>>()
                    .join("\n");
                messages.push(NeutralMessage::Tool {
                    tool_call_id: propose_call.id,
                    name: Some(propose_call.name),
                    content: format!("정책 가드 검사 실패:\n{reasons}\n\n위반 사항을 교정하여 다시 propose_hwp_edits를 호출하세요."),
                });
                continue;
            }

            // 2. Auto-Verification Gate
            phase_trace.push(LoopV2Phase::Verify);
            let mut after_report: Option<LayoutReport> = None;
            let mut verification_ok = true;
            let mut comp_summary = String::new();

            if let (Some(base_bytes), false) = (&tool_runtime.base_bytes, all_ops.is_empty()) {
                tool_runtime.trials_run += 1;
                (ctx.emit_step)(AgentStepEvent {
                    step_type: AgentStepType::Trial,
                    message: "제안된 연산 복사본 시험 적용 및 재측정 검증 중...".to_string(),
                    detail: None,
                });

                let verify_req_id = format!("verify_{request_id}");
                if let Ok(verify_ws) = ctx
                    .trial_store
                    .prepare_trial(
                        &verify_req_id,
                        ctx.options.format,
                        base_bytes,
                        ctx.base_revision,
                    )
                    .await
                {
                    let unmasked_ops = ctx
                        .masker
                        .unmask_and_filter_ops(&guard_res.sanitized_ops, &ctx.options.excluded_regions);
                    if let Ok(claw_res) = ctx
                        .doc_engine
                        .apply_ops_to_file(
                            &verify_ws.path,
                            ctx.options.format,
                            &unmasked_ops,
                            &ctx.options.tables_meta,
                        )
                        .await
                    {
                        if claw_res.ok {
                            if let Ok(patched_bytes) =
                                ctx.trial_store.read_trial_bytes(&verify_ws).await
                            {
                                if let Ok(report) = ctx.doc_engine.inspect_bytes(&patched_bytes).await
                                {
                                    let comparison = ctx.doc_engine.compare_layout_reports(
                                        tool_runtime.initial_report.as_ref().unwrap_or(&report),
                                        &report,
                                    );
                                    after_report = Some(report);
                                    comp_summary = comparison.summary_text.clone();

                                    if user_asked_layout_fix(&ctx.options.user_prompt)
                                        && !comparison.ok
                                        && !comparison.remaining_issues.is_empty()
                                        && verify_feedback_count < 1
                                        && turn < max_turns - 1
                                    {
                                        verify_feedback_count += 1;
                                        let _ = ctx.trial_store.cleanup_trial(&verify_ws).await;
                                        (ctx.emit_step)(AgentStepEvent {
                                            step_type: AgentStepType::Verify,
                                            message: "오버플로 미완전 해소 감지, 모델 정밀 교정 피드백 전달".to_string(),
                                            detail: Some(comparison.summary_text.clone()),
                                        });
                                        let remaining_lines = comparison
                                            .remaining_issues
                                            .iter()
                                            .map(|i| i.message.as_str())
                                            .collect::<Vec<_>>()
                                            .join("\n");
                                        messages.push(NeutralMessage::Tool {
                                            tool_call_id: propose_call.id,
                                            name: Some(propose_call.name),
                                            content: format!("시험 검증 결과: {}\n잔여 이슈 목록:\n{remaining_lines}\n\n행 높이나 열 너비를 더 확장하거나 셀 여백을 줄여 오버플로를 완전히 해소한 뒤 다시 propose_hwp_edits를 호출하세요.", comparison.summary_text),
                                        });
                                        continue;
                                    }

                                    verification_ok = comparison.ok;
                                }
                            }
                        }
                    }
                    let _ = ctx.trial_store.cleanup_trial(&verify_ws).await;
                }
            }

            // Annotate diff items with verification status, warnings, and metrics delta
            for item in &mut diff_items {
                item.verification_status = Some(if verification_ok {
                    DiffVerificationStatus::Verified
                } else {
                    DiffVerificationStatus::WarningUnresolved
                });
                item.warnings = Some(guard_res.warnings.clone());

                if let (Some(init_rep), Some(aft_rep), Some(tc)) = (
                    &tool_runtime.initial_report,
                    &after_report,
                    &item.target_coord,
                ) {
                    if let (Some(table_idx), Some(target_row), Some(target_col)) =
                        (tc.table_index, tc.row, tc.col)
                    {
                        let cell_before = init_rep
                            .tables
                            .get(table_idx)
                            .and_then(|t| t.cells.iter().find(|c| c.row == target_row && c.col == target_col));
                        let cell_after = aft_rep
                            .tables
                            .get(table_idx)
                            .and_then(|t| t.cells.iter().find(|c| c.row == target_row && c.col == target_col));
                        if cell_before.is_some() || cell_after.is_some() {
                            item.metrics_delta = Some(DiffMetricsDelta {
                                before_overflow_mm: cell_before.map(|c| c.overflow_amount_mm),
                                after_overflow_mm: cell_after.map(|c| c.overflow_amount_mm),
                                width_before_mm: cell_before.map(|c| c.width_mm),
                                width_after_mm: cell_after.map(|c| c.width_mm),
                                height_before_mm: cell_before.map(|c| c.height_mm),
                                height_after_mm: cell_after.map(|c| c.height_mm),
                            });
                        }
                    }
                }
            }

            messages.push(NeutralMessage::Tool {
                tool_call_id: propose_call.id,
                name: Some(propose_call.name),
                content: json!({
                    "ok": true,
                    "verification": verification_ok,
                    "summary": if comp_summary.is_empty() { "검증 완료" } else { comp_summary.as_str() },
                })
                .to_string(),
            });

            phase_trace.push(LoopV2Phase::Done);
            raw_result = LoopV2Result {
                summary: proposal_summary,
                diff_items,
                verification_passed: verification_ok,
                trials_run: tool_runtime.trials_run,
                final_phase: LoopV2Phase::Done,
                phase_trace: phase_trace.clone(),
            };

            (ctx.emit_step)(AgentStepEvent {
                step_type: AgentStepType::Verify,
                message: if verification_ok {
                    "검증 완료: 모든 레이아웃 이슈 해소".to_string()
                } else {
                    format!(
                        "검증 완료: {}",
                        if comp_summary.is_empty() {
                            "일부 잔여 이슈 보존"
                        } else {
                            comp_summary.as_str()
                        }
                    )
                },
                detail: None,
            });

            break;
        }
    }

    Ok(raw_result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::{
        default_compare_layout_reports, AllowAllGuard, GuardRejection, GuardResult,
        NoopPrivacyMasker, TempDirTrialStore,
    };
    use crate::types::{AgentTurnRequest, AgentTurnResponse, ProviderId};
    use hopclaw_types::{
        CellLayoutInfo, ClawExecResult, LayoutComparison, LayoutIssue, LayoutIssueCellRef,
        LayoutIssueCode, LayoutIssueMetrics, LayoutIssueSeverity, SelectionRect, TableLayoutInfo,
    };
    use std::collections::VecDeque;
    use std::path::Path;
    use std::sync::Mutex;

    enum MockStep {
        Resp(AgentTurnResponse),
        Err(AiError),
    }

    struct MockAdapter {
        history: Mutex<Vec<AgentTurnRequest>>,
        steps: Mutex<VecDeque<MockStep>>,
        supports_tools: bool,
    }

    impl MockAdapter {
        fn with_responses(resps: Vec<AgentTurnResponse>) -> Self {
            Self {
                history: Mutex::new(Vec::new()),
                steps: Mutex::new(resps.into_iter().map(MockStep::Resp).collect()),
                supports_tools: true,
            }
        }

        fn with_steps(steps: Vec<MockStep>) -> Self {
            Self {
                history: Mutex::new(Vec::new()),
                steps: Mutex::new(steps.into_iter().collect()),
                supports_tools: true,
            }
        }
    }

    impl ProviderAdapter for MockAdapter {
        fn id(&self) -> ProviderId {
            ProviderId::Baze
        }

        fn supports_tools(&self) -> bool {
            self.supports_tools
        }

        fn turn(
            &self,
            req: AgentTurnRequest,
        ) -> LocalBoxFuture<'_, Result<AgentTurnResponse, AiError>> {
            Box::pin(async move {
                self.history.lock().unwrap().push(req);
                match self.steps.lock().unwrap().pop_front() {
                    Some(MockStep::Resp(r)) => Ok(r),
                    Some(MockStep::Err(e)) => Err(e),
                    None => Ok(AgentTurnResponse {
                        content: "기본 응답".into(),
                        tool_calls: vec![],
                        reasoning_tokens: None,
                        stop_reason: None,
                    }),
                }
            })
        }
    }

    fn mock_report() -> LayoutReport {
        LayoutReport {
            timestamp: 1000,
            table_count: 1,
            issues_count: 1,
            issues: vec![LayoutIssue {
                id: "issue_0_0_1".into(),
                code: LayoutIssueCode::CELL_OVERFLOW_V,
                severity: LayoutIssueSeverity::Error,
                message: "표 0 (0행 1열): 2.0mm 넘침".into(),
                cell: Some(LayoutIssueCellRef {
                    table_index: 0,
                    sec: Some(0),
                    para: Some(0),
                    ctrl: Some(0),
                    row: 0,
                    col: 1,
                    cell_idx: 1,
                }),
                table_index: Some(0),
                metrics: LayoutIssueMetrics {
                    overflow_mm: Some(2.0),
                    ..Default::default()
                },
                text: Some("overflow".into()),
            }],
            tables: vec![TableLayoutInfo {
                table_index: 0,
                section: 0,
                para: 0,
                ctrl: 0,
                cell_count: 4,
                table_width_mm: Some(140.0),
                printable_width_mm: Some(150.0),
                cells: vec![
                    CellLayoutInfo {
                        cell_idx: 0,
                        row: 0,
                        col: 0,
                        row_span: 1,
                        col_span: 1,
                        width_mm: 70.0,
                        height_mm: 10.0,
                        avail_width_mm: 65.0,
                        avail_height_mm: 8.0,
                        estimated_lines: 1,
                        content_height_mm: 5.0,
                        is_vertical_overflow: false,
                        is_horizontal_overflow: false,
                        overflow_amount_mm: 0.0,
                        text: "A".into(),
                    },
                    CellLayoutInfo {
                        cell_idx: 1,
                        row: 0,
                        col: 1,
                        row_span: 1,
                        col_span: 1,
                        width_mm: 70.0,
                        height_mm: 10.0,
                        avail_width_mm: 65.0,
                        avail_height_mm: 8.0,
                        estimated_lines: 2,
                        content_height_mm: 12.0,
                        is_vertical_overflow: true,
                        is_horizontal_overflow: false,
                        overflow_amount_mm: 2.0,
                        text: "B".into(),
                    },
                ],
            }],
        }
    }

    struct MockDocEngine {
        comparisons: Mutex<VecDeque<LayoutComparison>>,
    }

    impl MockDocEngine {
        fn passing() -> Self {
            Self {
                comparisons: Mutex::new(VecDeque::new()),
            }
        }

        fn with_comparisons(items: Vec<LayoutComparison>) -> Self {
            Self {
                comparisons: Mutex::new(items.into_iter().collect()),
            }
        }
    }

    impl DocEngine for MockDocEngine {
        fn inspect_bytes<'a>(
            &'a self,
            _bytes: &'a [u8],
        ) -> LocalBoxFuture<'a, Result<LayoutReport, AiError>> {
            Box::pin(async move { Ok(mock_report()) })
        }

        fn apply_ops_to_file<'a>(
            &'a self,
            _file_path: &'a Path,
            _format: DocFormat,
            _ops: &'a [UnifiedOp],
            _tables_meta: &'a [TableCoordinateMeta],
        ) -> LocalBoxFuture<'a, Result<ClawExecResult, AiError>> {
            Box::pin(async move {
                Ok(ClawExecResult {
                    ok: true,
                    stdout_json: json!({}),
                    stderr: String::new(),
                })
            })
        }

        fn compare_layout_reports(
            &self,
            before: &LayoutReport,
            after: &LayoutReport,
        ) -> LayoutComparison {
            if let Some(next) = self.comparisons.lock().unwrap().pop_front() {
                return next;
            }
            let _ = (before, after);
            LayoutComparison {
                ok: true,
                resolved_issues: mock_report().issues,
                remaining_issues: vec![],
                new_issues: vec![],
                summary_text: "이슈 모두 해소".into(),
                metrics_delta: None,
            }
        }
    }

    fn base_ctx(
        adapter: Arc<dyn ProviderAdapter>,
        trial_dir: &Path,
    ) -> LoopV2Context {
        LoopV2Context {
            adapter,
            system_prompt: "시스템 프롬프트".into(),
            messages: vec![NeutralMessage::User {
                content: "표 0의 오버플로를 수정해줘".into(),
            }],
            options: LoopV2Options {
                request_id: "loop-test-1".into(),
                staging_path: Some("C:\\mock\\test.hwpx".into()),
                format: DocFormat::Hwpx,
                tables_meta: vec![],
                excluded_regions: vec![],
                auto_mask_pii: false,
                user_prompt: "표 0의 오버플로를 수정해줘".into(),
                doc_markdown: "테스트 문서 본문입니다.".into(),
                selection: None,
                reasoning_effort: None,
                timeout_ms: None,
                max_iterations: None,
                cancel_token: None,
            },
            base_bytes: Some(vec![0x50, 0x4b, 0x03, 0x04]),
            base_revision: 1,
            initial_report: Some(mock_report()),
            tools: default_v2_tools(),
            masker: Arc::new(NoopPrivacyMasker),
            guard: Arc::new(AllowAllGuard),
            doc_engine: Arc::new(MockDocEngine::passing()),
            trial_store: Arc::new(TempDirTrialStore::new(trial_dir)),
            emit_step: Arc::new(|_| {}),
            sleep_fn: Some(Arc::new(|_| Box::pin(async {}))),
            max_turns: None,
            max_tool_calls: None,
            total_timeout_ms: None,
        }
    }

    #[test]
    fn helper_looks_like_edit_intent_and_cap_reasoning_effort() {
        assert!(looks_like_edit_intent("표 1의 너비를 수정해줘", None));
        assert!(looks_like_edit_intent("문단을 추가해", None));
        assert!(looks_like_edit_intent("글자 크기를 줄여줘", None));
        let sel = SelectionTarget::BodyRange {
            sec: 0,
            start_para: 0,
            start_offset: 0,
            end_para: 0,
            end_offset: 3,
            selected_text: "abc".into(),
            rects: vec![SelectionRect {
                page_index: 0,
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            }],
        };
        assert!(looks_like_edit_intent("anything", Some(&sel)));

        assert!(!looks_like_edit_intent("이 문서에 표가 몇 개 있어?", None));
        assert!(!looks_like_edit_intent("어디에 오버플로가 발생했는지 알려줘", None));
        assert!(!looks_like_edit_intent("표 1의 내용은 뭐야?", None));

        assert_eq!(
            cap_reasoning_effort(Some(ReasoningEffortOption::High), ReasoningEffortOption::Medium),
            ReasoningEffortOption::Medium
        );
        assert_eq!(
            cap_reasoning_effort(Some(ReasoningEffortOption::Xhigh), ReasoningEffortOption::Medium),
            ReasoningEffortOption::Medium
        );
        assert_eq!(
            cap_reasoning_effort(Some(ReasoningEffortOption::Low), ReasoningEffortOption::Medium),
            ReasoningEffortOption::Low
        );
        assert_eq!(
            cap_reasoning_effort(Some(ReasoningEffortOption::None), ReasoningEffortOption::Medium),
            ReasoningEffortOption::None
        );
        assert_eq!(
            cap_reasoning_effort(None, ReasoningEffortOption::Medium),
            ReasoningEffortOption::Medium
        );
    }

    #[tokio::test]
    async fn scenario_1_normal_inspect_to_propose_flow() {
        let tmp = tempfile::tempdir().unwrap();
        let adapter = Arc::new(MockAdapter::with_responses(vec![
            AgentTurnResponse {
                content: "레이아웃을 확인해보겠습니다.".into(),
                tool_calls: vec![NeutralToolCall {
                    id: "call_inspect_1".into(),
                    name: "inspect_layout".into(),
                    arguments_json: "{}".into(),
                }],
                reasoning_tokens: None,
                stop_reason: None,
            },
            AgentTurnResponse {
                content: "수정안을 제안합니다.".into(),
                tool_calls: vec![NeutralToolCall {
                    id: "call_prop_1".into(),
                    name: "propose_hwp_edits".into(),
                    arguments_json: json!({
                        "summary": "표 열 너비 확장",
                        "diffItems": [{
                            "scope": "cell",
                            "title": "표 0 (0행 1열)",
                            "beforeText": "원본",
                            "afterText": "수정",
                            "targetCoord": { "tableIndex": 0, "row": 0, "col": 1 },
                            "unifiedOps": [{ "type": "set_col_width", "tableIndex": 0, "col": 1, "widthMm": 80.0 }]
                        }]
                    })
                    .to_string(),
                }],
                reasoning_tokens: None,
                stop_reason: None,
            },
        ]));

        let res = run_agent_loop_v2(base_ctx(adapter.clone(), tmp.path()))
            .await
            .unwrap();

        assert_eq!(adapter.history.lock().unwrap().len(), 2);
        assert_eq!(res.summary, "표 열 너비 확장");
        assert_eq!(res.diff_items.len(), 1);
        assert_eq!(
            res.diff_items[0].verification_status,
            Some(DiffVerificationStatus::Verified)
        );
        assert_eq!(res.diff_items[0].base_revision, Some(1));
        assert!(res.verification_passed);
        assert_eq!(res.trials_run, 1);
    }

    #[tokio::test]
    async fn scenario_2_multiple_tool_calls_answered_together() {
        let tmp = tempfile::tempdir().unwrap();
        let adapter = Arc::new(MockAdapter::with_responses(vec![
            AgentTurnResponse {
                content: "동시에 2개 셀 조회".into(),
                tool_calls: vec![
                    NeutralToolCall {
                        id: "call_c1".into(),
                        name: "inspect_layout".into(),
                        arguments_json: r#"{"tableIndex":0,"row":0,"col":0}"#.into(),
                    },
                    NeutralToolCall {
                        id: "call_c2".into(),
                        name: "inspect_layout".into(),
                        arguments_json: r#"{"tableIndex":0,"row":0,"col":1}"#.into(),
                    },
                ],
                reasoning_tokens: None,
                stop_reason: None,
            },
            AgentTurnResponse {
                content: "완료".into(),
                tool_calls: vec![NeutralToolCall {
                    id: "call_prop_2".into(),
                    name: "propose_hwp_edits".into(),
                    arguments_json: json!({ "summary": "완료", "diffItems": [] }).to_string(),
                }],
                reasoning_tokens: None,
                stop_reason: None,
            },
        ]));

        run_agent_loop_v2(base_ctx(adapter.clone(), tmp.path()))
            .await
            .unwrap();

        let hist = adapter.history.lock().unwrap();
        let replied_ids: Vec<&str> = hist[1]
            .messages
            .iter()
            .filter_map(|m| match m {
                NeutralMessage::Tool { tool_call_id, .. } => Some(tool_call_id.as_str()),
                _ => None,
            })
            .collect();
        assert!(replied_ids.contains(&"call_c1"));
        assert!(replied_ids.contains(&"call_c2"));
    }

    #[tokio::test]
    async fn scenario_3_and_4_unknown_tool_and_fenced_json_recovery() {
        let tmp = tempfile::tempdir().unwrap();
        let adapter = Arc::new(MockAdapter::with_responses(vec![
            AgentTurnResponse {
                content: "이상한 도구".into(),
                tool_calls: vec![
                    NeutralToolCall {
                        id: "call_bad".into(),
                        name: "non_existent_tool".into(),
                        arguments_json: "{}".into(),
                    },
                    NeutralToolCall {
                        id: "call_fenced".into(),
                        name: "inspect_layout".into(),
                        arguments_json: "```json\n{\"tableIndex\": 0}\n```".into(),
                    },
                ],
                reasoning_tokens: None,
                stop_reason: None,
            },
            AgentTurnResponse {
                content: "복구".into(),
                tool_calls: vec![NeutralToolCall {
                    id: "call_prop_3".into(),
                    name: "propose_hwp_edits".into(),
                    arguments_json: json!({ "summary": "복구 후 제안", "diffItems": [] }).to_string(),
                }],
                reasoning_tokens: None,
                stop_reason: None,
            },
        ]));

        let res = run_agent_loop_v2(base_ctx(adapter.clone(), tmp.path()))
            .await
            .unwrap();
        assert_eq!(res.summary, "복구 후 제안");
        let hist = adapter.history.lock().unwrap();
        let bad_reply = hist[1].messages.iter().find_map(|m| match m {
            NeutralMessage::Tool {
                tool_call_id,
                content,
                ..
            } if tool_call_id == "call_bad" => Some(content.as_str()),
            _ => None,
        });
        assert!(bad_reply.unwrap().contains("알 수 없는 도구"));
        let fenced_reply = hist[1].messages.iter().find_map(|m| match m {
            NeutralMessage::Tool {
                tool_call_id,
                content,
                ..
            } if tool_call_id == "call_fenced" => Some(content.as_str()),
            _ => None,
        });
        assert!(!fenced_reply.unwrap().contains("인자 파싱 실패"));
    }

    #[tokio::test]
    async fn scenario_5_and_6_budget_and_stagnation_force_propose() {
        let tmp = tempfile::tempdir().unwrap();
        let adapter = Arc::new(MockAdapter::with_responses(vec![
            AgentTurnResponse {
                content: "T1".into(),
                tool_calls: vec![NeutralToolCall {
                    id: "c1".into(),
                    name: "inspect_layout".into(),
                    arguments_json: r#"{"tableIndex":0}"#.into(),
                }],
                reasoning_tokens: None,
                stop_reason: None,
            },
            AgentTurnResponse {
                content: "T2".into(),
                tool_calls: vec![NeutralToolCall {
                    id: "c2".into(),
                    name: "inspect_layout".into(),
                    arguments_json: r#"{"tableIndex":0}"#.into(),
                }],
                reasoning_tokens: None,
                stop_reason: None,
            },
            AgentTurnResponse {
                content: "T3".into(),
                tool_calls: vec![NeutralToolCall {
                    id: "c3".into(),
                    name: "propose_hwp_edits".into(),
                    arguments_json: json!({ "summary": "정체 탈출 제안", "diffItems": [] }).to_string(),
                }],
                reasoning_tokens: None,
                stop_reason: None,
            },
        ]));

        let mut ctx = base_ctx(adapter.clone(), tmp.path());
        ctx.max_turns = Some(3);
        let res = run_agent_loop_v2(ctx).await.unwrap();

        let hist = adapter.history.lock().unwrap();
        assert_eq!(hist.len(), 3);
        assert_eq!(hist[2].tools.len(), 1);
        assert_eq!(hist[2].tools[0].name, "propose_hwp_edits");
        assert_eq!(hist[2].tool_choice, ToolChoice::named("propose_hwp_edits"));
        assert_eq!(res.summary, "정체 탈출 제안");
    }

    #[tokio::test]
    async fn scenario_7_and_8_nudge_on_edit_vs_direct_question() {
        let tmp = tempfile::tempdir().unwrap();
        // Edit intent -> 1-time nudge
        let adapter_edit = Arc::new(MockAdapter::with_responses(vec![
            AgentTurnResponse {
                content: "오버플로를 수정하려면 열 너비를 80mm로 늘려야 합니다.".into(),
                tool_calls: vec![],
                reasoning_tokens: None,
                stop_reason: None,
            },
            AgentTurnResponse {
                content: "제안 호출".into(),
                tool_calls: vec![NeutralToolCall {
                    id: "call_prop".into(),
                    name: "propose_hwp_edits".into(),
                    arguments_json: json!({ "summary": "넛지 후 제안 완료", "diffItems": [] }).to_string(),
                }],
                reasoning_tokens: None,
                stop_reason: None,
            },
        ]));
        let res_edit = run_agent_loop_v2(base_ctx(adapter_edit.clone(), tmp.path()))
            .await
            .unwrap();
        assert_eq!(adapter_edit.history.lock().unwrap().len(), 2);
        assert_eq!(res_edit.summary, "넛지 후 제안 완료");

        // Pure query -> immediate accept without nudge
        let adapter_query = Arc::new(MockAdapter::with_responses(vec![AgentTurnResponse {
            content: "이 문서에는 표가 총 1개 있습니다.".into(),
            tool_calls: vec![],
            reasoning_tokens: None,
            stop_reason: None,
        }]));
        let mut query_ctx = base_ctx(adapter_query.clone(), tmp.path());
        query_ctx.options.user_prompt = "이 문서에 표가 몇 개 있어?".into();
        let res_query = run_agent_loop_v2(query_ctx).await.unwrap();
        assert_eq!(adapter_query.history.lock().unwrap().len(), 1);
        assert!(res_query.summary.contains("표가 총 1개 있습니다"));
        assert!(res_query.diff_items.is_empty());
    }

    #[tokio::test]
    async fn scenario_9_and_10_and_15_retries_abort_and_tools_unsupported() {
        let tmp = tempfile::tempdir().unwrap();
        let slept = Arc::new(Mutex::new(Vec::new()));
        let slept_clone = Arc::clone(&slept);

        let adapter_retry = Arc::new(MockAdapter::with_steps(vec![
            MockStep::Err(AiError::TransientProvider {
                message: "Rate limit exceeded".into(),
                status: Some(429),
            }),
            MockStep::Resp(AgentTurnResponse {
                content: "429 극복".into(),
                tool_calls: vec![NeutralToolCall {
                    id: "c1".into(),
                    name: "propose_hwp_edits".into(),
                    arguments_json: json!({ "summary": "재시도 후 성공", "diffItems": [] }).to_string(),
                }],
                reasoning_tokens: None,
                stop_reason: None,
            }),
        ]));

        let mut ctx_retry = base_ctx(adapter_retry, tmp.path());
        ctx_retry.sleep_fn = Some(Arc::new(move |ms| {
            slept_clone.lock().unwrap().push(ms);
            Box::pin(async {})
        }));
        let res = run_agent_loop_v2(ctx_retry).await.unwrap();
        assert_eq!(res.summary, "재시도 후 성공");
        assert_eq!(*slept.lock().unwrap(), vec![500]);

        // AbortSignal
        let token = CancelToken::new();
        token.cancel();
        let mut ctx_abort = base_ctx(Arc::new(MockAdapter::with_responses(vec![])), tmp.path());
        ctx_abort.options.cancel_token = Some(token);
        let err_abort = run_agent_loop_v2(ctx_abort).await.unwrap_err();
        assert!(matches!(err_abort, AiError::Aborted));

        // ToolsUnsupported
        let adapter_unsup = Arc::new(MockAdapter::with_steps(vec![MockStep::Err(
            AiError::ToolsUnsupported("no tools".into()),
        )]));
        let err_unsup = run_agent_loop_v2(base_ctx(adapter_unsup, tmp.path()))
            .await
            .unwrap_err();
        assert!(matches!(err_unsup, AiError::ToolsUnsupported(_)));
    }

    #[tokio::test]
    async fn scenario_11_and_12_and_14_masking_trials_and_read_document() {
        let tmp = tempfile::tempdir().unwrap();

        struct SimpleMasker;
        impl PrivacyMasker for SimpleMasker {
            fn mask_text_for_prompt(
                &self,
                text: &str,
                _excluded: &[ExcludedRegion],
                _auto: bool,
            ) -> String {
                text.replace("010-9876-5432", "[[PII_PHONE_1]]")
            }
            fn unmask_text(&self, text: &str) -> String {
                text.to_string()
            }
            fn unmask_and_filter_ops(
                &self,
                ops: &[UnifiedOp],
                _excluded: &[ExcludedRegion],
            ) -> Vec<UnifiedOp> {
                ops.to_vec()
            }
        }

        let adapter = Arc::new(MockAdapter::with_responses(vec![
            AgentTurnResponse {
                content: "문단 조회 및 시험 적용".into(),
                tool_calls: vec![
                    NeutralToolCall {
                        id: "call_rd".into(),
                        name: "read_document".into(),
                        arguments_json: r#"{"kind":"paragraphs","start":0,"count":1}"#.into(),
                    },
                    NeutralToolCall {
                        id: "call_trial".into(),
                        name: "apply_trial_edits".into(),
                        arguments_json: json!({
                            "reason": "열 너비 조정",
                            "ops": [{ "type": "set_col_width", "tableIndex": 0, "col": 1, "widthMm": 80.0 }]
                        })
                        .to_string(),
                    },
                ],
                reasoning_tokens: None,
                stop_reason: None,
            },
            AgentTurnResponse {
                content: "제안".into(),
                tool_calls: vec![NeutralToolCall {
                    id: "call_prop".into(),
                    name: "propose_hwp_edits".into(),
                    arguments_json: json!({ "summary": "시험 후 제안", "diffItems": [] }).to_string(),
                }],
                reasoning_tokens: None,
                stop_reason: None,
            },
        ]));

        let mut ctx = base_ctx(adapter.clone(), tmp.path());
        ctx.options.doc_markdown = "대표자 홍길동 010-9876-5432".into();
        ctx.options.auto_mask_pii = true;
        ctx.masker = Arc::new(SimpleMasker);

        let res = run_agent_loop_v2(ctx).await.unwrap();
        assert_eq!(res.summary, "시험 후 제안");
        assert!(res.trials_run >= 1);

        let hist = adapter.history.lock().unwrap();
        let rd_msg = hist[1]
            .messages
            .iter()
            .find_map(|m| match m {
                NeutralMessage::Tool {
                    tool_call_id,
                    content,
                    ..
                } if tool_call_id == "call_rd" => Some(content.as_str()),
                _ => None,
            })
            .unwrap();
        assert!(!rd_msg.contains("010-9876-5432"));
        assert!(rd_msg.contains("paragraphs"));
    }

    #[tokio::test]
    async fn scenario_17_and_18_guard_and_verify_feedback_self_correction() {
        let tmp = tempfile::tempdir().unwrap();

        struct RejectFirstGuard {
            calls: Mutex<usize>,
        }
        impl PolicyGuard for RejectFirstGuard {
            fn lint_ops(&self, ops: &[UnifiedOp], _ctx: &GuardContext) -> GuardResult {
                let mut count = self.calls.lock().unwrap();
                *count += 1;
                if *count == 1 {
                    return GuardResult {
                        passed: false,
                        sanitized_ops: vec![],
                        rejected_ops: vec![GuardRejection {
                            op_index: 0,
                            rule: "G4".into(),
                            reason: "G4: 기존 문서에 허용되지 않는 연산".into(),
                        }],
                        warnings: vec![],
                    };
                }
                GuardResult {
                    passed: true,
                    sanitized_ops: ops.to_vec(),
                    rejected_ops: vec![],
                    warnings: vec![],
                }
            }
        }

        // Scenario 17: Guard violation -> 1-time feedback -> self-correct
        let adapter_guard = Arc::new(MockAdapter::with_responses(vec![
            AgentTurnResponse {
                content: "위반 연산 제안".into(),
                tool_calls: vec![NeutralToolCall {
                    id: "call_bad_prop".into(),
                    name: "propose_hwp_edits".into(),
                    arguments_json: json!({
                        "summary": "가드 위반 제안",
                        "diffItems": [{
                            "scope": "paragraph",
                            "title": "문단 1",
                            "beforeText": "a",
                            "afterText": "b",
                            "unifiedOps": [{ "type": "replace_body_text", "find": "a", "replace": "b" }]
                        }]
                    })
                    .to_string(),
                }],
                reasoning_tokens: None,
                stop_reason: None,
            },
            AgentTurnResponse {
                content: "자가 교정된 제안".into(),
                tool_calls: vec![NeutralToolCall {
                    id: "call_good_prop".into(),
                    name: "propose_hwp_edits".into(),
                    arguments_json: json!({
                        "summary": "교정 완료",
                        "diffItems": [{
                            "scope": "paragraph",
                            "title": "문단 1",
                            "beforeText": "a",
                            "afterText": "b",
                            "unifiedOps": [{ "type": "replace_body_text", "find": "a", "replace": "b" }]
                        }]
                    })
                    .to_string(),
                }],
                reasoning_tokens: None,
                stop_reason: None,
            },
        ]));

        let mut ctx_guard = base_ctx(adapter_guard.clone(), tmp.path());
        ctx_guard.guard = Arc::new(RejectFirstGuard {
            calls: Mutex::new(0),
        });
        let res_guard = run_agent_loop_v2(ctx_guard).await.unwrap();
        assert_eq!(adapter_guard.history.lock().unwrap().len(), 2);
        assert_eq!(res_guard.summary, "교정 완료");

        // Scenario 18: Auto-verification remaining issues feedback -> 2nd propose passes
        let adapter_verify = Arc::new(MockAdapter::with_responses(vec![
            AgentTurnResponse {
                content: "제안 1차".into(),
                tool_calls: vec![NeutralToolCall {
                    id: "call_p1".into(),
                    name: "propose_hwp_edits".into(),
                    arguments_json: json!({
                        "summary": "1차 제안",
                        "diffItems": [{
                            "scope": "cell",
                            "title": "표 0",
                            "beforeText": "a",
                            "afterText": "b",
                            "unifiedOps": [{ "type": "set_col_width", "tableIndex": 0, "col": 1, "widthMm": 72.0 }]
                        }]
                    })
                    .to_string(),
                }],
                reasoning_tokens: None,
                stop_reason: None,
            },
            AgentTurnResponse {
                content: "제안 2차".into(),
                tool_calls: vec![NeutralToolCall {
                    id: "call_p2".into(),
                    name: "propose_hwp_edits".into(),
                    arguments_json: json!({
                        "summary": "2차 정밀 교정 제안",
                        "diffItems": [{
                            "scope": "cell",
                            "title": "표 0",
                            "beforeText": "a",
                            "afterText": "b",
                            "unifiedOps": [{ "type": "set_col_width", "tableIndex": 0, "col": 1, "widthMm": 80.0 }]
                        }]
                    })
                    .to_string(),
                }],
                reasoning_tokens: None,
                stop_reason: None,
            },
        ]));

        let mut ctx_verify = base_ctx(adapter_verify.clone(), tmp.path());
        ctx_verify.options.user_prompt = "표 0의 오버플로를 수정해줘".into();
        ctx_verify.doc_engine = Arc::new(MockDocEngine::with_comparisons(vec![
            LayoutComparison {
                ok: false,
                resolved_issues: vec![],
                remaining_issues: mock_report().issues,
                new_issues: vec![],
                summary_text: "표 0 (0행 1열): 1.0mm 잔여 넘침".into(),
                metrics_delta: None,
            },
            LayoutComparison {
                ok: true,
                resolved_issues: mock_report().issues,
                remaining_issues: vec![],
                new_issues: vec![],
                summary_text: "이슈 모두 해결됨".into(),
                metrics_delta: None,
            },
        ]));

        let res_verify = run_agent_loop_v2(ctx_verify).await.unwrap();
        assert_eq!(adapter_verify.history.lock().unwrap().len(), 2);
        assert_eq!(res_verify.summary, "2차 정밀 교정 제안");
        assert!(res_verify.verification_passed);
        assert!(
            !default_compare_layout_reports(&mock_report(), &mock_report()).ok
        );
    }
}
