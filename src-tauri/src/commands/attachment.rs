use std::fs;
use tauri::command;

const MAX_ATTACHMENT_BYTES: u64 = 128 * 1024 * 1024;

fn validate_attachment_size(file_path: &str, size: u64) -> Result<(), String> {
    if size > MAX_ATTACHMENT_BYTES {
        return Err(format!(
            "첨부 파일은 128 MiB 이하만 처리할 수 있습니다: {}",
            file_path
        ));
    }
    Ok(())
}

#[command]
pub async fn read_attachment_bytes(file_path: String) -> Result<Vec<u8>, String> {
    let metadata = fs::metadata(&file_path).map_err(|error| {
        format!(
            "첨부 파일 정보를 읽을 수 없습니다: {} ({})",
            file_path, error
        )
    })?;
    if !metadata.is_file() {
        return Err(format!("첨부 경로가 일반 파일이 아닙니다: {}", file_path));
    }
    validate_attachment_size(&file_path, metadata.len())?;
    fs::read(&file_path).map_err(|error| format!("첨부 파일 읽기 실패: {}", error))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attachment_size_accepts_the_exact_limit() {
        assert!(validate_attachment_size("allowed.docx", MAX_ATTACHMENT_BYTES).is_ok());
    }

    #[test]
    fn attachment_size_rejects_files_above_the_limit() {
        assert!(validate_attachment_size("large.docx", MAX_ATTACHMENT_BYTES + 1).is_err());
    }
}
