use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

// Core proxy logic lives in `crates/hopclaw-ai/src/proxy.rs`.
// Until `hopclaw-ai = { path = "../crates/hopclaw-ai" }` is added to `src-tauri/Cargo.toml`
// during workspace integration, this thin adapter keeps the exact Tauri IPC signatures
// while enforcing the post-DNS-resolution 169.254.0.0/16 guard.

pub struct CancelEntry {
    token: u64,
    sender: tokio::sync::oneshot::Sender<()>,
}

static NEXT_CANCEL_TOKEN: AtomicU64 = AtomicU64::new(1);

pub fn get_cancel_map() -> &'static Arc<Mutex<HashMap<String, CancelEntry>>> {
    static CANCEL_MAP: OnceLock<Arc<Mutex<HashMap<String, CancelEntry>>>> = OnceLock::new();
    CANCEL_MAP.get_or_init(|| Arc::new(Mutex::new(HashMap::new())))
}

fn cleanup_cancel_registration(request_id: &str, token: u64) {
    if let Ok(mut lock) = get_cancel_map().lock() {
        if lock
            .get(request_id)
            .is_some_and(|entry| entry.token == token)
        {
            lock.remove(request_id);
        }
    }
}

struct CancelCleanup(Option<(String, u64)>);

impl Drop for CancelCleanup {
    fn drop(&mut self) {
        if let Some((request_id, token)) = self.0.as_ref() {
            cleanup_cancel_registration(request_id, *token);
        }
    }
}

#[derive(Deserialize, Debug)]
pub struct LlmProxyRequest {
    pub url: String,
    pub headers: HashMap<String, String>,
    pub body_json: serde_json::Value,
    pub timeout_ms: Option<u64>,
    pub request_id: Option<String>,
}

#[derive(Serialize, Debug)]
pub struct LlmProxyResponse {
    pub status: u16,
    pub body: serde_json::Value,
}

#[tauri::command]
pub async fn cancel_llm_request(request_id: String) -> Result<bool, String> {
    let map = get_cancel_map();
    let mut lock = map.lock().map_err(|e| e.to_string())?;
    if let Some(entry) = lock.remove(&request_id) {
        let _ = entry.sender.send(());
        Ok(true)
    } else {
        Ok(false)
    }
}

const DEFAULT_TIMEOUT_MS: u64 = 180_000;
const MIN_TIMEOUT_MS: u64 = 1_000;
const MAX_TIMEOUT_MS: u64 = 600_000;
const MAX_REQUEST_BODY_BYTES: usize = 20 * 1024 * 1024;
const MAX_RESPONSE_BODY_BYTES: usize = 32 * 1024 * 1024;

fn is_forbidden_link_local_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.octets()[0] == 169 && v4.octets()[1] == 254,
        IpAddr::V6(v6) => {
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return mapped.octets()[0] == 169 && mapped.octets()[1] == 254;
            }
            if let Some(compat) = v6.to_ipv4() {
                if v6.segments()[..6] == [0, 0, 0, 0, 0, 0] {
                    return compat.octets()[0] == 169 && compat.octets()[1] == 254;
                }
            }
            false
        }
    }
}

fn validate_resolved_addrs(host: &str, addrs: &[SocketAddr]) -> Result<(), String> {
    if addrs.is_empty() {
        return Err(format!("DNS 조회 결과가 비어 있습니다: {host}"));
    }
    for addr in addrs {
        if is_forbidden_link_local_ip(addr.ip()) {
            return Err(format!(
                "DNS 조회 결과({host} -> {})가 169.254.0.0/16 링크로컬/메타데이터 대역에 해당하여 차단되었습니다.",
                addr.ip()
            ));
        }
    }
    Ok(())
}

async fn resolve_and_guard_endpoint(url: &reqwest::Url) -> Result<Vec<SocketAddr>, String> {
    let port = url
        .port_or_known_default()
        .ok_or_else(|| "LLM endpoint 포트를 확인할 수 없습니다.".to_string())?;
    let host_str = url
        .host_str()
        .ok_or_else(|| "LLM endpoint 호스트가 비어 있습니다.".to_string())?;

    if let Ok(v4) = host_str.parse::<Ipv4Addr>() {
        let addrs = vec![SocketAddr::new(IpAddr::V4(v4), port)];
        validate_resolved_addrs(host_str, &addrs)?;
        return Ok(addrs);
    }

    let clean_v6 = host_str.trim_start_matches('[').trim_end_matches(']');
    if let Ok(v6) = clean_v6.parse::<std::net::Ipv6Addr>() {
        let addrs = vec![SocketAddr::new(IpAddr::V6(v6), port)];
        validate_resolved_addrs(host_str, &addrs)?;
        return Ok(addrs);
    }

    let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host_str, port))
        .await
        .map_err(|e| format!("DNS 조회 실패 ({host_str}:{port}): {e}"))?
        .collect();
    validate_resolved_addrs(host_str, &addrs)?;
    Ok(addrs)
}

