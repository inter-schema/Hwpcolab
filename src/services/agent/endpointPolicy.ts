export type EndpointValidationResult =
  | { ok: true; normalized: string }
  | { ok: false; reason: string };

function parseIpv4(hostname: string): number[] | null {
  const parts = hostname.split(".");
  if (parts.length !== 4) return null;
  const nums: number[] = [];
  for (const part of parts) {
    if (!/^\d+$/.test(part)) return null;
    const n = Number(part);
    if (n < 0 || n > 255 || (part.length > 1 && part.startsWith("0"))) {
      return null;
    }
    nums.push(n);
  }
  return nums;
}

function isLinkLocalIpv4(ipv4: number[]): boolean {
  return ipv4[0] === 169 && ipv4[1] === 254;
}

function isPrivateOrLoopbackIpv4(ipv4: number[]): boolean {
  // 127.0.0.0/8
  if (ipv4[0] === 127) return true;
  // 10.0.0.0/8
  if (ipv4[0] === 10) return true;
  // 172.16.0.0/12 (172.16.0.0 - 172.31.255.255)
  if (ipv4[0] === 172 && ipv4[1] >= 16 && ipv4[1] <= 31) return true;
  // 192.168.0.0/16
  if (ipv4[0] === 192 && ipv4[1] === 168) return true;
  return false;
}

function isLoopbackIpv6(hostname: string): boolean {
  const clean = hostname.replace(/^\[|\]$/g, "").toLowerCase();
  return clean === "::1" || clean === "0:0:0:0:0:0:0:1";
}

function isLocalhost(hostname: string): boolean {
  const lower = hostname.toLowerCase();
  return lower === "localhost" || lower.endsWith(".localhost");
}

/**
 * 사용자 지정 엔드포인트 URL을 검증하고 정규화합니다.
 * - https:// 는 원칙적으로 허용 (단, 169.254/16 메타데이터 IP는 거부)
 * - http:// 는 loopback (localhost, 127.0.0.0/8, ::1) 및 사설망 (10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16)만 허용
 * - 169.254.0.0/16 (링크로컬/클라우드 메타데이터) 거부
 * - URL 내 userinfo(username/password) 거부
 * - `/chat/completions` 자동 보정 (normalized 필드에 반영)
 */
export function validateCustomEndpoint(rawUrl: string): EndpointValidationResult {
  if (!rawUrl || typeof rawUrl !== "string" || !rawUrl.trim()) {
    return { ok: false, reason: "엔드포인트 URL을 입력해주세요." };
  }

  const trimmed = rawUrl.trim();
  let parsed: URL;
  try {
    parsed = new URL(trimmed);
  } catch {
    return { ok: false, reason: "유효하지 않은 URL 형식입니다." };
  }

  // 프로토콜 검사
  const protocol = parsed.protocol.toLowerCase();
  if (protocol !== "http:" && protocol !== "https:") {
    return { ok: false, reason: "http 또는 https 프로토콜만 허용됩니다." };
  }

  // 자격증명(userinfo) 거부
  if (parsed.username || parsed.password) {
    return { ok: false, reason: "URL에 사용자 자격증명(username/password)을 포함할 수 없습니다." };
  }

  const hostname = parsed.hostname;
  const ipv4 = parseIpv4(hostname);

  // 169.254.0.0/16 링크로컬/메타데이터 주소는 프로토콜 무관하게 거부
  if (ipv4 && isLinkLocalIpv4(ipv4)) {
    return { ok: false, reason: "169.254.0.0/16 링크로컬/메타데이터 주소는 허용되지 않습니다." };
  }

  if (protocol === "http:") {
    const isAllowedHttpHost =
      isLocalhost(hostname) ||
      isLoopbackIpv6(hostname) ||
      (ipv4 !== null && isPrivateOrLoopbackIpv4(ipv4));

    if (!isAllowedHttpHost) {
      return {
        ok: false,
        reason:
          "http 프로토콜은 localhost, 루프백(127.0.0.1, ::1) 또는 사설망(10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16)에서만 허용됩니다. 공개 인터넷 호스트는 https를 사용해주세요.",
      };
    }
  }

  // URL 정규화 (/chat/completions 자동 보정)
  let normalized = parsed.origin + parsed.pathname.replace(/\/+$/, "");
  if (!normalized.includes("/chat/completions") && !normalized.includes("/responses")) {
    normalized += "/chat/completions";
  }
  if (parsed.search) {
    normalized += parsed.search;
  }

  return { ok: true, normalized };
}
