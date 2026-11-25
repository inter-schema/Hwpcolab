use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;

use hopclaw_types::ParsedAttachment;

use crate::error::AttachError;
use crate::types::{WorkerRequest, WorkerResponse};

const MAX_WORKER_STDOUT_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct WorkerOptions {
    pub worker_bin: PathBuf,
    pub timeout: Duration,
}

impl WorkerOptions {
    pub fn new(worker_bin: impl Into<PathBuf>) -> Self {
        Self {
            worker_bin: worker_bin.into(),
            timeout: Duration::from_secs(30),
        }
    }
}

/// Runs attachment parsing inside a killable worker subprocess over stdin/stdout JSON.
/// On timeout, the child process is explicitly killed.
pub async fn spawn_and_run_worker(
    file_path: &str,
    opts: &WorkerOptions,
) -> Result<ParsedAttachment, AttachError> {
    let mut child = Command::new(&opts.worker_bin)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;

    let req_bytes = serde_json::to_vec(&WorkerRequest {
        file_path: file_path.to_string(),
    })
    .map_err(|e| AttachError::WorkerFailed(format!("워커 요청 직렬화 실패: {e}")))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(&req_bytes).await?;
        stdin.shutdown().await?;
    }

    let io_future = async {
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| AttachError::WorkerFailed("워커 stdout 파이프 없음".into()))?;
        let mut out_bytes = Vec::new();
        let mut limited = stdout.take((MAX_WORKER_STDOUT_BYTES + 1) as u64);
        limited.read_to_end(&mut out_bytes).await?;
        if out_bytes.len() > MAX_WORKER_STDOUT_BYTES {
            return Err(AttachError::LimitExceeded(
                "워커 출력이 허용 한도(32 MiB)를 초과했습니다.".into(),
            ));
        }

        let status = child.wait().await?;
        if !status.success() {
            return Err(AttachError::WorkerFailed(format!(
                "워커 프로세스 비정상 종료: {status}"
            )));
        }

        let resp: WorkerResponse = serde_json::from_slice(&out_bytes)
            .map_err(|e| AttachError::WorkerFailed(format!("워커 응답 파싱 실패: {e}")))?;
        if resp.ok {
            resp.attachment
                .ok_or_else(|| AttachError::WorkerFailed("워커 응답에 attachment 누락".into()))
        } else {
            Err(AttachError::WorkerFailed(
                resp.error.unwrap_or_else(|| "알 수 없는 워커 오류".into()),
            ))
        }
    };

    match tokio::time::timeout(opts.timeout, io_future).await {
        Ok(res) => res,
        Err(_) => {
            let _ = child.kill().await;
            Err(AttachError::WorkerTimeout(format!(
                "첨부 파일 파싱 워커 제한 시간({:?}) 초과로 프로세스를 종료했습니다.",
                opts.timeout
            )))
        }
    }
}