fn parse_llm_endpoint(raw_url: &str) -> Result<reqwest::Url, String> {
    let url = reqwest::Url::parse(raw_url)
        .map_err(|error| format!("LLM endpoint URL이 올바르지 않습니다: {}", error))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("LLM endpoint는 HTTP 또는 HTTPS URL이어야 합니다.".to_string());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("LLM endpoint URL에 사용자명이나 비밀번호를 포함할 수 없습니다.".to_string());
    }
    Ok(url)
}

fn serialize_bounded_request_body(body: &serde_json::Value) -> Result<Vec<u8>, String> {
    let bytes = serde_json::to_vec(body)
        .map_err(|error| format!("LLM 요청 본문을 직렬화할 수 없습니다: {}", error))?;
    if bytes.len() > MAX_REQUEST_BODY_BYTES {
        return Err(format!(
            "LLM 요청 본문이 허용 크기({} MiB)를 초과했습니다.",
            MAX_REQUEST_BODY_BYTES / (1024 * 1024)
        ));
    }
    Ok(bytes)
}

fn validate_response_content_length(content_length: Option<u64>) -> Result<(), String> {
    if content_length.is_some_and(|length| length > MAX_RESPONSE_BODY_BYTES as u64) {
        return Err(format!(
            "LLM 응답 본문이 허용 크기({} MiB)를 초과했습니다.",
            MAX_RESPONSE_BODY_BYTES / (1024 * 1024)
        ));
    }
    Ok(())
}

fn normalize_timeout_ms(requested: Option<u64>) -> u64 {
    requested
        .unwrap_or(DEFAULT_TIMEOUT_MS)
        .clamp(MIN_TIMEOUT_MS, MAX_TIMEOUT_MS)
}

async fn read_bounded_response_text(mut response: reqwest::Response) -> Result<String, String> {
    validate_response_content_length(response.content_length())?;

    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| format!("LLM 응답 본문을 읽을 수 없습니다: {}", error))?
    {
        let next_size = bytes
            .len()
            .checked_add(chunk.len())
            .ok_or_else(|| "LLM 응답 본문 크기를 계산할 수 없습니다.".to_string())?;
        if next_size > MAX_RESPONSE_BODY_BYTES {
            return Err(format!(
                "LLM 응답 본문이 허용 크기({} MiB)를 초과했습니다.",
                MAX_RESPONSE_BODY_BYTES / (1024 * 1024)
            ));
        }
        bytes.extend_from_slice(&chunk);
    }

    let text = String::from_utf8_lossy(&bytes).into_owned();
    Ok(text.strip_prefix('\u{feff}').unwrap_or(&text).to_string())
}

