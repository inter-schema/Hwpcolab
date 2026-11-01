use std::fs;
use tauri::command;

#[command]
pub async fn read_attachment_bytes(file_path: String) -> Result<Vec<u8>, String> {
    fs::read(&file_path).map_err(|e| format!("첨부 파일 읽기 실패: {}", e))
}
