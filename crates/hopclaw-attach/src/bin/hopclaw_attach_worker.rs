use hopclaw_attach::{
    assert_attachment_file_size, parse_local_attachment, WorkerRequest, WorkerResponse,
};
use std::io::{self, Read};

fn run() -> Result<WorkerResponse, String> {
    let mut input = Vec::new();
    io::stdin()
        .take(1024 * 1024)
        .read_to_end(&mut input)
        .map_err(|e| format!("stdin 읽기 실패: {e}"))?;

    let req: WorkerRequest =
        serde_json::from_slice(&input).map_err(|e| format!("요청 JSON 오류: {e}"))?;

    let meta = std::fs::metadata(&req.file_path)
        .map_err(|e| format!("첨부 파일 메타데이터 조회 실패: {e}"))?;
    assert_attachment_file_size(meta.len()).map_err(|e| e.to_string())?;

    let bytes =
        std::fs::read(&req.file_path).map_err(|e| format!("첨부 파일 읽기 실패: {e}"))?;
    let parsed =
        parse_local_attachment(&req.file_path, &bytes).map_err(|e| e.to_string())?;

    Ok(WorkerResponse {
        ok: true,
        attachment: Some(parsed),
        error: None,
    })
}

fn main() {
    let resp = match run() {
        Ok(r) => r,
        Err(err) => WorkerResponse {
            ok: false,
            attachment: None,
            error: Some(err),
        },
    };
    if let Ok(json) = serde_json::to_string(&resp) {
        println!("{json}");
    }
}
