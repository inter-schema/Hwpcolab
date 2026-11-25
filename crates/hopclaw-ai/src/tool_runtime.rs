use hopclaw_types::{
    AgentStepEvent, AgentStepType, DocFormat, ExcludedRegion, LayoutReport, TableCoordinateMeta,
    TrialStore, TrialWorkspace, UnifiedOp,
};
use regex::Regex;
use serde_json::{json, Map, Value};
use std::sync::{Arc, OnceLock};
use uuid::Uuid;

use crate::context::{execute_read_document, truncate_tool_result, ReadDocumentArgs, TOOL_RESULT_MAX_LENGTH};
use crate::error::AiError;
use crate::tool_schemas::default_v2_tools;
use crate::traits::{DocEngine, GuardContext, PolicyGuard, PrivacyMasker};
use crate::types::{NeutralToolCall, NeutralToolSpec};

/// Check if a tool is a read-only query that can run concurrently with other read tools.
pub fn is_read_only_tool(name: &str) -> bool {
    name == "inspect_layout" || name == "read_document"
}

/// Robust JSON arguments parser for tool calls.
/// Handles code fences, double-encoded JSON strings, trailing commas, and whitespace.
pub fn parse_tool_args(raw: Option<&str>) -> Result<Map<String, Value>, AiError> {
    static FENCE_RE: OnceLock<Regex> = OnceLock::new();
    static TRAILING_COMMA_RE: OnceLock<Regex> = OnceLock::new();
    let fence_re =
        FENCE_RE.get_or_init(|| Regex::new(r"(?is)^```(?:json)?\s*(.*?)\s*```$").unwrap());
    let trailing_comma_re = TRAILING_COMMA_RE.get_or_init(|| Regex::new(r",\s*([\]}])").unwrap());

    let Some(raw_str) = raw else {
        return Ok(Map::new());
    };
    let mut text = raw_str.trim().to_string();
    if text.is_empty() {
        return Ok(Map::new());
    }

    if let Some(caps) = fence_re.captures(&text) {
        text = caps[1].trim().to_string();
    }

    // Attempt 1: Standard JSON parse (including double-encoded string)
    if let Ok(val) = serde_json::from_str::<Value>(&text) {
        match val {
            Value::Object(map) => return Ok(map),
            Value::String(inner) => {
                if let Ok(Value::Object(map)) = serde_json::from_str::<Value>(&inner) {
                    return Ok(map);
                }
            }
            _ => {}
        }
    }

    // Attempt 2: Clean trailing commas
    let cleaned = trailing_comma_re.replace_all(&text, "$1").into_owned();
    if let Ok(val) = serde_json::from_str::<Value>(&cleaned) {
        match val {
            Value::Object(map) => return Ok(map),
            Value::String(inner) => {
                if let Ok(Value::Object(map)) = serde_json::from_str::<Value>(&inner) {
                    return Ok(map);
                }
            }
            _ => {}
        }
    }

    // Attempt 3: Wrapped in outer JSON string
    if text.starts_with('"') && text.ends_with('"') {
        if let Ok(unescaped) = serde_json::from_str::<String>(&text) {
            return parse_tool_args(Some(&unescaped));
        }
    }

    let preview: String = raw_str.chars().take(100).collect();
    Err(AiError::InvalidArgs(format!(
        "Invalid JSON arguments: {preview}"
    )))
}

pub type StepEmitter = Arc<dyn Fn(AgentStepEvent) + Send + Sync>;

pub struct ToolRuntimeConfig {
    pub request_id: String,
    pub base_bytes: Option<Vec<u8>>,
    pub base_revision: u64,
    pub format: DocFormat,
    pub initial_report: Option<LayoutReport>,
    pub tables_meta: Vec<TableCoordinateMeta>,
    pub excluded_regions: Vec<ExcludedRegion>,
    pub auto_mask_pii: bool,
    pub user_prompt: String,
    pub doc_markdown: String,
    pub available_tools: Vec<NeutralToolSpec>,
    pub masker: Arc<dyn PrivacyMasker>,
    pub guard: Arc<dyn PolicyGuard>,
    pub doc_engine: Arc<dyn DocEngine>,
    pub trial_store: Arc<dyn TrialStore>,
    pub emit_step: StepEmitter,
}

