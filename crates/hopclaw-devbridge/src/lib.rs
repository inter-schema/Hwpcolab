use axum::extract::{Path as AxumPath, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use cfb::CompoundFile;
use hopclaw_types::{ClawExecResult, EngineBackend, OpenedDocSession};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::fs;
use std::io::{Cursor, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tempfile::NamedTempFile;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use uuid::Uuid;

const MAX_ATTACHMENT_BYTES: u64 = 128 * 1024 * 1024;
const DEFAULT_LLM_TIMEOUT_MS: u64 = 180_000;
const MIN_LLM_TIMEOUT_MS: u64 = 1_000;
const MAX_LLM_TIMEOUT_MS: u64 = 600_000;
const MAX_LLM_REQUEST_BODY_BYTES: usize = 20 * 1024 * 1024;
const MAX_LLM_RESPONSE_BODY_BYTES: usize = 32 * 1024 * 1024;
const CFB_MAGIC: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
const HWP5_FILE_HEADER_SIGNATURE: &[u8] = b"HWP Document File";

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Validator hook for HWPX bytes (allows plugging in `hopclaw_hwpx::is_valid_hwpx_package`).
pub type HwpxValidator = Arc<dyn Fn(&[u8]) -> bool + Send + Sync>;

/// Configuration for [`DevBridgeState`], mirroring the directory constants in `vite.config.ts`.
#[derive(Debug, Clone)]
pub struct DevBridgeConfig {
    pub root_dir: PathBuf,
    pub staging_dir: PathBuf,
    pub fonts_dir: PathBuf,
    pub scripts_dir: PathBuf,
    pub engine_backend: EngineBackend,
}

impl DevBridgeConfig {
    pub fn from_root(root_dir: impl Into<PathBuf>) -> Self {
        let root_dir = root_dir.into();
        let staging_dir = root_dir.join(".hop-staging");
        let fonts_dir = root_dir.join("assets").join("fonts");
        let scripts_dir = root_dir.join("resources").join("claw-hwp").join("scripts");
        let engine_backend = match std::env::var("HOP_ENGINE_BACKEND")
            .or_else(|_| std::env::var("HOPCLAW_ENGINE_BACKEND"))
            .ok()
            .as_deref()
            .map(str::trim)
        {
            Some("rust") => EngineBackend::Rust,
            Some("shadow") => EngineBackend::Shadow,
            _ => EngineBackend::Node,
        };
        Self {
            root_dir,
            staging_dir,
            fonts_dir,
            scripts_dir,
            engine_backend,
        }
    }
}

struct CancelEntry {
    token: u64,
    sender: tokio::sync::oneshot::Sender<()>,
}

/// Shared runtime state for the `axum` dev/E2E bridge server.
pub struct DevBridgeState {
    pub config: DevBridgeConfig,
    hwpx_validator: HwpxValidator,
    active_http_cancels: Arc<Mutex<HashMap<String, CancelEntry>>>,
    next_cancel_token: AtomicU64,
    mock_default_hwp_app_enabled: Mutex<bool>,
}

impl DevBridgeState {
    pub fn new(config: DevBridgeConfig) -> Self {
        Self::with_hwpx_validator(config, Arc::new(default_is_valid_hwpx_zip))
    }

    pub fn with_hwpx_validator(config: DevBridgeConfig, hwpx_validator: HwpxValidator) -> Self {
        Self {
            config,
            hwpx_validator,
            active_http_cancels: Arc::new(Mutex::new(HashMap::new())),
            next_cancel_token: AtomicU64::new(1),
            mock_default_hwp_app_enabled: Mutex::new(false),
        }
    }
}

/// Build the `axum::Router` exposing `/__hop_claw_api/{cmd}` and `/fonts/{*path}`.
pub fn router(state: Arc<DevBridgeState>) -> Router {
    Router::new()
        .route("/fonts/{*path}", get(handle_font_asset))
        .route("/__hop_claw_api/{cmd}", post(handle_api_command))
        .with_state(state)
}

/// Start serving the dev bridge on an existing `TcpListener`.
pub async fn serve(
    listener: tokio::net::TcpListener,
    state: Arc<DevBridgeState>,
) -> std::io::Result<()> {
    axum::serve(listener, router(state)).await
}

async fn handle_font_asset(
    State(state): State<Arc<DevBridgeState>>,
    AxumPath(req_path): AxumPath<String>,
) -> Response {
    let Some(candidate) = resolve_font_asset_path(&state.config.fonts_dir, &req_path) else {
        return (StatusCode::NOT_FOUND, "Not Found").into_response();
    };
    match fs::read(&candidate) {
        Ok(bytes) => {
            let mut headers = HeaderMap::new();
            headers.insert(
                header::CONTENT_TYPE,
                header::HeaderValue::from_static("font/woff2"),
            );
            (StatusCode::OK, headers, bytes).into_response()
        }
        Err(_) => (StatusCode::NOT_FOUND, "Not Found").into_response(),
    }
}

/// Resolve one raw `/fonts` request path to a canonical `.woff2` file inside `fonts_dir`.
/// Mirrors `src/hop-host/core/font-asset-path.ts`.
pub fn resolve_font_asset_path(fonts_dir: &Path, raw_request_path: &str) -> Option<PathBuf> {
    let raw = raw_request_path.split('?').next().unwrap_or("");
    let trimmed = raw.trim_start_matches('/');
    if trimmed.is_empty() || trimmed.contains('\0') {
        return None;
    }

    let rel = Path::new(trimmed);
    if rel.is_absolute()
        || rel.components().any(|c| {
            matches!(
                c,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return None;
    }

    let canonical_root = fs::canonicalize(fonts_dir).ok()?;
    let lexical = canonical_root.join(rel);
    let canonical_candidate = fs::canonicalize(&lexical).ok()?;
    if !canonical_candidate.starts_with(&canonical_root) || canonical_candidate == canonical_root {
        return None;
    }
    let ext = canonical_candidate.extension()?.to_str()?;
    if !ext.eq_ignore_ascii_case("woff2") {
        return None;
    }
    if !canonical_candidate.is_file() {
        return None;
    }
    Some(canonical_candidate)
}

async fn handle_api_command(
    State(state): State<Arc<DevBridgeState>>,
    AxumPath(cmd): AxumPath<String>,
    body: axum::body::Bytes,
) -> (StatusCode, Json<Value>) {
    let args: Value = if body.is_empty() {
        json!({})
    } else {
        match serde_json::from_slice(&body) {
            Ok(v) => v,
            Err(err) => {
                return (
                    StatusCode::OK,
                    Json(json!({ "__hop_error": format!("Invalid JSON body: {err}") })),
                );
            }
        }
    };

    match handle_bridge_command(&state, &cmd, &args).await {
        Ok(result) => (StatusCode::OK, Json(json!({ "result": result }))),
        Err(message) => (StatusCode::OK, Json(json!({ "__hop_error": message }))),
    }
}

/// Dispatch one `/__hop_claw_api/:cmd` request with parity to `vite.config.ts`.
pub async fn handle_bridge_command(
    state: &DevBridgeState,
    cmd: &str,
    args: &Value,
) -> Result<Value, String> {
    fs::create_dir_all(&state.config.staging_dir).map_err(|e| e.to_string())?;

    match cmd {
        "get_startup_file" => {
            let val = std::env::var("HOP_STARTUP_FILE").ok();
            Ok(serde_json::to_value(val).map_err(|e| e.to_string())?)
        }

        "pick_local_file" => Ok(Value::Null),

        "list_local_fonts" => Ok(json!([])),

        "read_local_font" => {
            let raw = arg_str(args, &["path"]).unwrap_or("");
            let target = resolve_against_root(&state.config.root_dir, raw);
            let bytes = fs::read(&target).map_err(|e| e.to_string())?;
            Ok(serde_json::to_value(bytes).map_err(|e| e.to_string())?)
        }

        "open_document_session" => {
            let raw = arg_str(args, &["filePath", "file_path"]).unwrap_or("");
            let src_path = resolve_against_root(&state.config.root_dir, raw);
            if !src_path.exists() {
                return Err(format!("파일 읽기 실패: {raw}"));
            }
            let bytes = fs::read(&src_path).map_err(|e| format!("파일 읽기 실패: {e}"))?;
            let has_zip = bytes.starts_with(&[0x50, 0x4b, 0x03, 0x04]);
            let requested_hwpx = src_path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.eq_ignore_ascii_case("hwpx"))
                .unwrap_or(false);

            let is_hwpx = if has_zip {
                if !(state.hwpx_validator)(&bytes) {
                    return Err(
                        "유효한 HWPX(OWPML) 패키지가 아닙니다. 일반 ZIP 또는 손상된 문서는 열 수 없습니다."
                            .to_string(),
                    );
                }
                true
            } else {
                if requested_hwpx {
                    return Err(
                        "HWPX 파일의 ZIP/OWPML 패키지 구조가 손상되었거나 올바르지 않습니다."
                            .to_string(),
                    );
                }
                validate_hwp5_if_cfb(&bytes)?;
                false
            };

            let ext = if is_hwpx { "hwpx" } else { "hwp" };
            let staging_path = state
                .config
                .staging_dir
                .join(format!("staging_{}.{}", Uuid::new_v4(), ext));
            fs::write(&staging_path, &bytes).map_err(|e| e.to_string())?;

            let file_name = src_path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| format!("untitled.{ext}"));

            let session = OpenedDocSession {
                original_path: raw.to_string(),
                staging_path: staging_path.to_string_lossy().into_owned(),
                file_name,
                is_hwpx,
                bytes,
                session_id: None,
                revision: None,
            };
            Ok(serde_json::to_value(session).map_err(|e| e.to_string())?)
        }

        "create_blank_session" => {
            let is_hwpx = arg_bool(args, &["isHwpx", "is_hwpx"]).unwrap_or(false);
            let ext = if is_hwpx { "hwpx" } else { "hwp" };
            let staging_path = state
                .config
                .staging_dir
                .join(format!("new_{}.{}", Uuid::new_v4(), ext));
            let session = OpenedDocSession {
                original_path: String::new(),
                staging_path: staging_path.to_string_lossy().into_owned(),
                file_name: format!("새 문서.{ext}"),
                is_hwpx,
                bytes: Vec::new(),
                session_id: None,
                revision: None,
            };
            Ok(serde_json::to_value(session).map_err(|e| e.to_string())?)
        }

        "read_file_bytes" => {
            let raw = arg_str(args, &["filePath", "file_path"]).unwrap_or("");
            let target = resolve_against_root(&state.config.root_dir, raw);
            let bytes = fs::read(&target).map_err(|e| e.to_string())?;
            Ok(serde_json::to_value(bytes).map_err(|e| e.to_string())?)
        }

        "write_file_bytes" => {
            let raw = arg_str(args, &["filePath", "file_path"]).unwrap_or("");
            let target = resolve_against_root(&state.config.root_dir, raw);
            let bytes = parse_byte_array(args.get("bytes"))?;
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            fs::write(&target, &bytes).map_err(|e| e.to_string())?;
            Ok(Value::Null)
        }

        "read_attachment_bytes" => {
            let raw = arg_str(args, &["filePath", "file_path"]).unwrap_or("");
            let target = resolve_against_root(&state.config.root_dir, raw);
            let st = fs::metadata(&target).map_err(|e| e.to_string())?;
            if !st.is_file() {
                return Err(format!("첨부 경로가 일반 파일이 아닙니다: {raw}"));
            }
            if st.len() > MAX_ATTACHMENT_BYTES {
                return Err(format!(
                    "첨부 파일은 128 MiB 이하만 처리할 수 있습니다: {raw}"
                ));
            }
            let bytes = fs::read(&target).map_err(|e| e.to_string())?;
            Ok(serde_json::to_value(bytes).map_err(|e| e.to_string())?)
        }

        "commit_staging_to_original" => {
            let staging_raw = arg_str(args, &["stagingPath", "staging_path"]).unwrap_or("");
            let staging_path = resolve_against_root(&state.config.root_dir, staging_raw);
            let original_raw = arg_str(args, &["originalPath", "original_path"])
                .unwrap_or("")
                .trim();
            let target_path = if original_raw.is_empty() {
                let ext = staging_path
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| format!(".{e}"))
                    .unwrap_or_else(|| ".hwp".to_string());
                state
                    .config
                    .staging_dir
                    .join(format!("saved_{}{}", Uuid::new_v4(), ext))
            } else {
                resolve_against_root(&state.config.root_dir, original_raw)
            };
            copy_file_atomically(&staging_path, &target_path)?;
            Ok(Value::String(target_path.to_string_lossy().into_owned()))
        }

        "inspect_document" => {
            let raw = arg_str(args, &["filePath", "file_path"]).unwrap_or("");
            let target = resolve_against_root(&state.config.root_dir, raw);
            let script = state.config.scripts_dir.join("extract_text.js");
            let output = new_node_command()
                .arg(&script)
                .arg("--inspect")
                .arg("--with-cell-text")
                .arg(&target)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output()
                .await
                .map_err(|e| format!("Failed to spawn extract_text.js: {e}"))?;
            if !output.status.success() {
                return Err(String::from_utf8_lossy(&output.stderr).into_owned());
            }
            let stdout_str = String::from_utf8_lossy(&output.stdout);
            serde_json::from_str(&stdout_str)
                .map_err(|e| format!("Invalid JSON from inspect_document: {e}"))
        }

        "extract_markdown" => {
            let raw = arg_str(args, &["filePath", "file_path"]).unwrap_or("");
            let target = resolve_against_root(&state.config.root_dir, raw);
            let script = state.config.scripts_dir.join("extract_text.js");
            let output = new_node_command()
                .arg(&script)
                .arg("--format")
                .arg("markdown")
                .arg(&target)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output()
                .await
                .map_err(|e| format!("Failed to spawn extract_text.js: {e}"))?;
            if !output.status.success() {
                return Err(String::from_utf8_lossy(&output.stderr).into_owned());
            }
            Ok(Value::String(
                String::from_utf8_lossy(&output.stdout).into_owned(),
            ))
        }

        "execute_claw_ops" => {
            let is_hwpx = arg_bool(args, &["isHwpx", "is_hwpx"]).unwrap_or(false);
            let payload_json = arg_str(args, &["payloadJson", "payload_json"]).unwrap_or("{}");
            let script = state
                .config
                .scripts_dir
                .join(if is_hwpx { "hwpx-edit.js" } else { "create.js" });
            let result = run_claw_script_with_stdin(&script, payload_json, 30_000).await?;
            Ok(serde_json::to_value(result).map_err(|e| e.to_string())?)
        }

        "call_llm_http" => {
            let req_val = args.get("req").cloned().unwrap_or_else(|| json!({}));
            let req: LlmBridgeRequest = serde_json::from_value(req_val)
                .map_err(|e| format!("Invalid call_llm_http req: {e}"))?;
            let resp = call_llm_http_inner(state, req).await?;
            Ok(serde_json::to_value(resp).map_err(|e| e.to_string())?)
        }

        "cancel_llm_request" => {
            let request_id = arg_str(args, &["requestId", "request_id"]).unwrap_or("");
            let mut lock = state
                .active_http_cancels
                .lock()
                .map_err(|e| e.to_string())?;
            if let Some(entry) = lock.remove(request_id) {
                let _ = entry.sender.send(());
                Ok(Value::Bool(true))
            } else {
                Ok(Value::Bool(false))
            }
        }

        "get_hwp_default_app_status" => {
            let enabled = *state
                .mock_default_hwp_app_enabled
                .lock()
                .map_err(|e| e.to_string())?;
            Ok(json!({
                "isDefault": enabled,
                "platform": node_platform_name(),
                "supported": true,
            }))
        }

        "set_hwp_default_app" => {
            let enabled = arg_bool(args, &["enabled"]).unwrap_or(false);
            let mut lock = state
                .mock_default_hwp_app_enabled
                .lock()
                .map_err(|e| e.to_string())?;
            *lock = enabled;
            Ok(json!({
                "isDefault": *lock,
                "platform": node_platform_name(),
                "supported": true,
            }))
        }

        other => Err(format!("Unknown bridge command: {other}")),
    }
}

fn node_platform_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "win32"
    } else if cfg!(target_os = "macos") {
        "darwin"
    } else {
        "linux"
    }
}

