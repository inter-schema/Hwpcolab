use crate::hwpx_package::is_valid_hwpx_package;
use crate::state::{detect_document_format, write_bytes_atomically, AppState};
pub use hopclaw_types::OpenedDocSession;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use tauri::ipc::{InvokeBody, Request, Response};
use tauri::{AppHandle, Manager, State};

/// Recognize the ZIP local-file-header signature (not an EOCD or arbitrary PK prefix).
#[cfg(test)]
fn has_zip_local_file_header(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0x50, 0x4b, 0x03, 0x04])
}

/// 1) 네이티브 파일 열기 다이얼로그 (.hwp, .hwpx, .xlsx, .pdf, .png 등)
#[tauri::command]
pub async fn pick_local_file(extensions: Vec<String>) -> Result<Option<String>, String> {
    let ext_refs: Vec<&str> = extensions.iter().map(|s| s.as_str()).collect();
    let mut dialog = rfd::AsyncFileDialog::new();
    if !ext_refs.is_empty() {
        dialog = dialog.add_filter("Supported Files", &ext_refs);
    }
    let file = dialog.pick_file().await;
    Ok(file.map(|f| f.path().to_string_lossy().to_string()))
}

/// 2) HWP/HWPX 문서 열기 + Shadow Staging/Disk Undo 세션 생성 + 시그니처 기반 포맷 판별
#[tauri::command]
pub async fn open_document_session(
    app: AppHandle,
    state: State<'_, AppState>,
    file_path: String,
) -> Result<OpenedDocSession, String> {
    let src_path = PathBuf::from(&file_path);
    let bytes = fs::read(&src_path).map_err(|e| format!("파일 읽기 실패: {}", e))?;

    let requested_hwpx = src_path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.eq_ignore_ascii_case("hwpx"))
        .unwrap_or(false);

    let format = detect_document_format(&bytes, requested_hwpx, is_valid_hwpx_package)
        .map_err(|err| err.message)?;

    let ext = match format {
        hopclaw_types::DocFormat::Hwpx => "hwpx",
        hopclaw_types::DocFormat::Hwp5 => "hwp",
    };

    // Ensure the app cache staging dir exists cleanly (AppHandle cache path overrides default temp root if needed).
    if let Ok(cache_dir) = app.path().app_cache_dir() {
        let _ = fs::create_dir_all(cache_dir.join("hop-staging"));
    }

    let file_name = src_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| format!("untitled.{}", ext));

    state
        .sessions
        .open_from_bytes(file_path, file_name, format, bytes)
        .map_err(|err| err.message)
}

/// 3) 빈 화면에서 신규 문서 세션 시작
#[tauri::command]
pub async fn create_blank_session(
    app: AppHandle,
    state: State<'_, AppState>,
    is_hwpx: bool,
) -> Result<OpenedDocSession, String> {
    if let Ok(cache_dir) = app.path().app_cache_dir() {
        let _ = fs::create_dir_all(cache_dir.join("hop-staging"));
    }

    state
        .sessions
        .create_blank(is_hwpx)
        .map_err(|err| err.message)
}

/// Raw-byte IPC read returning `tauri::ipc::Response` (`ArrayBuffer` in WebView JS).
#[tauri::command]
pub async fn read_file_bytes(file_path: String) -> Result<Response, String> {
    let bytes = fs::read(&file_path).map_err(|e| e.to_string())?;
    Ok(Response::new(bytes))
}