impl ToolRuntimeConfig {
    pub fn infer_format(staging_path: Option<&str>, base_bytes: Option<&[u8]>) -> DocFormat {
        if let Some(p) = staging_path {
            if p.to_ascii_lowercase().ends_with(".hwpx") {
                return DocFormat::Hwpx;
            }
            if p.to_ascii_lowercase().ends_with(".hwp") {
                return DocFormat::Hwp5;
            }
        }
        if let Some(bytes) = base_bytes {
            if bytes.len() >= 2 && bytes[0] == 0x50 && bytes[1] == 0x4b {
                return DocFormat::Hwpx;
            }
        }
        DocFormat::Hwp5
    }
}

/// Stateful Tool Execution Runtime for v2 Agent Loop.
/// Handles cumulative trial edits on a fixed request-scoped trial workspace,
/// privacy masking, and length truncation.
pub struct ToolRuntime {
    pub trials_run: usize,
    pub current_bytes: Option<Vec<u8>>,
    pub current_report: Option<LayoutReport>,
    pub base_bytes: Option<Vec<u8>>,
    pub initial_report: Option<LayoutReport>,
    pub base_revision: u64,
    pub format: DocFormat,

    request_id: String,
    fixed_trial: Option<TrialWorkspace>,
    tables_meta: Vec<TableCoordinateMeta>,
    excluded_regions: Vec<ExcludedRegion>,
    auto_mask_pii: bool,
    user_prompt: String,
    doc_markdown: String,
    available_tools: Vec<NeutralToolSpec>,
    masker: Arc<dyn PrivacyMasker>,
    guard: Arc<dyn PolicyGuard>,
    doc_engine: Arc<dyn DocEngine>,
    trial_store: Arc<dyn TrialStore>,
    emit_step: StepEmitter,
}

impl ToolRuntime {
    pub fn new(cfg: ToolRuntimeConfig) -> Self {
        let request_id = if cfg.request_id.is_empty() {
            format!("req_{}", &Uuid::new_v4().simple().to_string()[..8])
        } else {
            cfg.request_id
        };
        let available_tools = if cfg.available_tools.is_empty() {
            default_v2_tools()
        } else {
            cfg.available_tools
        };

        Self {
            trials_run: 0,
            current_bytes: cfg.base_bytes.clone(),
            current_report: cfg.initial_report.clone(),
            base_bytes: cfg.base_bytes,
            initial_report: cfg.initial_report,
            base_revision: cfg.base_revision,
            format: cfg.format,
            request_id,
            fixed_trial: None,
            tables_meta: cfg.tables_meta,
            excluded_regions: cfg.excluded_regions,
            auto_mask_pii: cfg.auto_mask_pii,
            user_prompt: cfg.user_prompt,
            doc_markdown: cfg.doc_markdown,
            available_tools,
            masker: cfg.masker,
            guard: cfg.guard,
            doc_engine: cfg.doc_engine,
            trial_store: cfg.trial_store,
            emit_step: cfg.emit_step,
        }
    }

    pub fn request_id(&self) -> &str {
        &self.request_id
    }

    pub fn fixed_trial_workspace(&self) -> Option<&TrialWorkspace> {
        self.fixed_trial.as_ref()
    }

    /// Execute a read-only tool call with shared `&self` access (for concurrent execution).
    pub fn execute_read_only_call(&self, call: &NeutralToolCall) -> String {
        let raw_output = match parse_tool_args(Some(&call.arguments_json)) {
            Err(_) => json!({
                "ok": false,
                "error": "인자 파싱 실패: 유효한 JSON 객체 형식이어야 합니다.",
                "hint": "코드펜스나 불필요한 따옴표 없이 올바른 JSON 객체 형태로 전달하세요.",
                "raw": call.arguments_json.chars().take(80).collect::<String>(),
            })
            .to_string(),
            Ok(args) => match call.name.as_str() {
                "inspect_layout" => self.handle_inspect_layout(&args),
                "read_document" => self.handle_read_document(&args),
                other => json!({
                    "ok": false,
                    "error": format!("알 수 없는 도구: '{other}'"),
                    "availableTools": self.available_tools.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
                })
                .to_string(),
            },
        };

        let masked = self.masker.mask_text_for_prompt(
            &raw_output,
            &self.excluded_regions,
            self.auto_mask_pii,
        );
        truncate_tool_result(&masked, TOOL_RESULT_MAX_LENGTH)
    }