fn resolve_against_root(root: &Path, raw: &str) -> PathBuf {
    let p = Path::new(raw);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        root.join(p)
    }
}

fn arg_str<'a>(args: &'a Value, keys: &[&str]) -> Option<&'a str> {
    for k in keys {
        if let Some(s) = args.get(*k).and_then(|v| v.as_str()) {
            return Some(s);
        }
    }
    None
}

fn arg_bool(args: &Value, keys: &[&str]) -> Option<bool> {
    for k in keys {
        if let Some(b) = args.get(*k).and_then(|v| v.as_bool()) {
            return Some(b);
        }
    }
    None
}

fn parse_byte_array(value: Option<&Value>) -> Result<Vec<u8>, String> {
    let Some(arr) = value.and_then(|v| v.as_array()) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::with_capacity(arr.len());
    for v in arr {
        let b = v
            .as_u64()
            .and_then(|n| u8::try_from(n).ok())
            .ok_or_else(|| "Invalid byte value in bytes array".to_string())?;
        out.push(b);
    }
    Ok(out)
}

/// Lightweight HWPX ZIP/OWPML structural check used as default when `hopclaw-hwpx` is not plugged in.
fn default_is_valid_hwpx_zip(bytes: &[u8]) -> bool {
    let Ok(mut archive) = zip::ZipArchive::new(Cursor::new(bytes)) else {
        return false;
    };
    if archive.is_empty() {
        return false;
    }
    let Ok(mut mimetype_entry) = archive.by_index(0) else {
        return false;
    };
    if mimetype_entry.name() != "mimetype"
        || mimetype_entry.compression() != zip::CompressionMethod::Stored
    {
        return false;
    }
    let mut mime_buf = String::new();
    if mimetype_entry.read_to_string(&mut mime_buf).is_err() || mime_buf != "application/hwp+zip" {
        return false;
    }
    drop(mimetype_entry);

    let mut has_header = false;
    let mut has_section0 = false;
    let mut has_hpf = false;
    for i in 0..archive.len() {
        if let Ok(entry) = archive.by_index(i) {
            let name = entry.name();
            if name.eq_ignore_ascii_case("Contents/header.xml") {
                has_header = true;
            } else if name.eq_ignore_ascii_case("Contents/section0.xml") {
                has_section0 = true;
            } else if name.eq_ignore_ascii_case("Contents/content.hpf") {
                has_hpf = true;
            }
        }
    }
    has_header && has_section0 && has_hpf
}

