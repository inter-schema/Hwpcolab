use hopclaw_types::{
    BoxFuture, ClawExecResult, DocFormat, ErrorCode, ErrorModel, ExcludedRegion, LayoutComparison,
    LayoutIssue, LayoutIssueCode, LayoutReport, TableCoordinateMeta, TrialStore, TrialWorkspace,
    UnifiedOp,
};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

use crate::error::AiError;
use crate::types::LocalBoxFuture;

/// Privacy masking boundary implemented by `hopclaw-privacy`.
pub trait PrivacyMasker: Send + Sync {
    fn mask_text_for_prompt(
        &self,
        text: &str,
        excluded_regions: &[ExcludedRegion],
        auto_mask_pii: bool,
    ) -> String;

    fn unmask_text(&self, text: &str) -> String;

    fn unmask_and_filter_ops(
        &self,
        ops: &[UnifiedOp],
        excluded_regions: &[ExcludedRegion],
    ) -> Vec<UnifiedOp>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct NoopPrivacyMasker;

impl PrivacyMasker for NoopPrivacyMasker {
    fn mask_text_for_prompt(
        &self,
        text: &str,
        _excluded_regions: &[ExcludedRegion],
        _auto_mask_pii: bool,
    ) -> String {
        text.to_string()
    }

    fn unmask_text(&self, text: &str) -> String {
        text.to_string()
    }

