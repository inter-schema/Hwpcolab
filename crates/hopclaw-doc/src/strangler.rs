//! Strangler-pattern backend selection (`node | rust | shadow`) and shadow diff helpers
//! used by `src-tauri/src/commands/claw_bridge.rs` for `inspect_document` and `extract_markdown`.

use hopclaw_types::{EngineBackend, InspectDocumentResult};

/// Parse `engine.backend` string (`"node"`, `"rust"`, `"shadow"`), defaulting to `EngineBackend::Node`.
pub fn parse_engine_backend(raw: Option<&str>) -> EngineBackend {
    match raw.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
        Some("rust") => EngineBackend::Rust,
        Some("shadow") => EngineBackend::Shadow,
        _ => EngineBackend::Node,
    }
}

/// Resolve the configured backend from `HOPCLAW_ENGINE_BACKEND` env var (default: `Node`).
pub fn resolve_engine_backend_from_env() -> EngineBackend {
    let val = std::env::var("HOPCLAW_ENGINE_BACKEND").ok();
    parse_engine_backend(val.as_deref())
}

/// Compare Node `inspect_document` JSON against Rust `InspectDocumentResult` in shadow mode.
/// Returns `Ok(())` when parity holds, or `Err(summary)` describing the mismatching fields.
pub fn inspect_shadow_diff(
    node_json: &serde_json::Value,
    rust_res: &InspectDocumentResult,
) -> Result<(), String> {
    let node_parsed: InspectDocumentResult = serde_json::from_value(node_json.clone())
        .map_err(|e| format!("failed to decode node inspect JSON: {e}"))?;

    let mut diffs = Vec::new();
    if node_parsed.file_type != rust_res.file_type {
        diffs.push(format!(
            "fileType: node={} rust={}",
            node_parsed.file_type, rust_res.file_type
        ));
    }
    if node_parsed.section_count != rust_res.section_count {
        diffs.push(format!(
            "sectionCount: node={} rust={}",
            node_parsed.section_count, rust_res.section_count
        ));
    }
    if node_parsed.paragraph_count != rust_res.paragraph_count {
        diffs.push(format!(
            "paragraphCount: node={} rust={}",
            node_parsed.paragraph_count, rust_res.paragraph_count
        ));
    }
    if node_parsed.table_count != rust_res.table_count {
        diffs.push(format!(
            "tableCount: node={} rust={}",
            node_parsed.table_count, rust_res.table_count
        ));
    }
    if node_parsed.cell_count != rust_res.cell_count {
        diffs.push(format!(
            "cellCount: node={} rust={}",
            node_parsed.cell_count, rust_res.cell_count
        ));
    }
    if node_parsed.image_count != rust_res.image_count {
        diffs.push(format!(
            "imageCount: node={:?} rust={:?}",
            node_parsed.image_count, rust_res.image_count
        ));
    }

    if diffs.is_empty() {
        Ok(())
    } else {
        Err(diffs.join("; "))
    }
}

/// Compare Node and Rust markdown extraction outputs (normalizing CRLF line endings).
pub fn markdown_shadow_diff(node_md: &str, rust_md: &str) -> Result<(), String> {
    let n = node_md.replace("\r\n", "\n");
    let r = rust_md.replace("\r\n", "\n");
    if n == r {
        Ok(())
    } else {
        Err(format!(
            "markdown length mismatch: node={} chars, rust={} chars",
            n.chars().count(),
            r.chars().count()
        ))
    }
}
