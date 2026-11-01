use std::path::PathBuf;
use std::process::Stdio;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tauri::{AppHandle, Manager};
use serde::{Deserialize, Serialize};

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ClawExecResult {
    pub ok: bool,
    pub stdout_json: serde_json::Value,
    pub stderr: String,
}

fn new_node_command() -> Command {
    let mut cmd = Command::new("node");
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
    if let Ok(home) = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")) {
        let fallback = PathBuf::from(home)
            .join(".gemini")
            .join("config")
            .join("skills")
            .join("hwp")
            .join("scripts")
            .join(script_name);
        if fallback.exists() {
            return Ok(fallback);
        }
    }
    Err(format!("Script not found: {} (tried: {:?})", script_name, dev_path))
}

fn resolve_script_path(app: &AppHandle, script_name: &str) -> Result<PathBuf, String> {
    let p = resolve_script_path_inner(app, script_name)?;
    Ok(strip_unc_prefix(p))
}

/// 1) 문서 구조 및 셀 텍스트 전수 검사 (extract_text.js --inspect --with-cell-text)
#[tauri::command]
pub async fn inspect_document(app: AppHandle, file_path: String) -> Result<serde_json::Value, String> {
    let script = resolve_script_path(&app, "extract_text.js")?;
    let file_path = strip_unc_prefix(PathBuf::from(file_path));
    let output = new_node_command()
        .arg(&script)
        .arg("--inspect")
        .arg("--with-cell-text")
        .arg(&file_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|e| format!("Failed to spawn node extract_text.js: {}", e))?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }
    let stdout_str = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(&stdout_str).map_err(|e| format!("Invalid JSON from inspect: {} (raw: {})", e, stdout_str))
}

/// 2) 문서 마크다운 텍스트 추출 (extract_text.js --format markdown)
#[tauri::command]
pub async fn extract_markdown(app: AppHandle, file_path: String) -> Result<String, String> {
    let script = resolve_script_path(&app, "extract_text.js")?;
    let file_path = strip_unc_prefix(PathBuf::from(file_path));
    let output = new_node_command()
        .arg(&script)
        .arg("--format")
        .arg("markdown")
        .arg(&file_path)
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