    fn unmask_and_filter_ops(
        &self,
        ops: &[UnifiedOp],
        _excluded_regions: &[ExcludedRegion],
    ) -> Vec<UnifiedOp> {
        ops.to_vec()
    }
}

#[derive(Debug, Clone, Default)]
pub struct GuardContext {
    pub user_prompt: String,
    pub is_new_document: bool,
    pub excluded_regions: Vec<ExcludedRegion>,
    pub layout_report: Option<LayoutReport>,
    pub tables_meta: Vec<TableCoordinateMeta>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuardRejection {
    pub op_index: usize,
    pub rule: String,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct GuardResult {
    pub passed: bool,
    pub sanitized_ops: Vec<UnifiedOp>,
    pub rejected_ops: Vec<GuardRejection>,
    pub warnings: Vec<String>,
}

/// Policy Guard (G1-G6) boundary implemented by `hopclaw-ops`.
pub trait PolicyGuard: Send + Sync {
    fn lint_ops(&self, ops: &[UnifiedOp], ctx: &GuardContext) -> GuardResult;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct AllowAllGuard;

impl PolicyGuard for AllowAllGuard {
    fn lint_ops(&self, ops: &[UnifiedOp], _ctx: &GuardContext) -> GuardResult {
        GuardResult {
            passed: true,
            sanitized_ops: ops.to_vec(),
            rejected_ops: Vec::new(),
            warnings: Vec::new(),
        }
    }
}

/// Document inspection + operation execution boundary implemented by `hopclaw-doc` /
/// `hopclaw-ops` / `hopclaw-hwpx` / `hopclaw-hwp5`.
pub trait DocEngine: Send + Sync {
    fn inspect_bytes<'a>(
        &'a self,
        bytes: &'a [u8],
    ) -> LocalBoxFuture<'a, Result<LayoutReport, AiError>>;

    fn apply_ops_to_file<'a>(
        &'a self,
        file_path: &'a Path,
        format: DocFormat,
        ops: &'a [UnifiedOp],
        tables_meta: &'a [TableCoordinateMeta],
    ) -> LocalBoxFuture<'a, Result<ClawExecResult, AiError>>;

    fn format_layout_report_for_prompt(&self, report: &LayoutReport) -> String {
        default_format_layout_report_for_prompt(report)
    }

    fn compare_layout_reports(
        &self,
        before: &LayoutReport,
        after: &LayoutReport,
    ) -> LayoutComparison {
        default_compare_layout_reports(before, after)
    }
}

pub fn default_format_layout_report_for_prompt(report: &LayoutReport) -> String {
    if report.issues_count == 0 {
        return format!(
            "[레이아웃 검사 결과: 정상] 표 {}개에 대해 시각적 넘침이나 여백 침범 이슈가 검출되지 않았습니다.",
            report.table_count
        );
    }

    let mut lines = vec![
        format!("[레이아웃 검사 결과: 이슈 {}건 검출]", report.issues_count),
        format!("총 {}개의 표에서 발견된 문제 목록:", report.table_count),
    ];

    for (i, iss) in report.issues.iter().enumerate() {
        let severity_str = match iss.severity {
            hopclaw_types::LayoutIssueSeverity::Error => "error",
            hopclaw_types::LayoutIssueSeverity::Warning => "warning",
        };
        if iss.code == LayoutIssueCode::CELL_OVERFLOW_V {
            if let Some(cell) = &iss.cell {
                lines.push(format!(
                    "{}. [세로 넘침 / {}] 표 {} [행 {}, 열 {}]: 내용({}mm) > 셀({}mm) (+{}mm 초과, {}줄) | \"{}\"",
                    i + 1,
                    severity_str,
                    cell.table_index,
                    cell.row,
                    cell.col,
                    iss.metrics.content_height_mm.unwrap_or(0.0),
                    iss.metrics.cell_height_mm.unwrap_or(0.0),
                    iss.metrics.overflow_mm.unwrap_or(0.0),
                    iss.metrics.lines.unwrap_or(0),
                    iss.text.as_deref().unwrap_or("")
                ));
                continue;
            }
        } else if iss.code == LayoutIssueCode::TABLE_EXCEEDS_PAGE {
            lines.push(format!(
                "{}. [표 폭 초과 / {}] 표 {}: 표 너비 {}mm > 인쇄 영역 {}mm (+{}mm 여백 침범)",
                i + 1,
                severity_str,
                iss.table_index.unwrap_or(0),
                iss.metrics.table_width_mm.unwrap_or(0.0),
                iss.metrics.printable_width_mm.unwrap_or(0.0),
                iss.metrics.overflow_width_mm.unwrap_or(0.0)
            ));
            continue;
        }
        let code_str = match iss.code {
            LayoutIssueCode::CELL_OVERFLOW_V => "CELL_OVERFLOW_V",
            LayoutIssueCode::CELL_OVERFLOW_H => "CELL_OVERFLOW_H",
            LayoutIssueCode::TABLE_EXCEEDS_PAGE => "TABLE_EXCEEDS_PAGE",
        };
        lines.push(format!(
            "{}. [{} / {}] {}",
            i + 1,
            code_str,
            severity_str,
            iss.message
        ));
    }

    lines.join("\n")
}

pub fn default_compare_layout_reports(
    before: &LayoutReport,
    after: &LayoutReport,
) -> LayoutComparison {
    let before_ids: HashSet<&str> = before.issues.iter().map(|i| i.id.as_str()).collect();
    let after_ids: HashSet<&str> = after.issues.iter().map(|i| i.id.as_str()).collect();

    let resolved_issues: Vec<LayoutIssue> = before
        .issues
        .iter()
        .filter(|i| !after_ids.contains(i.id.as_str()))
        .cloned()
        .collect();
    let remaining_issues: Vec<LayoutIssue> = after
        .issues
        .iter()
        .filter(|i| before_ids.contains(i.id.as_str()))
        .cloned()
        .collect();
    let new_issues: Vec<LayoutIssue> = after
        .issues
        .iter()
        .filter(|i| !before_ids.contains(i.id.as_str()))
        .cloned()
        .collect();

    let ok = new_issues.is_empty() && (after.issues_count == 0 || !resolved_issues.is_empty());

    let mut parts = Vec::new();
    if !resolved_issues.is_empty() {
        parts.push(format!("해결된 이슈: {}건", resolved_issues.len()));
    }
    if !remaining_issues.is_empty() {
        parts.push(format!("잔여 이슈: {}건", remaining_issues.len()));
    }
    if !new_issues.is_empty() {
        parts.push(format!("새로 발생한 이슈: {}건", new_issues.len()));
    }
    if parts.is_empty() {
        parts.push("이슈 없음 (정상)".to_string());
    }

    LayoutComparison {
        ok,
        resolved_issues,
        remaining_issues,
        new_issues,
        summary_text: parts.join(", "),
        metrics_delta: None,
    }
}

/// Filesystem-backed `TrialStore` isolating `trial_<req_id>.hwpx|hwp` files
/// in a temporary directory and cleaning them up on completion or cancel.
pub struct TempDirTrialStore {
    base_dir: PathBuf,
    tracked: Arc<Mutex<HashMap<String, Vec<PathBuf>>>>,
}

impl TempDirTrialStore {
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        Self {
            base_dir: base_dir.into(),
            tracked: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn safe_id(request_id: &str) -> String {
        request_id
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect()
    }
}

impl Default for TempDirTrialStore {
    fn default() -> Self {
        Self::new(std::env::temp_dir().join("hopclaw-trials"))
    }
}

impl TrialStore for TempDirTrialStore {
    fn prepare_trial<'a>(
        &'a self,
        request_id: &'a str,
        format: DocFormat,
        base_bytes: &'a [u8],
        base_revision: u64,
    ) -> BoxFuture<'a, Result<TrialWorkspace, ErrorModel>> {
        Box::pin(async move {
            tokio::fs::create_dir_all(&self.base_dir)
                .await
                .map_err(|e| {
                    ErrorModel::new(ErrorCode::E_INTERNAL_PANIC, format!("임시 디렉토리 생성 실패: {e}"))
                })?;

            let ext = match format {
                DocFormat::Hwpx => "hwpx",
                DocFormat::Hwp5 => "hwp",
            };
            let safe = Self::safe_id(request_id);
            let trial_id = format!("trial_{safe}_{}", Uuid::new_v4().simple());
            let path = self.base_dir.join(format!("trial_{safe}.{ext}"));

            tokio::fs::write(&path, base_bytes).await.map_err(|e| {
                ErrorModel::new(ErrorCode::E_INTERNAL_PANIC, format!("시험 파일 쓰기 실패: {e}"))
            })?;

            if let Ok(mut guard) = self.tracked.lock() {
                let list = guard.entry(request_id.to_string()).or_default();
                if !list.contains(&path) {
                    list.push(path.clone());
                }
            }

            Ok(TrialWorkspace {
                trial_id,
                path,
                format,
                base_revision,
            })
        })
    }

