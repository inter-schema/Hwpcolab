use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use url::{Host, Url};

use crate::error::AiError;
use crate::types::BoxFuture;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EndpointValidationResult {
    Ok { normalized: String },
    Err { reason: String },
}

impl EndpointValidationResult {
    pub fn is_ok(&self) -> bool {
        matches!(self, Self::Ok { .. })
    }

    pub fn normalized(&self) -> Option<&str> {
        match self {
            Self::Ok { normalized } => Some(normalized.as_str()),
            Self::Err { .. } => None,
        }
    }

    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::Ok { .. } => None,
            Self::Err { reason } => Some(reason.as_str()),
        }
    }
}

fn parse_ipv4(hostname: &str) -> Option<[u8; 4]> {
    let parts: Vec<&str> = hostname.split('.').collect();
    if parts.len() != 4 {
        return None;
    }
    let mut nums = [0u8; 4];
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() || !part.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        if part.len() > 1 && part.starts_with('0') {
            return None;
        }
        let n: u8 = part.parse().ok()?;
        nums[i] = n;
    }
    Some(nums)
}

fn is_link_local_ipv4(ipv4: [u8; 4]) -> bool {
    ipv4[0] == 169 && ipv4[1] == 254
}

fn is_private_or_loopback_ipv4(ipv4: [u8; 4]) -> bool {
    // 127.0.0.0/8
    if ipv4[0] == 127 {
        return true;
    }
    // 10.0.0.0/8
    if ipv4[0] == 10 {
        return true;
    }
    // 172.16.0.0/12 (172.16.0.0 - 172.31.255.255)
    if ipv4[0] == 172 && (16..=31).contains(&ipv4[1]) {
        return true;
    }
    // 192.168.0.0/16
    if ipv4[0] == 192 && ipv4[1] == 168 {
        return true;
    }
    false
}

fn is_loopback_ipv6(hostname: &str) -> bool {
    let clean = hostname
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_ascii_lowercase();
    if clean == "::1" || clean == "0:0:0:0:0:0:0:1" {
        return true;
    }
    clean
        .parse::<Ipv6Addr>()
        .map(|addr| addr.is_loopback())
        .unwrap_or(false)
}

fn is_localhost(hostname: &str) -> bool {
    let lower = hostname.to_ascii_lowercase();
    lower == "localhost" || lower.ends_with(".localhost")
}

/// Checks whether an IP address falls into the forbidden `169.254.0.0/16`
/// link-local / cloud metadata range (including IPv4-mapped IPv6 `::ffff:169.254.0.0/16`).
pub fn is_forbidden_link_local_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_link_local_ipv4(v4.octets()),
        IpAddr::V6(v6) => {
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return is_link_local_ipv4(mapped.octets());
            }
            if let Some(compat) = v6.to_ipv4() {
                if v6.segments()[..6] == [0, 0, 0, 0, 0, 0] {
                    return is_link_local_ipv4(compat.octets());
                }
            }
            false
        }
    }
}

/// Validates and normalizes a custom OpenAI-compatible endpoint URL.
/// Ported from `src/services/agent/endpointPolicy.ts`.
pub fn validate_custom_endpoint(raw_url: &str) -> EndpointValidationResult {
    let trimmed = raw_url.trim();
    if trimmed.is_empty() {
        return EndpointValidationResult::Err {
            reason: "엔드포인트 URL을 입력해주세요.".to_string(),
        };
    }

    let parsed = match Url::parse(trimmed) {
        Ok(u) => u,
        Err(_) => {
            return EndpointValidationResult::Err {
                reason: "유효하지 않은 URL 형식입니다.".to_string(),
            };
        }
    };

    let scheme = parsed.scheme().to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return EndpointValidationResult::Err {
            reason: "http 또는 https 프로토콜만 허용됩니다.".to_string(),
        };
    }

    if !parsed.username().is_empty() || parsed.password().is_some() {
        return EndpointValidationResult::Err {
            reason: "URL에 사용자 자격증명(username/password)을 포함할 수 없습니다.".to_string(),
        };
    }

    let hostname = match parsed.host_str() {
        Some(h) if !h.is_empty() => h,
        _ => {
            return EndpointValidationResult::Err {
                reason: "유효하지 않은 URL 형식입니다.".to_string(),
            };
        }
    };

    let ipv4 = parse_ipv4(hostname);
    let ipv6_link_local = hostname
        .trim_start_matches('[')
        .trim_end_matches(']')
        .parse::<Ipv6Addr>()
        .map(|addr| is_forbidden_link_local_ip(IpAddr::V6(addr)))
        .unwrap_or(false);

    if ipv4.is_some_and(is_link_local_ipv4) || ipv6_link_local {
        return EndpointValidationResult::Err {
            reason: "169.254.0.0/16 링크로컬/메타데이터 주소는 허용되지 않습니다.".to_string(),
        };
    }

    if scheme == "http" {
        let is_allowed_http_host = is_localhost(hostname)
            || is_loopback_ipv6(hostname)
            || ipv4.is_some_and(is_private_or_loopback_ipv4);

        if !is_allowed_http_host {
            return EndpointValidationResult::Err {
                reason: "http 프로토콜은 localhost, 루프백(127.0.0.1, ::1) 또는 사설망(10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16)에서만 허용됩니다. 공개 인터넷 호스트는 https를 사용해주세요.".to_string(),
            };
        }
    }

    let origin = parsed.origin().ascii_serialization();
    let trimmed_path = parsed.path().trim_end_matches('/');
    let mut normalized = format!("{}{}", origin, trimmed_path);
    if !normalized.contains("/chat/completions") && !normalized.contains("/responses") {
        normalized.push_str("/chat/completions");
    }
    if let Some(query) = parsed.query() {
        if !query.is_empty() {
            normalized.push('?');
            normalized.push_str(query);
        }
    }

    EndpointValidationResult::Ok { normalized }
}