fn validate_hwp5_if_cfb(bytes: &[u8]) -> Result<(), String> {
    if !bytes.starts_with(&CFB_MAGIC) {
        return Ok(());
    }
    let mut buf = bytes.to_vec();
    if buf.len() >= 512 {
        let sector_shift = u16::from_le_bytes([buf[30], buf[31]]);
        let sector_len: usize = match sector_shift {
            12 => 4096,
            _ => 512,
        };
        let num_fat_sectors = u32::from_le_bytes([buf[44], buf[45], buf[46], buf[47]]) as usize;
        let entries_per_fat_sector = sector_len / 4;
        if let Some(fat_entries) = num_fat_sectors.checked_mul(entries_per_fat_sector) {
            if let Some(needed_len) = fat_entries
                .checked_add(1)
                .and_then(|total_sectors| total_sectors.checked_mul(sector_len))
            {
                let max_pad = sector_len * entries_per_fat_sector;
                if needed_len > buf.len() && needed_len - buf.len() <= max_pad {
                    buf.resize(needed_len, 0);
                }
            }
        }
    }
    let mut comp = CompoundFile::open(Cursor::new(buf))
        .map_err(|e| format!("HWP 5.0 CFB 컨테이너 열기 실패: {e}"))?;
    let mut stream = comp
        .open_stream("/FileHeader")
        .or_else(|_| comp.open_stream("FileHeader"))
        .map_err(|e| format!("HWP 5.0 FileHeader 누락: {e}"))?;
    let mut buf = [0_u8; 32];
    let n = stream.read(&mut buf).map_err(|e| e.to_string())?;
    if n < HWP5_FILE_HEADER_SIGNATURE.len()
        || &buf[..HWP5_FILE_HEADER_SIGNATURE.len()] != HWP5_FILE_HEADER_SIGNATURE
    {
        return Err("유효한 HWP 5.0 문서가 아닙니다 (FileHeader 서명 불일치).".to_string());
    }
    Ok(())
}