    fn read_trial_bytes<'a>(
        &'a self,
        trial: &'a TrialWorkspace,
    ) -> BoxFuture<'a, Result<Vec<u8>, ErrorModel>> {
        Box::pin(async move {
            tokio::fs::read(&trial.path).await.map_err(|e| {
                ErrorModel::new(ErrorCode::E_INTERNAL_PANIC, format!("시험 파일 읽기 실패: {e}"))
            })
        })
    }

    fn write_trial_bytes<'a>(
        &'a self,
        trial: &'a TrialWorkspace,
        bytes: &'a [u8],
    ) -> BoxFuture<'a, Result<(), ErrorModel>> {
        Box::pin(async move {
            tokio::fs::write(&trial.path, bytes).await.map_err(|e| {
                ErrorModel::new(ErrorCode::E_INTERNAL_PANIC, format!("시험 파일 쓰기 실패: {e}"))
            })
        })
    }

    fn cleanup_trial<'a>(
        &'a self,
        trial: &'a TrialWorkspace,
    ) -> BoxFuture<'a, Result<(), ErrorModel>> {
        Box::pin(async move {
            match tokio::fs::remove_file(&trial.path).await {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(ErrorModel::new(
                    ErrorCode::E_INTERNAL_PANIC,
                    format!("시험 파일 삭제 실패: {e}"),
                )),
            }
        })
    }

    fn cleanup_request_trials<'a>(
        &'a self,
        request_id: &'a str,
    ) -> BoxFuture<'a, Result<(), ErrorModel>> {
        Box::pin(async move {
            let paths = {
                self.tracked
                    .lock()
                    .ok()
                    .and_then(|mut guard| guard.remove(request_id))
                    .unwrap_or_default()
            };
            for path in paths {
                match tokio::fs::remove_file(&path).await {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => {
                        return Err(ErrorModel::new(
                            ErrorCode::E_INTERNAL_PANIC,
                            format!("시험 파일 정리 실패: {e}"),
                        ));
                    }
                }
            }
            Ok(())
        })
    }
}
