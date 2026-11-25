use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use hopclaw_devbridge::{router, DevBridgeConfig, DevBridgeState};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;
use tower::ServiceExt;

fn build_test_router(root: &Path) -> axum::Router {
    let mut cfg = DevBridgeConfig::from_root(root);
    cfg.staging_dir = root.join(".hop-staging");
    cfg.fonts_dir = root.join("fonts");
    router(Arc::new(DevBridgeState::new(cfg)))
}

async fn send_req(
    app: &axum::Router,
    method: Method,
    uri: &str,
    json_body: Option<Value>,
) -> (StatusCode, header::HeaderMap, Vec<u8>) {
    let mut builder = Request::builder().method(method).uri(uri);
    let body = if let Some(val) = json_body {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
        Body::from(serde_json::to_vec(&val).unwrap())
    } else {
        Body::empty()
    };
    let resp = app.clone().oneshot(builder.body(body).unwrap()).await.unwrap();
    let status = resp.status();
    let headers = resp.headers().clone();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes().to_vec();
    (status, headers, bytes)
}

async fn post_cmd(app: &axum::Router, cmd: &str, args: Value) -> Value {
    let (status, headers, bytes) =
        send_req(app, Method::POST, &format!("/__hop_claw_api/{cmd}"), Some(args)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers.get(header::CONTENT_TYPE).unwrap(), "application/json");
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn devbridge_serves_woff2_fonts_and_rejects_traversal() {
    let dir = tempfile::tempdir().unwrap();
    let fonts_dir = dir.path().join("fonts");
    std::fs::create_dir_all(&fonts_dir).unwrap();
    std::fs::write(fonts_dir.join("test.woff2"), b"wOF2-bytes").unwrap();
    std::fs::write(fonts_dir.join("secret.txt"), b"nope").unwrap();
    std::fs::write(dir.path().join("outside.woff2"), b"outside").unwrap();

    let app = build_test_router(dir.path());

    let (status, headers, body) = send_req(&app, Method::GET, "/fonts/test.woff2", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers.get(header::CONTENT_TYPE).unwrap(), "font/woff2");
    assert_eq!(body, b"wOF2-bytes");

    let (non_woff, _, _) = send_req(&app, Method::GET, "/fonts/secret.txt", None).await;
    assert_eq!(non_woff, StatusCode::NOT_FOUND);

    let (traversal, _, _) = send_req(&app, Method::GET, "/fonts/../outside.woff2", None).await;
    assert_eq!(traversal, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn devbridge_handles_session_file_io_and_error_envelope_parity() {
    let dir = tempfile::tempdir().unwrap();
    let sample_hwpx = std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/demo_official_report.hwpx"),
    )
    .unwrap();
    let sample_hwp = std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/demo_official_report.hwp"),
    )
    .unwrap();
    let sample_xlsx = std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/demo_budget_ref.xlsx"),
    )
    .unwrap();
    std::fs::write(dir.path().join("doc.hwpx"), &sample_hwpx).unwrap();
    std::fs::write(dir.path().join("doc.hwp"), &sample_hwp).unwrap();
    std::fs::write(dir.path().join("bad.xlsx"), &sample_xlsx).unwrap();
    std::fs::write(dir.path().join("att.txt"), b"attachment-body").unwrap();

    let app = build_test_router(dir.path());

    // Open valid HWPX
    let open_resp = post_cmd(&app, "open_document_session", json!({ "filePath": "doc.hwpx" })).await;
    assert_eq!(open_resp["result"]["is_hwpx"], json!(true));
    assert_eq!(open_resp["result"]["file_name"], json!("doc.hwpx"));
    let staging_path = open_resp["result"]["staging_path"].as_str().unwrap();
    assert!(Path::new(staging_path).is_file());

    // Open valid HWP5 (exercises padded CFB validation)
    let open_hwp = post_cmd(&app, "open_document_session", json!({ "filePath": "doc.hwp" })).await;
    assert_eq!(open_hwp["result"]["is_hwpx"], json!(false));
    assert_eq!(open_hwp["result"]["file_name"], json!("doc.hwp"));

    // Reject non-HWPX ZIP with __hop_error
    let bad_resp = post_cmd(&app, "open_document_session", json!({ "filePath": "bad.xlsx" })).await;
    assert!(bad_resp["__hop_error"].as_str().unwrap().contains("HWPX"));

    // Create blank, write_file_bytes, read_file_bytes, commit_staging_to_original
    let blank = post_cmd(&app, "create_blank_session", json!({ "isHwpx": true })).await;
    let blank_staging = blank["result"]["staging_path"].as_str().unwrap();

    let w_resp = post_cmd(
        &app,
        "write_file_bytes",
        json!({ "filePath": blank_staging, "bytes": [1, 2, 3, 4] }),
    )
    .await;
    assert_eq!(w_resp["result"], Value::Null);

    let r_resp = post_cmd(&app, "read_file_bytes", json!({ "filePath": blank_staging })).await;
    assert_eq!(r_resp["result"], json!([1, 2, 3, 4]));

    let att_resp = post_cmd(&app, "read_attachment_bytes", json!({ "filePath": "att.txt" })).await;
    assert_eq!(att_resp["result"], json!(b"attachment-body".to_vec()));

    let commit_resp = post_cmd(
        &app,
        "commit_staging_to_original",
        json!({ "stagingPath": blank_staging, "originalPath": "committed.hwpx" }),
    )
    .await;
    let committed_path = commit_resp["result"].as_str().unwrap();
    assert_eq!(std::fs::read(committed_path).unwrap(), vec![1, 2, 3, 4]);

    // Default HWP app toggle + metadata commands parity
    let set_resp = post_cmd(&app, "set_hwp_default_app", json!({ "enabled": true })).await;
    assert_eq!(set_resp["result"]["isDefault"], json!(true));
    assert_eq!(set_resp["result"]["supported"], json!(true));
    let get_resp = post_cmd(&app, "get_hwp_default_app_status", json!({})).await;
    assert_eq!(get_resp["result"]["isDefault"], json!(true));

    assert_eq!(post_cmd(&app, "pick_local_file", json!({})).await["result"], Value::Null);
    assert_eq!(post_cmd(&app, "list_local_fonts", json!({})).await["result"], json!([]));

    // Unknown command returns __hop_error
    let unk = post_cmd(&app, "non_existent_cmd", json!({})).await;
    assert!(unk["__hop_error"]
        .as_str()
        .unwrap()
        .contains("Unknown bridge command"));
}

#[tokio::test]
async fn devbridge_validates_llm_http_policy_and_cancel_state() {
    let dir = tempfile::tempdir().unwrap();
    let app = build_test_router(dir.path());

    // Reject file:// endpoint
    let bad_scheme = post_cmd(
        &app,
        "call_llm_http",
        json!({
            "req": {
                "url": "file:///etc/passwd",
                "body_json": {}
            }
        }),
    )
    .await;
    assert!(bad_scheme["__hop_error"]
        .as_str()
        .unwrap()
        .contains("HTTP or HTTPS"));

    // Reject URL with embedded userinfo
    let bad_userinfo = post_cmd(
        &app,
        "call_llm_http",
        json!({
            "req": {
                "url": "https://user:pass@example.test/v1/chat",
                "body_json": {}
            }
        }),
    )
    .await;
    assert!(bad_userinfo["__hop_error"]
        .as_str()
        .unwrap()
        .contains("userinfo"));

    // Cancel unknown request_id returns false cleanly
    let cancel = post_cmd(
        &app,
        "cancel_llm_request",
        json!({ "requestId": "missing-id" }),
    )
    .await;
    assert_eq!(cancel["result"], json!(false));
}