/// Decode either raw-byte IPC (`InvokeBody::Raw` + `x-file-path` header) or legacy JSON `{ filePath, bytes }`.
pub(crate) fn parse_write_file_bytes_request(
    body: &InvokeBody,
    headers: &tauri::http::HeaderMap,
) -> Result<(String, Vec<u8>), String> {
    match body {
        InvokeBody::Raw(bytes) => {
            let header_val = headers
                .get("x-file-path")
                .or_else(|| headers.get("file-path"))
                .ok_or_else(|| "x-file-path 헤더가 누락되었습니다.".to_string())?;
            let path = header_val
                .to_str()
                .map_err(|e| format!("x-file-path 헤더 인코딩 오류: {e}"))?
                .to_string();
            Ok((path, bytes.clone()))
        }
        InvokeBody::Json(value) => {
            let file_path = value
                .get("filePath")
                .or_else(|| value.get("file_path"))
                .and_then(|v| v.as_str())
                .ok_or_else(|| "filePath 인자가 누락되었습니다.".to_string())?
                .to_string();
            let raw_bytes = value
                .get("bytes")
                .and_then(|v| v.as_array())
                .ok_or_else(|| "bytes 배열 인자가 누락되었습니다.".to_string())?;
            let mut out = Vec::with_capacity(raw_bytes.len());
            for item in raw_bytes {
                let b = item
                    .as_u64()
                    .and_then(|n| u8::try_from(n).ok())
                    .ok_or_else(|| "bytes 배열에 유효하지 않은 바이트 값이 포함되어 있습니다.".to_string())?;
                out.push(b);
            }
            Ok((file_path, out))
        }
    }
}

/// Dual-mode `write_file_bytes`: accepts raw binary `Request` or backward-compatible JSON `{ filePath, bytes }`.
/// Writes atomically and snapshots a new disk revision when targeting a tracked session's staging file.
#[tauri::command]
pub async fn write_file_bytes(
    state: State<'_, AppState>,
    request: Request<'_>,
) -> Result<(), String> {
    let (file_path, bytes) = parse_write_file_bytes_request(request.body(), request.headers())?;
    let target = Path::new(&file_path);
    if state
        .sessions
        .record_write_if_tracked(target, &bytes)
        .map_err(|e| e.message)?
        .is_none()
    {
        write_bytes_atomically(target, &bytes).map_err(|e| e.message)?;
    }
    Ok(())
}

/// Replace a destination only after the complete sibling temp file has been
/// copied and synced. tempfile::persist uses an atomic replace operation and
/// returns the temporary file on failure so it can be cleaned up safely.
fn commit_staged_file_atomically(
    staging_path: &std::path::Path,
    target_path: &std::path::Path,
) -> Result<(), String> {
    use std::io::Write;
    use tempfile::NamedTempFile;

    let parent = target_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("."));
    if !parent.is_dir() {
        return Err(format!(
            "저장 경로의 상위 디렉터리가 유효하지 않습니다: {}",
            parent.display()
        ));
    }

    let source_metadata = fs::metadata(staging_path).map_err(|error| {
        format!(
            "스테이징 파일을 읽을 수 없습니다: {} ({})",
            staging_path.display(),
            error
        )
    })?;
    if !source_metadata.is_file() {
        return Err(format!(
            "스테이징 경로가 일반 파일이 아닙니다: {}",
            staging_path.display()
        ));
    }

    let mut source = fs::File::open(staging_path).map_err(|error| {
        format!(
            "스테이징 파일을 열 수 없습니다: {} ({})",
            staging_path.display(),
            error
        )
    })?;
    let mut temporary = NamedTempFile::new_in(parent)
        .map_err(|error| format!("저장 임시 파일을 만들 수 없습니다: {}", error))?;

    std::io::copy(&mut source, temporary.as_file_mut())
        .map_err(|error| format!("저장 임시 파일 기록 실패: {}", error))?;
    temporary
        .as_file_mut()
        .flush()
        .map_err(|error| format!("저장 임시 파일 flush 실패: {}", error))?;
    fs::set_permissions(temporary.path(), source_metadata.permissions())
        .map_err(|error| format!("저장 임시 파일 권한 설정 실패: {}", error))?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|error| format!("저장 임시 파일 동기화 실패: {}", error))?;

    temporary
        .persist(target_path)
        .map_err(|error| format!("원본 저장 실패: {}", error.error))?;

    // Best-effort directory sync: some Windows filesystems do not expose
    // directory handles that can be synced, while the file bytes are synced.
    if let Ok(directory) = fs::File::open(parent) {
        let _ = directory.sync_all();
    }
    Ok(())
}

