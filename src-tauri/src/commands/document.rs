use std::fs;
use std::path::PathBuf;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use uuid::Uuid;
use crate::hwpx_package::is_valid_hwpx_package;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct OpenedDocSession {
    pub original_path: String,
    pub staging_path: String,
    pub file_name: String,
    pub is_hwpx: bool,
    pub bytes: Vec<u8>,
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

/// 2) HWP/HWPX 문서 열기 + Shadow Staging 파일 생성 + 매직 바이트(PK) 기반 포맷 판별
#[tauri::command]
pub async fn open_document_session(app: AppHandle, file_path: String) -> Result<OpenedDocSession, String> {
    let src_path = PathBuf::from(&file_path);
    let bytes = fs::read(&src_path).map_err(|e| format!("파일 읽기 실패: {}", e))?;

    let has_zip_signature = bytes.starts_with(b"PK\\x03\\x04");
    let requested_hwpx = src_path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.eq_ignore_ascii_case("hwpx"))
        .unwrap_or(false);
    let is_hwpx = if has_zip_signature {
        if !is_valid_hwpx_package(&bytes) {
            return Err("유효한 HWPX(OWPML) 패키지가 아닙니다. 일반 ZIP 또는 손상된 문서는 열 수 없습니다.".to_string());
        }
        true
    } else {
        if requested_hwpx {
            return Err("HWPX 파일의 ZIP/OWPML 패키지 구조가 손상되었거나 올바르지 않습니다.".to_string());
        }
        false
    };
    let ext = if is_hwpx { "hwpx" } else { "hwp" };

    let staging_dir = app.path().app_cache_dir().map_err(|e| e.to_string())?.join("hop-staging");
    fs::create_dir_all(&staging_dir).map_err(|e| e.to_string())?;

    let staging_file_name = format!("staging_{}.{}", Uuid::new_v4(), ext);
    let staging_path = staging_dir.join(staging_file_name);
    fs::write(&staging_path, &bytes).map_err(|e| format!("스테이징 파일 생성 실패: {}", e))?;

    let file_name = src_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| format!("untitled.{}", ext));

    Ok(OpenedDocSession {
        original_path: file_path,
        staging_path: staging_path.to_string_lossy().to_string(),
        file_name,
        is_hwpx,
        bytes,
    })
}

/// 3) 빈 화면에서 신규 문서 세션 시작
#[tauri::command]
pub async fn create_blank_session(app: AppHandle, is_hwpx: bool) -> Result<OpenedDocSession, String> {
    let ext = if is_hwpx { "hwpx" } else { "hwp" };
    let staging_dir = app.path().app_cache_dir().map_err(|e| e.to_string())?.join("hop-staging");
    fs::create_dir_all(&staging_dir).map_err(|e| e.to_string())?;
    let staging_path = staging_dir.join(format!("new_{}.{}", Uuid::new_v4(), ext));

    Ok(OpenedDocSession {
        original_path: String::new(),
        staging_path: staging_path.to_string_lossy().to_string(),
        file_name: format!("새 문서.{}", ext),
        is_hwpx,
        bytes: Vec::new(),
    })
}

#[tauri::command]
pub async fn read_file_bytes(file_path: String) -> Result<Vec<u8>, String> {
    fs::read(&file_path).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn write_file_bytes(file_path: String, bytes: Vec<u8>) -> Result<(), String> {
    fs::write(&file_path, bytes).map_err(|e| e.to_string())
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
pub async fn commit_staging_to_original(staging_path: String, original_path: String) -> Result<String, String> {
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
    commit_staged_file_atomically(std::path::Path::new(&staging_path), std::path::Path::new(&target))?;
    Ok(target)
}

/// 5) OS 파일 연결(.hwp/.hwpx 더블클릭) 등으로 실행 시 전달된 시작 파일 경로 조회
#[tauri::command]
pub fn get_startup_file() -> Option<String> {
    std::env::args()
        .skip(1)
        .find(|arg| {
            let lower = arg.to_lowercase();
            (lower.ends_with(".hwp") || lower.ends_with(".hwpx")) && PathBuf::from(arg).exists()
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
}