#[tauri::command]
pub async fn call_llm_http(req: LlmProxyRequest) -> Result<LlmProxyResponse, String> {
    let endpoint = parse_llm_endpoint(&req.url)?;
    let resolved_addrs = resolve_and_guard_endpoint(&endpoint).await?;
    let request_body = serialize_bounded_request_body(&req.body_json)?;
    let timeout_ms = normalize_timeout_ms(req.timeout_ms);

    let mut client_builder = reqwest::Client::builder()
        .timeout(Duration::from_millis(timeout_ms))
        // Do not forward provider credentials to a redirect target.
        .redirect(reqwest::redirect::Policy::none());

    if resolved_addrs.iter().all(|addr| addr.ip().is_loopback()) {
        client_builder = client_builder.no_proxy();
    }

    if let Some(domain) = endpoint.domain() {
        client_builder = client_builder.resolve_to_addrs(domain, &resolved_addrs);
    }

    let client = client_builder
        .build()
        .map_err(|error| format!("Reqwest 클라이언트 생성 실패: {}", error))?;

    let has_content_type = req
        .headers
        .keys()
        .any(|name| name.eq_ignore_ascii_case("content-type"));
    let mut builder = client.post(endpoint);
    for (name, value) in req.headers {
        builder = builder.header(&name, &value);
    }
    if !has_content_type {
        builder = builder.header(reqwest::header::CONTENT_TYPE, "application/json");
    }
    let builder = builder.body(request_body);

    let request_id = req.request_id.clone();
    let mut cancel_token = None;
    let cancel_rx = if let Some(ref request_id) = request_id {
        use std::collections::hash_map::Entry;

        let (sender, receiver) = tokio::sync::oneshot::channel::<()>();
        let token = NEXT_CANCEL_TOKEN.fetch_add(1, Ordering::Relaxed);
        let mut lock = get_cancel_map()
            .lock()
            .map_err(|error| format!("LLM 취소 상태 잠금 실패: {}", error))?;

        match lock.entry(request_id.clone()) {
            Entry::Vacant(entry) => {
                entry.insert(CancelEntry { token, sender });
                cancel_token = Some(token);
                Some(receiver)
            }
            Entry::Occupied(_) => {
                return Err("동일 request_id가 이미 실행 중입니다.".to_string());
            }
        }
    } else {
        None
    };

    let _cleanup = CancelCleanup(request_id.zip(cancel_token));

    let request_future = async {
        let response = builder
            .send()
            .await
            .map_err(|error| format!("LLM 네트워크 요청 실패: {}", error))?;
        let status = response.status().as_u16();
        let text = read_bounded_response_text(response).await?;
        let body =
            serde_json::from_str(&text).unwrap_or_else(|_| serde_json::json!({ "raw_text": text }));
        Ok::<LlmProxyResponse, String>(LlmProxyResponse { status, body })
    };

    match cancel_rx {
        Some(receiver) => {
            tokio::select! {
                result = request_future => result,
                _ = receiver => Err("LLM 요청이 사용자에 의해 취소되었습니다.".to_string()),
            }
        }
        None => request_future.await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::Future;
    use tokio::io::AsyncReadExt;
    use tokio::io::AsyncWriteExt;
    use tokio::net::TcpListener;
    use uuid::Uuid;

    #[test]
    fn endpoint_validation_allows_http_and_https_but_rejects_other_schemes_and_userinfo() {
        assert!(parse_llm_endpoint("https://api.example.test/v1/chat").is_ok());
        assert!(parse_llm_endpoint("http://127.0.0.1:11434/v1/chat").is_ok());
        assert!(parse_llm_endpoint("file:///etc/passwd").is_err());
        assert!(parse_llm_endpoint("ftp://example.test/model").is_err());
        assert!(parse_llm_endpoint("https://user:secret@example.test/v1/chat").is_err());
    }

    #[test]
    fn post_dns_guard_blocks_169_254_metadata_ips() {
        let err = validate_resolved_addrs(
            "rebind.example.test",
            &["169.254.169.254:443".parse().unwrap()],
        )
        .unwrap_err();
        assert!(err.contains("169.254.0.0/16"));

        let err_v6 = validate_resolved_addrs(
            "rebind.example.test",
            &["[::ffff:169.254.169.254]:443".parse().unwrap()],
        )
        .unwrap_err();
        assert!(err_v6.contains("169.254.0.0/16"));

        assert!(validate_resolved_addrs(
            "api.example.test",
            &["93.184.216.34:443".parse().unwrap()],
        )
        .is_ok());
    }

    #[test]
    fn request_body_size_is_bounded_before_network_dispatch() {
        let under_limit = serde_json::json!({"prompt": "x".repeat(MAX_REQUEST_BODY_BYTES - 100)});
        let bytes = serialize_bounded_request_body(&under_limit).unwrap();
        assert!(bytes.len() <= MAX_REQUEST_BODY_BYTES);

        let over_limit = serde_json::json!({"prompt": "x".repeat(MAX_REQUEST_BODY_BYTES + 1)});
        assert!(serialize_bounded_request_body(&over_limit).is_err());
    }

    #[test]
    fn response_content_length_is_rejected_above_the_memory_limit() {
        assert!(validate_response_content_length(Some(MAX_RESPONSE_BODY_BYTES as u64)).is_ok());
        assert!(
            validate_response_content_length(Some(MAX_RESPONSE_BODY_BYTES as u64 + 1)).is_err()
        );
        assert!(validate_response_content_length(None).is_ok());
    }

    #[test]
    fn user_timeout_is_clamped_to_the_supported_range() {
        assert_eq!(normalize_timeout_ms(None), DEFAULT_TIMEOUT_MS);
        assert_eq!(normalize_timeout_ms(Some(1)), MIN_TIMEOUT_MS);
        assert_eq!(normalize_timeout_ms(Some(u64::MAX)), MAX_TIMEOUT_MS);
        assert_eq!(normalize_timeout_ms(Some(30_000)), 30_000);
    }

    fn request(url: String, request_id: Option<String>) -> LlmProxyRequest {
        LlmProxyRequest {
            url,
            headers: HashMap::new(),
            body_json: serde_json::json!({"model": "test", "messages": []}),
            timeout_ms: Some(5_000),
            request_id,
        }
    }

    #[tokio::test]
    async fn request_without_request_id_is_not_cancelled_immediately() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request_bytes = [0_u8; 2048];
            let _ = stream.read(&mut request_bytes).await.unwrap();
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 11\r\nConnection: close\r\n\r\n{\"ok\":true}",
                )
                .await
                .unwrap();
        });

        let response = call_llm_http(request(format!("http://{address}/chat"), None)).await;

        assert!(
            response.is_ok(),
            "request without an ID should run: {response:?}"
        );
        assert_eq!(response.unwrap().status, 200);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn does_not_follow_redirects_to_another_host() {
        let redirect_target = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let redirect_target_address = redirect_target.local_addr().unwrap();
        let source = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let source_address = source.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let (mut stream, _) = source.accept().await.unwrap();
            let mut request_bytes = [0_u8; 2048];
            let _ = stream.read(&mut request_bytes).await.unwrap();
            let response = format!(
                "HTTP/1.1 302 Found\r\nLocation: http://{}/credential-sink\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                redirect_target_address
            );
            stream.write_all(response.as_bytes()).await.unwrap();
        });

        let mut req = request(format!("http://{source_address}/redirect"), None);
        req.headers.insert(
            "authorization".to_string(),
            "Bearer do-not-forward".to_string(),
        );
        let response = call_llm_http(req).await.unwrap();

        assert_eq!(response.status, 302);
        assert!(
            tokio::time::timeout(Duration::from_millis(250), redirect_target.accept())
                .await
                .is_err(),
            "the client must not follow a provider redirect to another host"
        );
        server.await.unwrap();
    }

    #[tokio::test]
    async fn rejects_oversized_response_content_length_before_body_read() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request_bytes = [0_u8; 2048];
            let _ = stream.read(&mut request_bytes).await.unwrap();
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        MAX_RESPONSE_BODY_BYTES + 1
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
        });

        let result = call_llm_http(request(format!("http://{address}/large"), None)).await;

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("허용 크기"));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn duplicate_request_ids_do_not_replace_or_cancel_the_original_request() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let request_id = format!("duplicate-test-{}", Uuid::new_v4());
        let (accepted_tx, accepted_rx) = tokio::sync::oneshot::channel();

        let server = tokio::spawn(async move {
            let (_stream, _) = listener.accept().await.unwrap();
            let _ = accepted_tx.send(());
            tokio::time::sleep(Duration::from_secs(5)).await;
        });

        let first_id = request_id.clone();
        let first = tokio::spawn(async move {
            call_llm_http(request(format!("http://{address}/first"), Some(first_id))).await
        });
        accepted_rx.await.unwrap();

        let duplicate = call_llm_http(request(
            format!("http://{address}/second"),
            Some(request_id.clone()),
        ))
        .await;

        assert!(
            duplicate.is_err(),
            "a duplicate in-flight request ID must be rejected"
        );
        assert!(
            duplicate.unwrap_err().contains("이미 실행 중"),
            "duplicate ID should produce an explicit conflict error"
        );
        assert!(cancel_llm_request(request_id).await.unwrap());
        assert!(first.await.unwrap().is_err());

        server.abort();
    }

    #[tokio::test]
    async fn stale_cancel_cleanup_does_not_remove_a_reused_request_id() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let request_id = format!("reused-test-{}", Uuid::new_v4());
        let (first_accepted_tx, first_accepted_rx) = tokio::sync::oneshot::channel();

        let server = tokio::spawn(async move {
            let (_first_stream, _) = listener.accept().await.unwrap();
            let _ = first_accepted_tx.send(());
            let (_second_stream, _) = listener.accept().await.unwrap();
            tokio::time::sleep(Duration::from_secs(5)).await;
        });

        let first_id = request_id.clone();
        let first = tokio::spawn(async move {
            call_llm_http(request(format!("http://{address}/first"), Some(first_id))).await
        });
        first_accepted_rx.await.unwrap();

        assert!(cancel_llm_request(request_id.clone()).await.unwrap());

        let mut second = Box::pin(call_llm_http(request(
            format!("http://{address}/second"),
            Some(request_id.clone()),
        )));
        std::future::poll_fn(|cx| match second.as_mut().poll(cx) {
            std::task::Poll::Pending => std::task::Poll::Ready(()),
            std::task::Poll::Ready(result) => {
                panic!("replacement request completed unexpectedly: {result:?}")
            }
        })
        .await;

        tokio::task::yield_now().await;
        let second_cancelled = cancel_llm_request(request_id).await.unwrap();
        server.abort();

        assert!(
            second_cancelled,
            "cleanup from the first request must not remove the replacement registration"
        );
        assert!(second.await.is_err());
        assert!(first.await.unwrap().is_err());
    }
}