    /// Execute any tool call (read-only or mutating trial edits).
    /// Guarantees never returning `Err`; all errors are returned as JSON tool content.
    pub async fn execute_tool_call(&mut self, call: &NeutralToolCall) -> String {
        let raw_output = match self.execute_internal(call).await {
            Ok(s) => s,
            Err(e) => json!({
                "ok": false,
                "error": format!("도구 실행 오류: {e}"),
            })
            .to_string(),
        };

        let masked = self.masker.mask_text_for_prompt(
            &raw_output,
            &self.excluded_regions,
            self.auto_mask_pii,
        );
        truncate_tool_result(&masked, TOOL_RESULT_MAX_LENGTH)
    }

    async fn execute_internal(&mut self, call: &NeutralToolCall) -> Result<String, AiError> {
        let args = match parse_tool_args(Some(&call.arguments_json)) {
            Ok(a) => a,
            Err(_) => {
                return Ok(json!({
                    "ok": false,
                    "error": "인자 파싱 실패: 유효한 JSON 객체 형식이어야 합니다.",
                    "hint": "코드펜스나 불필요한 따옴표 없이 올바른 JSON 객체 형태로 전달하세요.",
                    "raw": call.arguments_json.chars().take(80).collect::<String>(),
                })
                .to_string());
            }
        };

        match call.name.as_str() {
            "inspect_layout" => Ok(self.handle_inspect_layout(&args)),
            "read_document" => Ok(self.handle_read_document(&args)),
            "apply_trial_edits" => self.handle_apply_trial_edits(&args).await,
            other => Ok(json!({
                "ok": false,
                "error": format!("알 수 없는 도구: '{other}'"),
                "availableTools": self.available_tools.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
            })
            .to_string()),
        }
    }

    fn handle_inspect_layout(&self, args: &Map<String, Value>) -> String {
        (self.emit_step)(AgentStepEvent {
            step_type: AgentStepType::Inspect,
            message: "도구 호출: 레이아웃 세부 수치 조회".to_string(),
            detail: None,
        });

        let Some(report) = &self.current_report else {
            return "레이아웃 리포트를 사용할 수 없습니다.".to_string();
        };

        if let Some(table_index) = args.get("tableIndex").and_then(|v| v.as_u64()).map(|n| n as usize) {
            let Some(tbl) = report.tables.iter().find(|t| t.table_index == table_index) else {
                return json!({
                    "ok": false,
                    "error": format!("Table {table_index} not found"),
                })
                .to_string();
            };

            if let (Some(row), Some(col)) = (
                args.get("row").and_then(|v| v.as_u64()).map(|n| n as usize),
                args.get("col").and_then(|v| v.as_u64()).map(|n| n as usize),
            ) {
                let cell = tbl.cells.iter().find(|c| c.row == row && c.col == col);
                let cell_issues: Vec<_> = report
                    .issues
                    .iter()
                    .filter(|i| {
                        i.cell.as_ref().is_some_and(|c| {
                            c.table_index == table_index && c.row == row && c.col == col
                        })
                    })
                    .collect();
                return json!({
                    "ok": true,
                    "cell": cell,
                    "issues": cell_issues,
                })
                .to_string();
            }

            let table_issues: Vec<_> = report
                .issues
                .iter()
                .filter(|i| i.table_index == Some(table_index))
                .collect();
            return json!({
                "ok": true,
                "tableWidthMm": tbl.table_width_mm,
                "printableWidthMm": tbl.printable_width_mm,
                "cellCount": tbl.cell_count,
                "issues": table_issues,
            })
            .to_string();
        }

        self.doc_engine.format_layout_report_for_prompt(report)
    }

