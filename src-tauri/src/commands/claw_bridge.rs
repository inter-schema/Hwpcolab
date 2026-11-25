use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tauri::{AppHandle, Manager};

pub use hopclaw_types::ClawExecResult;
use hopclaw_types::EngineBackend;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

fn resolve_node_binary() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(exe_dir) = exe_path.parent() {
                let next_to_exe = exe_dir.join("node.exe");
                if next_to_exe.is_file() {
                    return next_to_exe;
                }
                let in_res = exe_dir.join("resources").join("node.exe");
                if in_res.is_file() {
                    return in_res;
                }
                let in_claw = exe_dir.join("resources").join("claw-hwp").join("node.exe");
                if in_claw.is_file() {
                    return in_claw;
                }
            }
        }
        if let Ok(local_app) = std::env::var("LOCALAPPDATA") {
            let p = PathBuf::from(local_app).join("Programs").join("nodejs").join("node.exe");
            if p.is_file() {
                return p;
            }
        }
        for candidate in &[
            r"C:\Program Files\nodejs\node.exe",
            r"C:\Program Files (x86)\nodejs\node.exe",
        ] {
            let p = PathBuf::from(candidate);
            if p.is_file() {
                return p;
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(exe_dir) = exe_path.parent() {
                let next_to_exe = exe_dir.join("node");
                if next_to_exe.is_file() {
                    return next_to_exe;
                }
                let in_res = exe_dir.join("resources").join("node");
                if in_res.is_file() {
                    return in_res;
                }
                let in_claw = exe_dir.join("resources").join("claw-hwp").join("node");
                if in_claw.is_file() {
                    return in_claw;
                }
            }
        }
        for candidate in &[
            "/usr/local/bin/node",
            "/usr/bin/node",
        ] {
            let p = PathBuf::from(candidate);
            if p.is_file() {
                return p;
            }
        }
    }
    PathBuf::from("node")
}

// `mut` below is only needed on Windows (creation_flags); on Linux
// clippy -D warnings would fail without this, while the Windows build
// still needs the mutable binding. Same Linux-only issue as the
// 2026-11-04 QA branch fix, which Main never picked up.
#[cfg_attr(not(target_os = "windows"), allow(unused_mut))]
fn new_node_command() -> Command {
    let node_bin = resolve_node_binary();
    let mut cmd = Command::new(node_bin);
    #[cfg(target_os = "windows")]
    {
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

fn strip_unc_prefix(path: PathBuf) -> PathBuf {
    let s = path.to_string_lossy();
    if let Some(stripped) = s.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{}", stripped))
    } else if let Some(stripped) = s.strip_prefix(r"\\?\") {
        PathBuf::from(stripped)
    } else {
        path
    }
}

fn resolve_script_path_inner(app: &AppHandle, script_name: &str) -> Result<PathBuf, String> {
    if let Ok(resource_dir) = app.path().resource_dir() {
        // Tauri v2 maps "../resources/..." to "_up_/resources/..." in resource_dir
        let up_candidate = resource_dir
            .join("_up_")
            .join("resources")
            .join("claw-hwp")
            .join("scripts")
            .join(script_name);
        if up_candidate.exists() {
            return Ok(up_candidate);
        }

        let candidate = resource_dir
            .join("resources")
            .join("claw-hwp")
            .join("scripts")
            .join(script_name);
        if candidate.exists() {
            return Ok(candidate);
        }
    }
    // Exe 옆에 resources 폴더가 있는 포터블 실행 환경 지원
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let portable_candidate = exe_dir
                .join("resources")
                .join("claw-hwp")
                .join("scripts")
                .join(script_name);
            if portable_candidate.exists() {
                return Ok(portable_candidate);
            }
            let portable_up = exe_dir
                .join("_up_")
                .join("resources")
                .join("claw-hwp")
                .join("scripts")
                .join(script_name);
            if portable_up.exists() {
                return Ok(portable_up);
            }
        }
    }
    // 개발 모드 폴백 경로
    let current = std::env::current_dir().map_err(|e| e.to_string())?;
    let root = if current.ends_with("src-tauri") {
        current.parent().unwrap_or(&current).to_path_buf()
    } else {
        current
    };
    let dev_path = root
        .join("resources")
        .join("claw-hwp")
        .join("scripts")
        .join(script_name);
    if dev_path.exists() {
        return Ok(dev_path);
    }
    Err(format!("Script not found: {} (tried: {:?})", script_name, dev_path))
}

fn resolve_script_path(app: &AppHandle, script_name: &str) -> Result<PathBuf, String> {
    let p = resolve_script_path_inner(app, script_name)?;
    Ok(strip_unc_prefix(p))
}

async fn inspect_document_node(app: &AppHandle, file_path: &Path) -> Result<serde_json::Value, String> {
    let script = resolve_script_path(app, "extract_text.js")?;
    let output = new_node_command()
        .arg(&script)
        .arg("--inspect")
        .arg("--with-cell-text")
        .arg(file_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|e| format!("Failed to spawn node extract_text.js: {}", e))?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }
    let stdout_str = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(&stdout_str)
        .map_err(|e| format!("Invalid JSON from inspect: {} (raw: {})", e, stdout_str))
}

async fn extract_markdown_node(app: &AppHandle, file_path: &Path) -> Result<String, String> {
    let script = resolve_script_path(app, "extract_text.js")?;
    let output = new_node_command()
        .arg(&script)
        .arg("--format")
        .arg("markdown")
        .arg(file_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|e| format!("Failed to spawn node extract_text.js: {}", e))?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

async fn inspect_hwpx_rust(bytes: Vec<u8>) -> Result<hopclaw_doc::InspectDocumentResult, String> {
    tokio::task::spawn_blocking(move || {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            hopclaw_doc::inspect_hwpx(&bytes, false)
        }))
        .map_err(|_| "panic in hopclaw_doc::inspect_hwpx".to_string())?
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("inspect_hwpx task join error: {e}"))?
}

