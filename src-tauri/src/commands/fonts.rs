use crate::font_catalog::{
    collect_desktop_local_font_entries, read_desktop_local_font, LocalFontEntry,
};
use std::path::Path;
use tauri::ipc::Response;

#[tauri::command]
pub fn list_local_fonts() -> Result<Vec<LocalFontEntry>, String> {
    Ok(collect_desktop_local_font_entries())
}

#[tauri::command]
pub fn read_local_font(path: String) -> Result<Response, String> {
    let bytes = read_desktop_local_font(Path::new(&path))?;
    Ok(Response::new(bytes))
}