    fn handle_read_document(&self, args: &Map<String, Value>) -> String {
        let kind = args
            .get("kind")
            .and_then(|v| v.as_str())
            .unwrap_or("paragraphs")
            .to_string();

        (self.emit_step)(AgentStepEvent {
            step_type: AgentStepType::Inspect,
            message: format!("도구 호출: 문서 부분 조회 ({kind})"),
            detail: None,
        });

        let parsed_args = ReadDocumentArgs {
            kind,
            start: args.get("start").and_then(|v| v.as_u64()).map(|n| n as usize),
            count: args.get("count").and_then(|v| v.as_u64()).map(|n| n as usize),
            table_index: args
                .get("tableIndex")
                .and_then(|v| v.as_u64())
                .map(|n| n as usize),
        };

        execute_read_document(&parsed_args, &self.doc_markdown, &self.tables_meta)
    }

    async fn handle_apply_trial_edits(&mut self, args: &Map<String, Value>) -> Result<String, AiError> {
        self.trials_run += 1;
        let reason = args
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or("시뮬레이션");
        (self.emit_step)(AgentStepEvent {
            step_type: AgentStepType::Trial,
            message: format!("도구 호출: 시험 적용 ({reason})"),
            detail: None,
        });

        if args.get("reset").and_then(|v| v.as_bool()) == Some(true) {
            if let Some(base) = &self.base_bytes {
                self.current_bytes = Some(base.clone());
                self.current_report = self.initial_report.clone();
            }
        }

        let trial_ops: Vec<UnifiedOp> = args
            .get("ops")
            .cloned()
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();

        let guard_ctx = GuardContext {
            user_prompt: self.user_prompt.clone(),
            is_new_document: false,
            excluded_regions: self.excluded_regions.clone(),
            layout_report: self
                .current_report
                .clone()
                .or_else(|| self.initial_report.clone()),
            tables_meta: self.tables_meta.clone(),
        };
        let guard_res = self.guard.lint_ops(&trial_ops, &guard_ctx);

        if !guard_res.passed {
            let detail = guard_res
                .rejected_ops
                .iter()
                .map(|r| r.reason.as_str())
                .collect::<Vec<_>>()
                .join("; ");
            (self.emit_step)(AgentStepEvent {
                step_type: AgentStepType::Guard,
                message: "시험 적용 정책 가드 위반".to_string(),
                detail: Some(detail),
            });
            let feedback_reasons = guard_res
                .rejected_ops
                .iter()
                .map(|r| r.reason.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            let rejected_json: Vec<Value> = guard_res
                .rejected_ops
                .iter()
                .map(|r| json!({ "opIndex": r.op_index, "rule": r.rule, "reason": r.reason }))
                .collect();
            return Ok(json!({
                "ok": false,
                "guardRejected": rejected_json,
                "feedback": format!("정책 가드 위반으로 시험 적용이 취소되었습니다:\n{feedback_reasons}"),
            })
            .to_string());
        }

        let Some(source_bytes) = self.current_bytes.clone().or_else(|| self.base_bytes.clone()) else {
            return Ok(json!({
                "ok": true,
                "note": "파일 바이너리 부재로 정적 가드만 통과했습니다.",
            })
            .to_string());
        };

        let trial = match self.fixed_trial.clone() {
            Some(existing) => {
                if let Err(err) = self
                    .trial_store
                    .write_trial_bytes(&existing, &source_bytes)
                    .await
                {
                    return Ok(json!({
                        "ok": false,
                        "error": format!("시험 적용 처리 중 오류: {err}"),
                    })
                    .to_string());
                }
                existing
            }
            None => {
                match self
                    .trial_store
                    .prepare_trial(
                        &self.request_id,
                        self.format,
                        &source_bytes,
                        self.base_revision,
                    )
                    .await
                {
                    Ok(ws) => {
                        self.fixed_trial = Some(ws.clone());
                        ws
                    }
                    Err(err) => {
                        return Ok(json!({
                            "ok": false,
                            "error": format!("시험 적용 처리 중 오류: {err}"),
                        })
                        .to_string());
                    }
                }
            }
        };

        let unmasked_ops = self
            .masker
            .unmask_and_filter_ops(&guard_res.sanitized_ops, &self.excluded_regions);

        let exec_res = match self
            .doc_engine
            .apply_ops_to_file(&trial.path, self.format, &unmasked_ops, &self.tables_meta)
            .await
        {
            Ok(res) => res,
            Err(err) => {
                return Ok(json!({
                    "ok": false,
                    "error": format!("시험 적용 처리 중 오류: {err}"),
                })
                .to_string());
            }
        };

        if !exec_res.ok {
            return Ok(json!({
                "ok": false,
                "error": if exec_res.stderr.is_empty() { "Claw execution error".to_string() } else { exec_res.stderr },
            })
            .to_string());
        }

        let patched_bytes = match self.trial_store.read_trial_bytes(&trial).await {
            Ok(b) => b,
            Err(err) => {
                return Ok(json!({
                    "ok": false,
                    "error": format!("시험 적용 처리 중 오류: {err}"),
                })
                .to_string());
            }
        };

        let trial_report = match self.doc_engine.inspect_bytes(&patched_bytes).await {
            Ok(r) => r,
            Err(err) => {
                return Ok(json!({
                    "ok": false,
                    "error": format!("시험 적용 처리 중 오류: {err}"),
                })
                .to_string());
            }
        };

        let comparison = self.doc_engine.compare_layout_reports(
            self.initial_report.as_ref().unwrap_or(&trial_report),
            &trial_report,
        );

        self.current_bytes = Some(patched_bytes);
        self.current_report = Some(trial_report.clone());

        (self.emit_step)(AgentStepEvent {
            step_type: AgentStepType::Verify,
            message: format!("시험 적용 실측: {}", comparison.summary_text),
            detail: None,
        });

        Ok(json!({
            "ok": true,
            "summary": comparison.summary_text,
            "resolvedCount": comparison.resolved_issues.len(),
            "remainingCount": comparison.remaining_issues.len(),
            "newCount": comparison.new_issues.len(),
            "details": self.doc_engine.format_layout_report_for_prompt(&trial_report),
        })
        .to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::{AllowAllGuard, NoopPrivacyMasker, TempDirTrialStore};
    use crate::types::LocalBoxFuture;
    use hopclaw_types::{
        CellLayoutInfo, ClawExecResult, LayoutIssue, LayoutIssueCellRef, LayoutIssueCode,
        LayoutIssueMetrics, LayoutIssueSeverity, TableLayoutInfo,
    };
    use std::collections::HashSet;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    fn sample_report() -> LayoutReport {
        let issue = LayoutIssue {
            id: "cell:0:0:0:1:v_overflow".to_string(),
            code: LayoutIssueCode::CELL_OVERFLOW_V,
            severity: LayoutIssueSeverity::Error,
            message: "표 0 (0행 1열): 2.5mm 넘침".to_string(),
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
                cell_height_mm: Some(10.0),
                content_height_mm: Some(12.5),
                overflow_mm: Some(2.5),
                lines: Some(2),
                ..Default::default()
            },
            text: Some("긴 텍스트".to_string()),
        };

        LayoutReport {
            timestamp: 1000,
            table_count: 1,
            issues_count: 1,
            issues: vec![issue],
            tables: vec![TableLayoutInfo {
                table_index: 0,
                section: 0,
                para: 0,
                ctrl: 0,
                cell_count: 4,
                table_width_mm: Some(150.0),
                printable_width_mm: Some(150.0),
                cells: vec![
                    CellLayoutInfo {
                        cell_idx: 0,
                        row: 0,
                        col: 0,
                        row_span: 1,
                        col_span: 1,
                        width_mm: 75.0,
                        height_mm: 10.0,
                        avail_width_mm: 70.0,
                        avail_height_mm: 8.0,
                        estimated_lines: 1,
                        content_height_mm: 5.0,
                        is_vertical_overflow: false,
                        is_horizontal_overflow: false,
                        overflow_amount_mm: 0.0,
                        text: "A".to_string(),
                    },
                    CellLayoutInfo {
                        cell_idx: 1,
                        row: 0,
                        col: 1,
                        row_span: 1,
                        col_span: 1,
                        width_mm: 75.0,
                        height_mm: 10.0,
                        avail_width_mm: 70.0,
                        avail_height_mm: 8.0,
                        estimated_lines: 2,
                        content_height_mm: 12.5,
                        is_vertical_overflow: true,
                        is_horizontal_overflow: false,
                        overflow_amount_mm: 2.5,
                        text: "B".to_string(),
                    },
                ],
            }],
        }
    }

    struct MockDocEngine {
        report: LayoutReport,
        patched_bytes: Vec<u8>,
        seen_paths: Mutex<Vec<PathBuf>>,
    }

    impl DocEngine for MockDocEngine {
        fn inspect_bytes<'a>(
            &'a self,
            _bytes: &'a [u8],
        ) -> LocalBoxFuture<'a, Result<LayoutReport, AiError>> {
            Box::pin(async move { Ok(self.report.clone()) })
        }

        fn apply_ops_to_file<'a>(
            &'a self,
            file_path: &'a Path,
            _format: DocFormat,
            _ops: &'a [UnifiedOp],
            _tables_meta: &'a [TableCoordinateMeta],
        ) -> LocalBoxFuture<'a, Result<ClawExecResult, AiError>> {
            Box::pin(async move {
                self.seen_paths.lock().unwrap().push(file_path.to_path_buf());
                tokio::fs::write(file_path, &self.patched_bytes)
                    .await
                    .unwrap();
                Ok(ClawExecResult {
                    ok: true,
                    stdout_json: json!({}),
                    stderr: String::new(),
                })
            })
        }
    }

    #[test]
    fn is_read_only_tool_identifies_read_tools() {
        assert!(is_read_only_tool("inspect_layout"));
        assert!(is_read_only_tool("read_document"));
        assert!(!is_read_only_tool("apply_trial_edits"));
        assert!(!is_read_only_tool("propose_hwp_edits"));
    }

    #[test]
    fn parse_tool_args_handles_fences_trailing_commas_and_double_encoded() {
        let valid = parse_tool_args(Some(r#"{"reason": "test", "tableIndex": 0}"#)).unwrap();
        assert_eq!(valid["reason"], "test");

        let fenced = parse_tool_args(Some("```json\n{\"reason\": \"fenced\", \"ops\": []}\n```"))
            .unwrap();
        assert_eq!(fenced["reason"], "fenced");

        let trailing = parse_tool_args(Some(r#"{"a": 1, "b": [10, 20,],}"#)).unwrap();
        assert_eq!(trailing["a"], 1);

        let double_encoded = serde_json::to_string(r#"{"reason": "double", "val": 42}"#).unwrap();
        let decoded = parse_tool_args(Some(&double_encoded)).unwrap();
        assert_eq!(decoded["val"], 42);

        assert!(parse_tool_args(Some("")).unwrap().is_empty());
        assert!(parse_tool_args(Some("   ")).unwrap().is_empty());
        assert!(parse_tool_args(None).unwrap().is_empty());

        assert!(parse_tool_args(Some("not a json at all {[[")).is_err());
    }

    #[tokio::test]
    async fn executes_inspect_layout_and_handles_unknown_and_broken_json() {
        let tmp = tempfile::tempdir().unwrap();
        let events = Arc::new(Mutex::new(Vec::new()));
        let events_clone = Arc::clone(&events);

        let mut runtime = ToolRuntime::new(ToolRuntimeConfig {
            request_id: "req-1".into(),
            base_bytes: None,
            base_revision: 1,
            format: DocFormat::Hwpx,
            initial_report: Some(sample_report()),
            tables_meta: vec![],
            excluded_regions: vec![],
            auto_mask_pii: false,
            user_prompt: String::new(),
            doc_markdown: String::new(),
            available_tools: default_v2_tools(),
            masker: Arc::new(NoopPrivacyMasker),
            guard: Arc::new(AllowAllGuard),
            doc_engine: Arc::new(MockDocEngine {
                report: sample_report(),
                patched_bytes: vec![],
                seen_paths: Mutex::new(vec![]),
            }),
            trial_store: Arc::new(TempDirTrialStore::new(tmp.path())),
            emit_step: Arc::new(move |ev| events_clone.lock().unwrap().push(ev)),
        });

        let res_str = runtime
            .execute_tool_call(&NeutralToolCall {
                id: "c1".into(),
                name: "inspect_layout".into(),
                arguments_json: r#"{"tableIndex":0,"row":0,"col":1}"#.into(),
            })
            .await;
        let res: Value = serde_json::from_str(&res_str).unwrap();
        assert_eq!(res["ok"], true);
        assert_eq!(res["cell"]["overflowAmountMm"], 2.5);
        assert_eq!(res["issues"].as_array().unwrap().len(), 1);
        assert_eq!(events.lock().unwrap()[0].step_type, AgentStepType::Inspect);

        let bad_tool_str = runtime
            .execute_tool_call(&NeutralToolCall {
                id: "c2".into(),
                name: "non_existent_tool".into(),
                arguments_json: "{}".into(),
            })
            .await;
        let bad_tool: Value = serde_json::from_str(&bad_tool_str).unwrap();
        assert_eq!(bad_tool["ok"], false);
        assert!(bad_tool["error"].as_str().unwrap().contains("알 수 없는 도구"));

        let broken_json_str = runtime
            .execute_tool_call(&NeutralToolCall {
                id: "c3".into(),
                name: "inspect_layout".into(),
                arguments_json: "this is totally { not json".into(),
            })
            .await;
        let broken: Value = serde_json::from_str(&broken_json_str).unwrap();
        assert_eq!(broken["ok"], false);
        assert!(broken["error"].as_str().unwrap().contains("인자 파싱 실패"));
    }

    #[tokio::test]
    async fn single_fixed_trial_path_and_reset_behavior() {
        let tmp = tempfile::tempdir().unwrap();
        let engine = Arc::new(MockDocEngine {
            report: sample_report(),
            patched_bytes: vec![0x50, 0x4b, 0x99, 0x99],
            seen_paths: Mutex::new(vec![]),
        });
        let base_bytes = vec![0x50, 0x4b, 0x03, 0x04];

        let mut runtime = ToolRuntime::new(ToolRuntimeConfig {
            request_id: "test-req-123".into(),
            base_bytes: Some(base_bytes.clone()),
            base_revision: 1,
            format: DocFormat::Hwpx,
            initial_report: Some(sample_report()),
            tables_meta: vec![],
            excluded_regions: vec![],
            auto_mask_pii: false,
            user_prompt: String::new(),
            doc_markdown: String::new(),
            available_tools: default_v2_tools(),
            masker: Arc::new(NoopPrivacyMasker),
            guard: Arc::new(AllowAllGuard),
            doc_engine: engine.clone(),
            trial_store: Arc::new(TempDirTrialStore::new(tmp.path())),
            emit_step: Arc::new(|_| {}),
        });

        runtime
            .execute_tool_call(&NeutralToolCall {
                id: "t1".into(),
                name: "apply_trial_edits".into(),
                arguments_json: json!({
                    "reason": "시험 1",
                    "ops": [{ "type": "set_col_width", "tableIndex": 0, "col": 1, "widthMm": 80 }]
                })
                .to_string(),
            })
            .await;
        assert_eq!(runtime.current_bytes.as_deref(), Some([0x50, 0x4b, 0x99, 0x99].as_slice()));

        runtime
            .execute_tool_call(&NeutralToolCall {
                id: "t2".into(),
                name: "apply_trial_edits".into(),
                arguments_json: json!({
                    "reason": "시험 2",
                    "ops": [{ "type": "set_row_height", "tableIndex": 0, "row": 0, "heightMm": 15 }]
                })
                .to_string(),
            })
            .await;

        {
            let paths = engine.seen_paths.lock().unwrap();
            let unique: HashSet<&PathBuf> = paths.iter().collect();
            assert_eq!(unique.len(), 1);
            assert!(paths[0]
                .to_string_lossy()
                .contains("trial_test_req_123.hwpx"));
            assert_eq!(runtime.trials_run, 2);
        }

        // Reset call should start from base_bytes again before patching
        let res_reset = runtime
            .execute_tool_call(&NeutralToolCall {
                id: "t3".into(),
                name: "apply_trial_edits".into(),
                arguments_json: json!({
                    "reason": "초기화 후 적용",
                    "reset": true,
                    "ops": [{ "type": "set_col_width", "tableIndex": 0, "col": 0, "widthMm": 65 }]
                })
                .to_string(),
            })
            .await;
        let reset_val: Value = serde_json::from_str(&res_reset).unwrap();
        assert_eq!(reset_val["ok"], true);
        assert_eq!(runtime.trials_run, 3);
    }
}