/// DNS resolution abstraction so the post-resolution `169.254.0.0/16` guard
/// can be verified both with OS DNS and deterministic mock resolvers in tests.
pub trait DnsResolver: Send + Sync {
    fn resolve<'a>(&'a self, host: &'a str, port: u16) -> BoxFuture<'a, Result<Vec<SocketAddr>, String>>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemDnsResolver;

impl DnsResolver for SystemDnsResolver {
    fn resolve<'a>(&'a self, host: &'a str, port: u16) -> BoxFuture<'a, Result<Vec<SocketAddr>, String>> {
        Box::pin(async move {
            let addrs = tokio::net::lookup_host((host, port))
                .await
                .map_err(|e| format!("DNS 조회 실패 ({host}:{port}): {e}"))?;
            Ok(addrs.collect())
        })
    }
}

/// Resolves the URL host and enforces that neither the literal IP nor any
/// DNS-resolved IP address falls inside `169.254.0.0/16` (including IPv4-mapped IPv6).
/// Returns the validated `SocketAddr` list for connection pinning.
pub async fn resolve_and_guard_endpoint(
    url: &Url,
    resolver: &dyn DnsResolver,
) -> Result<Vec<SocketAddr>, AiError> {
    let port = url
        .port_or_known_default()
        .ok_or_else(|| AiError::EndpointPolicy("LLM endpoint 포트를 확인할 수 없습니다.".into()))?;

    let host = url
        .host()
        .ok_or_else(|| AiError::EndpointPolicy("LLM endpoint 호스트가 비어 있습니다.".into()))?;

    match host {
        Host::Ipv4(v4) => {
            if is_forbidden_link_local_ip(IpAddr::V4(v4)) {
                return Err(AiError::EndpointPolicy(
                    "169.254.0.0/16 링크로컬/메타데이터 주소는 허용되지 않습니다.".into(),
                ));
            }
            Ok(vec![SocketAddr::new(IpAddr::V4(v4), port)])
        }
        Host::Ipv6(v6) => {
            if is_forbidden_link_local_ip(IpAddr::V6(v6)) {
                return Err(AiError::EndpointPolicy(
                    "169.254.0.0/16 링크로컬/메타데이터 주소는 허용되지 않습니다.".into(),
                ));
            }
            Ok(vec![SocketAddr::new(IpAddr::V6(v6), port)])
        }
        Host::Domain(domain) => {
            if let Ok(v4) = domain.parse::<Ipv4Addr>() {
                if is_forbidden_link_local_ip(IpAddr::V4(v4)) {
                    return Err(AiError::EndpointPolicy(
                        "169.254.0.0/16 링크로컬/메타데이터 주소는 허용되지 않습니다.".into(),
                    ));
                }
                return Ok(vec![SocketAddr::new(IpAddr::V4(v4), port)]);
            }

            let resolved = resolver
                .resolve(domain, port)
                .await
                .map_err(AiError::HttpProxy)?;

            if resolved.is_empty() {
                return Err(AiError::HttpProxy(format!(
                    "DNS 조회 결과가 비어 있습니다: {domain}"
                )));
            }

            for addr in &resolved {
                if is_forbidden_link_local_ip(addr.ip()) {
                    return Err(AiError::EndpointPolicy(format!(
                        "DNS 조회 결과({domain} -> {})가 169.254.0.0/16 링크로컬/메타데이터 대역에 해당하여 차단되었습니다.",
                        addr.ip()
                    )));
                }
            }

            Ok(resolved)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockDnsResolver {
        addrs: Vec<SocketAddr>,
    }

    impl DnsResolver for MockDnsResolver {
        fn resolve<'a>(&'a self, _host: &'a str, _port: u16) -> BoxFuture<'a, Result<Vec<SocketAddr>, String>> {
            Box::pin(async move { Ok(self.addrs.clone()) })
        }
    }

    #[test]
    fn allows_ipv6_loopback() {
        let res = validate_custom_endpoint("http://[::1]:11434");
        assert_eq!(
            res,
            EndpointValidationResult::Ok {
                normalized: "http://[::1]:11434/chat/completions".to_string()
            }
        );
    }

    #[test]
    fn rejects_172_15_on_http() {
        let res = validate_custom_endpoint("http://172.15.0.1:8000/v1");
        assert!(!res.is_ok());
    }

    #[test]
    fn allows_172_16_on_http() {
        let res = validate_custom_endpoint("http://172.16.0.1:8000/v1");
        assert_eq!(
            res.normalized(),
            Some("http://172.16.0.1:8000/v1/chat/completions")
        );
    }

    #[test]
    fn allows_172_31_on_http() {
        let res = validate_custom_endpoint("http://172.31.255.1:8080");
        assert_eq!(
            res.normalized(),
            Some("http://172.31.255.1:8080/chat/completions")
        );
    }

    #[test]
    fn rejects_172_32_on_http() {
        let res = validate_custom_endpoint("http://172.32.0.1:8080");
        assert!(!res.is_ok());
    }

    #[test]
    fn allows_192_168_on_http() {
        let res = validate_custom_endpoint("http://192.168.1.100:11434/v1");
        assert_eq!(
            res.normalized(),
            Some("http://192.168.1.100:11434/v1/chat/completions")
        );
    }

    #[test]
    fn allows_10_x_on_http() {
        let res = validate_custom_endpoint("http://10.0.1.5:5000");
        assert_eq!(
            res.normalized(),
            Some("http://10.0.1.5:5000/chat/completions")
        );
    }

    #[test]
    fn rejects_169_254_on_http_and_https() {
        let res_http = validate_custom_endpoint("http://169.254.169.254/latest/meta-data");
        assert!(!res_http.is_ok());
        assert!(res_http.reason().unwrap().contains("169.254"));

        let res_https = validate_custom_endpoint("https://169.254.169.254/api");
        assert!(!res_https.is_ok());
        assert!(res_https.reason().unwrap().contains("169.254"));
    }

    #[test]
    fn allows_localhost_on_http() {
        let res = validate_custom_endpoint("http://localhost:11434");
        assert_eq!(
            res.normalized(),
            Some("http://localhost:11434/chat/completions")
        );
    }

    #[test]
    fn handles_uppercase_hosts_and_schemes() {
        let res = validate_custom_endpoint("HTTP://LOCALHOST:11434/V1");
        assert_eq!(
            res.normalized(),
            Some("http://localhost:11434/V1/chat/completions")
        );
    }

    #[test]
    fn handles_custom_ports_on_https() {
        let res = validate_custom_endpoint("https://api.custom-ai.org:8443/v1");
        assert_eq!(
            res.normalized(),
            Some("https://api.custom-ai.org:8443/v1/chat/completions")
        );
    }

    #[test]
    fn rejects_userinfo_in_url() {
        let res = validate_custom_endpoint("http://admin:secret@localhost:11434/v1");
        assert!(!res.is_ok());
        assert!(res.reason().unwrap().contains("자격증명"));
    }

    #[test]
    fn rejects_non_http_schemes() {
        assert!(!validate_custom_endpoint("ftp://localhost:21/api").is_ok());
        assert!(!validate_custom_endpoint("file:///etc/passwd").is_ok());
        assert!(!validate_custom_endpoint("ws://localhost:8000").is_ok());
    }

    #[test]
    fn does_not_duplicate_chat_completions() {
        let res = validate_custom_endpoint("https://api.example.com/v1/chat/completions");
        assert_eq!(
            res.normalized(),
            Some("https://api.example.com/v1/chat/completions")
        );
    }

    #[test]
    fn allows_127_0_0_1_loopback() {
        let res = validate_custom_endpoint("http://127.0.0.1:8080/v1");
        assert_eq!(
            res.normalized(),
            Some("http://127.0.0.1:8080/v1/chat/completions")
        );
    }

    #[tokio::test]
    async fn post_dns_guard_blocks_169_254_resolution_and_ipv4_mapped_v6() {
        let url = Url::parse("https://rebind.evil.test/v1/chat/completions").unwrap();

        let bad_v4 = MockDnsResolver {
            addrs: vec!["169.254.169.254:443".parse().unwrap()],
        };
        let err = resolve_and_guard_endpoint(&url, &bad_v4).await.unwrap_err();
        assert!(matches!(err, AiError::EndpointPolicy(_)));
        assert!(err.to_string().contains("169.254.0.0/16"));

        let bad_mixed = MockDnsResolver {
            addrs: vec![
                "93.184.216.34:443".parse().unwrap(),
                "[::ffff:169.254.169.254]:443".parse().unwrap(),
            ],
        };
        let err_v6 = resolve_and_guard_endpoint(&url, &bad_mixed)
            .await
            .unwrap_err();
        assert!(matches!(err_v6, AiError::EndpointPolicy(_)));

        let good = MockDnsResolver {
            addrs: vec!["93.184.216.34:443".parse().unwrap()],
        };
        let resolved = resolve_and_guard_endpoint(&url, &good).await.unwrap();
        assert_eq!(resolved.len(), 1);
    }
}