fn copy_file_atomically(source_path: &Path, target_path: &Path) -> Result<(), String> {
    let parent = target_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    if !parent.is_dir() {
        return Err(format!(
            "저장 경로의 상위 디렉터리가 유효하지 않습니다: {}",
            parent.display()
        ));
    }
    let meta = fs::metadata(source_path).map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err(format!(
            "스테이징 경로가 일반 파일이 아닙니다: {}",
            source_path.display()
        ));
    }
    let mut src = fs::File::open(source_path).map_err(|e| e.to_string())?;
    let mut temp = NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    std::io::copy(&mut src, temp.as_file_mut()).map_err(|e| e.to_string())?;
    temp.as_file_mut().flush().map_err(|e| e.to_string())?;
    fs::set_permissions(temp.path(), meta.permissions()).map_err(|e| e.to_string())?;
    temp.as_file().sync_all().map_err(|e| e.to_string())?;
    temp.persist(target_path).map_err(|e| e.error.to_string())?;
    if let Ok(dir) = fs::File::open(parent) {
        let _ = dir.sync_all();
    }
    Ok(())
}

#[cfg_attr(not(target_os = "windows"), allow(unused_mut))]
fn new_node_command() -> Command {
    let mut cmd = Command::new("node");
    #[cfg(target_os = "windows")]
    {
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

async fn run_claw_script_with_stdin(
    script_path: &Path,
    payload_json: &str,
    timeout_ms: u64,
) -> Result<ClawExecResult, String> {
    let mut cmd = new_node_command();
    cmd.kill_on_drop(true);
    let mut child = cmd
        .arg(script_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to spawn {}: {e}", script_path.display()))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(payload_json.as_bytes())
            .await
            .map_err(|e| e.to_string())?;
        stdin.shutdown().await.map_err(|e| e.to_string())?;
    }

    let output = tokio::time::timeout(
        Duration::from_millis(timeout_ms),
        child.wait_with_output(),
    )
    .await
    .map_err(|_| "claw 실행 타임아웃: 30초 내에 프로세스가 응답하지 않았습니다.".to_string())?
    .map_err(|e| e.to_string())?;

    let stdout_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr_str = String::from_utf8_lossy(&output.stderr).into_owned();
    let parsed: Value = serde_json::from_str(&stdout_str).unwrap_or_else(|_| {
        json!({ "status": "error", "raw_stdout": stdout_str, "stderr": stderr_str })
    });
    let ok = output.status.success()
        && (parsed.get("status").and_then(|v| v.as_str()) == Some("success")
            || parsed.get("ok").and_then(|v| v.as_bool()) == Some(true));
    Ok(ClawExecResult {
        ok,
        stdout_json: parsed,
        stderr: stderr_str,
    })
}

#[derive(Debug, Deserialize)]
struct LlmBridgeRequest {
    #[serde(default)]
    url: String,
    #[serde(default)]
    headers: HashMap<String, String>,
    #[serde(default)]
    body_json: Value,
    timeout_ms: Option<u64>,
    request_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct LlmBridgeResponse {
    status: u16,
    body: Value,
}

struct CancelGuard {
    map: Arc<Mutex<HashMap<String, CancelEntry>>>,
    key: Option<(String, u64)>,
}

impl Drop for CancelGuard {
    fn drop(&mut self) {
        if let Some((request_id, token)) = &self.key {
            if let Ok(mut lock) = self.map.lock() {
                if lock.get(request_id).is_some_and(|e| e.token == *token) {
                    lock.remove(request_id);
                }
            }
        }
    }
}

async fn call_llm_http_inner(
    state: &DevBridgeState,
    req: LlmBridgeRequest,
) -> Result<LlmBridgeResponse, String> {
    let url = reqwest::Url::parse(&req.url)
        .map_err(|e| format!("LLM endpoint URL이 올바르지 않습니다: {e}"))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("LLM endpoint must use HTTP or HTTPS.".to_string());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("LLM endpoint URLs must not contain userinfo.".to_string());
    }

    let body_bytes = serde_json::to_vec(&req.body_json).map_err(|e| e.to_string())?;
    if body_bytes.len() > MAX_LLM_REQUEST_BODY_BYTES {
        return Err(format!(
            "LLM request body exceeds the {MAX_LLM_REQUEST_BODY_BYTES}-byte limit."
        ));
    }

    let timeout_ms = req
        .timeout_ms
        .unwrap_or(DEFAULT_LLM_TIMEOUT_MS)
        .clamp(MIN_LLM_TIMEOUT_MS, MAX_LLM_TIMEOUT_MS);

    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(timeout_ms))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| e.to_string())?;

    let has_content_type = req
        .headers
        .keys()
        .any(|k| k.eq_ignore_ascii_case("content-type"));
    let mut builder = client.post(url);
    for (k, v) in req.headers {
        builder = builder.header(&k, &v);
    }
    if !has_content_type {
        builder = builder.header(reqwest::header::CONTENT_TYPE, "application/json");
    }
    let builder = builder.body(body_bytes);

    let mut cancel_token = None;
    let cancel_rx = if let Some(ref request_id) = req.request_id {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let token = state.next_cancel_token.fetch_add(1, Ordering::Relaxed);
        let mut lock = state
            .active_http_cancels
            .lock()
            .map_err(|e| e.to_string())?;
        match lock.entry(request_id.clone()) {
            Entry::Vacant(e) => {
                e.insert(CancelEntry { token, sender: tx });
                cancel_token = Some(token);
                Some(rx)
            }
            Entry::Occupied(_) => {
                return Err("동일 request_id가 이미 실행 중입니다.".to_string());
            }
        }
    } else {
        None
    };

    let _guard = CancelGuard {
        map: state.active_http_cancels.clone(),
        key: req.request_id.zip(cancel_token),
    };

    let fut = async {
        let mut resp = builder.send().await.map_err(|e| e.to_string())?;
        let status = resp.status().as_u16();
        if resp
            .content_length()
            .is_some_and(|n| n > MAX_LLM_RESPONSE_BODY_BYTES as u64)
        {
            return Err(format!(
                "LLM response body exceeds the {MAX_LLM_RESPONSE_BODY_BYTES}-byte limit."
            ));
        }
        let mut out = Vec::new();
        while let Some(chunk) = resp.chunk().await.map_err(|e| e.to_string())? {
            if out.len().saturating_add(chunk.len()) > MAX_LLM_RESPONSE_BODY_BYTES {
                return Err(format!(
                    "LLM response body exceeds the {MAX_LLM_RESPONSE_BODY_BYTES}-byte limit."
                ));
            }
            out.extend_from_slice(&chunk);
        }
        let text = String::from_utf8_lossy(&out);
        let clean = text.strip_prefix('\u{feff}').unwrap_or(&text);
        let body: Value =
            serde_json::from_str(clean).unwrap_or_else(|_| json!({ "raw_text": clean }));
        Ok::<LlmBridgeResponse, String>(LlmBridgeResponse { status, body })
    };

    match cancel_rx {
        Some(rx) => tokio::select! {
            res = fut => res,
            _ = rx => Err("LLM 요청이 사용자에 의해 취소되었습니다.".to_string()),
        },
        None => fut.await,
    }
}