#[tauri::command]
pub async fn commit_staging_to_original(
    staging_path: String,
    original_path: String,
) -> Result<String, String> {
    let target = if original_path.trim().is_empty() {
        let picked = rfd::AsyncFileDialog::new()
            .add_filter("HWP/HWPX Document", &["hwp", "hwpx"])
            .save_file()
            .await;
        match picked {
            Some(file) => file.path().to_string_lossy().to_string(),
            None => return Err("저장이 취소되었습니다.".to_string()),
        }
    } else {
        original_path
    };
    commit_staged_file_atomically(
        std::path::Path::new(&staging_path),
        std::path::Path::new(&target),
    )?;
    Ok(target)
}

/// 5) OS 파일 연결(.hwp/.hwpx 더블클릭) 등으로 실행 시 전달된 시작 파일 경로 조회
#[tauri::command]
pub fn get_startup_file() -> Option<String> {
    std::env::args().skip(1).find(|arg| {
        let lower = arg.to_lowercase();
        (lower.ends_with(".hwp") || lower.ends_with(".hwpx")) && PathBuf::from(arg).exists()
    })
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct HwpDefaultAppStatus {
    pub is_default: bool,
    pub supported: bool,
}

#[tauri::command]
pub async fn get_hwp_default_app_status(app: AppHandle) -> Result<HwpDefaultAppStatus, String> {
    let config_dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    let flag_path = config_dir.join("default_hwp_app.json");
    let is_default = fs::read_to_string(&flag_path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("enabled").and_then(|b| b.as_bool()))
        .unwrap_or(false);
    Ok(HwpDefaultAppStatus {
        is_default,
        supported: true,
    })
}

#[cfg(target_os = "windows")]
fn query_hkcu_ext_progid(ext: &str) -> Option<String> {
    let key = format!(r"HKCU\Software\Classes\{}", ext);
    let output = std::process::Command::new("reg")
        .args(["query", &key, "/ve"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if let Some(idx) = line.find("REG_SZ") {
            let val = line[idx + "REG_SZ".len()..].trim();
            if !val.is_empty() && val != "HOPClaw.Document" {
                return Some(val.to_string());
            }
        }
    }
    None
}

#[cfg(target_os = "windows")]
fn notify_assoc_changed() {
    #[link(name = "shell32")]
    extern "system" {
        fn SHChangeNotify(
            w_event_id: i32,
            u_flags: u32,
            dw_item1: *const std::ffi::c_void,
            dw_item2: *const std::ffi::c_void,
        );
    }
    const SHCNE_ASSOCCHANGED: i32 = 0x0800_0000;
    const SHCNF_IDLIST: u32 = 0x0000;
    unsafe {
        SHChangeNotify(
            SHCNE_ASSOCCHANGED,
            SHCNF_IDLIST,
            std::ptr::null(),
            std::ptr::null(),
        );
    }
}

#[tauri::command]
pub async fn set_hwp_default_app(
    app: AppHandle,
    enabled: bool,
) -> Result<HwpDefaultAppStatus, String> {
    let config_dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    fs::create_dir_all(&config_dir).map_err(|e| e.to_string())?;
    let flag_path = config_dir.join("default_hwp_app.json");

    let mut backups: std::collections::HashMap<String, String> = fs::read_to_string(&flag_path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("backups").cloned())
        .and_then(|b| serde_json::from_value(b).ok())
        .unwrap_or_default();

    #[cfg(target_os = "windows")]
    if let Ok(exe_path) = std::env::current_exe() {
        let exe_str = exe_path.to_string_lossy().to_string();
        let open_cmd = format!("\"{}\" \"%1\"", exe_str);
        if enabled {
            for ext in [".hwp", ".hwpx"] {
                if !backups.contains_key(ext) {
                    if let Some(prev) = query_hkcu_ext_progid(ext) {
                        backups.insert(ext.to_string(), prev);
                    }
                }
            }
            let _ = std::process::Command::new("reg")
                .args([
                    "add",
                    r"HKCU\Software\Classes\HOPClaw.Document",
                    "/ve",
                    "/d",
                    "Hangul Word Processor Document",
                    "/f",
                ])
                .status();
            let _ = std::process::Command::new("reg")
                .args([
                    "add",
                    r"HKCU\Software\Classes\HOPClaw.Document\shell\open\command",
                    "/ve",
                    "/d",
                    &open_cmd,
                    "/f",
                ])
                .status();
            for ext in [".hwp", ".hwpx"] {
                let key = format!(r"HKCU\Software\Classes\{}", ext);
                let _ = std::process::Command::new("reg")
                    .args(["add", &key, "/ve", "/d", "HOPClaw.Document", "/f"])
                    .status();
            }
            notify_assoc_changed();
        } else {
            for ext in [".hwp", ".hwpx"] {
                let key = format!(r"HKCU\Software\Classes\{}", ext);
                if let Some(prev) = backups.get(ext) {
                    let _ = std::process::Command::new("reg")
                        .args(["add", &key, "/ve", "/d", prev, "/f"])
                        .status();
                } else {
                    let _ = std::process::Command::new("reg")
                        .args(["delete", &key, "/ve", "/f"])
                        .status();
                }
            }
            notify_assoc_changed();
        }
    }

    let payload = serde_json::json!({
        "enabled": enabled,
        "backups": backups,
    });
    fs::write(&flag_path, payload.to_string()).map_err(|e| e.to_string())?;

    Ok(HwpDefaultAppStatus {
        is_default: enabled,
        supported: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commit_staged_file_atomically_replaces_an_existing_target() {
        let dir = tempfile::tempdir().unwrap();
        let staging = dir.path().join("staging.hwp");
        let target = dir.path().join("document.hwp");
        std::fs::write(&staging, b"complete-new-document").unwrap();
        std::fs::write(&target, b"old-document").unwrap();

        commit_staged_file_atomically(&staging, &target).unwrap();

        assert_eq!(std::fs::read(&target).unwrap(), b"complete-new-document");
    }

    #[test]
    fn commit_staged_file_atomically_preserves_target_when_staging_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let staging = dir.path().join("missing.hwp");
        let target = dir.path().join("document.hwp");
        std::fs::write(&target, b"preserve-this-document").unwrap();

        assert!(commit_staged_file_atomically(&staging, &target).is_err());

        assert_eq!(std::fs::read(&target).unwrap(), b"preserve-this-document");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn commit_staged_file_atomically_preserves_existing_directory_on_replace_failure() {
        let dir = tempfile::tempdir().unwrap();
        let staging = dir.path().join("staging.hwp");
        let target = dir.path().join("document.hwp");
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("keep.txt"), b"keep").unwrap();
        std::fs::write(&staging, b"new-content").unwrap();

        assert!(commit_staged_file_atomically(&staging, &target).is_err());

        assert_eq!(std::fs::read(target.join("keep.txt")).unwrap(), b"keep");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
    }

    #[test]
    fn recognizes_the_binary_zip_local_file_header_signature() {
        assert!(has_zip_local_file_header(&[0x50, 0x4b, 0x03, 0x04]));
        assert!(!has_zip_local_file_header(&[0x50, 0x4b]));
        assert!(!has_zip_local_file_header(&[0x50, 0x4b, 0x05, 0x06]));
    }

    #[test]
    fn parse_write_file_bytes_accepts_both_raw_and_json_bodies() {
        let mut headers = tauri::http::HeaderMap::new();
        headers.insert("x-file-path", "staging.hwpx".parse().unwrap());
        let raw = InvokeBody::Raw(vec![1, 2, 3, 4]);
        let (path, bytes) = parse_write_file_bytes_request(&raw, &headers).unwrap();
        assert_eq!(path, "staging.hwpx");
        assert_eq!(bytes, vec![1, 2, 3, 4]);

        let json_body = InvokeBody::Json(serde_json::json!({
            "filePath": "legacy.hwp",
            "bytes": [10, 20, 30]
        }));
        let (j_path, j_bytes) =
            parse_write_file_bytes_request(&json_body, &tauri::http::HeaderMap::new()).unwrap();
        assert_eq!(j_path, "legacy.hwp");
        assert_eq!(j_bytes, vec![10, 20, 30]);
    }
}