async fn extract_hwpx_markdown_rust(bytes: Vec<u8>) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            hopclaw_doc::extract_hwpx_markdown(&bytes)
        }))
        .map_err(|_| "panic in hopclaw_doc::extract_hwpx_markdown".to_string())?
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("extract_hwpx_markdown task join error: {e}"))?
}

/// 1) 문서 구조 및 셀 텍스트 전수 검사 (Strangler router: node | rust | shadow)
#[tauri::command]
pub async fn inspect_document(app: AppHandle, file_path: String) -> Result<serde_json::Value, String> {
    let file_path = strip_unc_prefix(PathBuf::from(file_path));
    let backend = hopclaw_doc::resolve_engine_backend_from_env();

    match backend {
        EngineBackend::Node => inspect_document_node(&app, &file_path).await,
        EngineBackend::Rust => {
            let bytes = tokio::fs::read(&file_path)
                .await
                .map_err(|e| format!("Failed to read document {:?}: {}", file_path, e))?;
            if hopclaw_doc::is_hwpx_bytes(&bytes) {
                let res = inspect_hwpx_rust(bytes).await?;
                serde_json::to_value(res).map_err(|e| e.to_string())
            } else {
                // HWP 5.0 cell-text inspection falls back to Node until `native-rhwp` is linked.
                inspect_document_node(&app, &file_path).await
            }
        }
        EngineBackend::Shadow => {
            let node_res = inspect_document_node(&app, &file_path).await?;
            if let Ok(bytes) = tokio::fs::read(&file_path).await {
                if hopclaw_doc::is_hwpx_bytes(&bytes) {
                    match inspect_hwpx_rust(bytes).await {
                        Ok(rust_res) => {
                            if let Err(diff) = hopclaw_doc::inspect_shadow_diff(&node_res, &rust_res)
                            {
                                eprintln!(
                                    "[claw_bridge][shadow] inspect_document mismatch on {:?}: {}",
                                    file_path, diff
                                );
                            }
                        }
                        Err(err) => {
                            eprintln!(
                                "[claw_bridge][shadow] inspect_document rust error on {:?}: {}",
                                file_path, err
                            );
                        }
                    }
                }
            }
            Ok(node_res)
        }
    }
}

/// 2) 문서 마크다운 텍스트 추출 (Strangler router: node | rust | shadow)
#[tauri::command]
pub async fn extract_markdown(app: AppHandle, file_path: String) -> Result<String, String> {
    let file_path = strip_unc_prefix(PathBuf::from(file_path));
    let backend = hopclaw_doc::resolve_engine_backend_from_env();

    match backend {
        EngineBackend::Node => extract_markdown_node(&app, &file_path).await,
        EngineBackend::Rust => {
            let bytes = tokio::fs::read(&file_path)
                .await
                .map_err(|e| format!("Failed to read document {:?}: {}", file_path, e))?;
            if hopclaw_doc::is_hwpx_bytes(&bytes) {
                extract_hwpx_markdown_rust(bytes).await
            } else {
                // HWP 5.0 markdown extraction falls back to Node until `native-rhwp` is linked.
                extract_markdown_node(&app, &file_path).await
            }
        }
        EngineBackend::Shadow => {
            let node_md = extract_markdown_node(&app, &file_path).await?;
            if let Ok(bytes) = tokio::fs::read(&file_path).await {
                if hopclaw_doc::is_hwpx_bytes(&bytes) {
                    match extract_hwpx_markdown_rust(bytes).await {
                        Ok(rust_md) => {
                            if let Err(diff) = hopclaw_doc::markdown_shadow_diff(&node_md, &rust_md) {
                                eprintln!(
                                    "[claw_bridge][shadow] extract_markdown mismatch on {:?}: {}",
                                    file_path, diff
                                );
                            }
                        }
                        Err(err) => {
                            eprintln!(
                                "[claw_bridge][shadow] extract_markdown rust error on {:?}: {}",
                                file_path, err
                            );
                        }
                    }
                }
            }
            Ok(node_md)
        }
    }
}

/// 3) .hwp (create.js) 또는 .hwpx (hwpx-edit.js) 배치 패치 실행
#[tauri::command]
pub async fn execute_claw_ops(
    app: AppHandle,
    is_hwpx: bool,
    payload_json: String,
) -> Result<ClawExecResult, String> {
    let script_name = if is_hwpx { "hwpx-edit.js" } else { "create.js" };
    let script = resolve_script_path(&app, script_name)?;

    let mut cmd = new_node_command();
    cmd.kill_on_drop(true);
    let mut child = cmd
        .arg(&script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to spawn {}: {}", script_name, e))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(payload_json.as_bytes()).await.map_err(|e| e.to_string())?;
        stdin.shutdown().await.map_err(|e| e.to_string())?;
    }

    let output = match tokio::time::timeout(std::time::Duration::from_secs(30), child.wait_with_output()).await {
        Ok(res) => res.map_err(|e| e.to_string())?,
        Err(_) => {
            return Err("claw 실행 타임아웃: 30초 내에 프로세스가 응답하지 않았습니다.".to_string());
        }
    };
    let stdout_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr_str = String::from_utf8_lossy(&output.stderr).to_string();

    let parsed: serde_json::Value = serde_json::from_str(&stdout_str).unwrap_or_else(|_| {
        serde_json::json!({ "status": "error", "raw_stdout": stdout_str, "stderr": stderr_str })
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
