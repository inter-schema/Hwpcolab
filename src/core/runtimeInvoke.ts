import { invoke as tauriInvoke } from "@tauri-apps/api/core";

export function isTauriEnv(): boolean {
  return (
    typeof window !== "undefined" &&
    Boolean((window as any).__TAURI_INTERNALS__ || (window as any).__TAURI__)
  );
}

/**
 * Tauri 데스크톱 런타임에서는 네이티브 Tauri invoke를 호출하고,
 * Vite 개발 서버 / Playwright E2E 브라우저 환경에서는 로컬 Node.js 브리지(/__hop_claw_api/*)를 호출하여
 * 동일한 claw-hwp 스크립트 실행, 스테이징 파일 관리, LLM 프록시를 100% 동일하게 수행합니다.
 */
export async function runtimeInvoke<T = any>(cmd: string, args: Record<string, any> = {}): Promise<T> {
  if (isTauriEnv()) {
    return tauriInvoke<T>(cmd, args);
  }

  const resp = await fetch(`/__hop_claw_api/${encodeURIComponent(cmd)}`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(args),
  });

  if (!resp.ok) {
    const errText = await resp.text();
    throw new Error(`Local bridge error (${cmd}, ${resp.status}): ${errText}`);
  }

  const data = await resp.json();
  if (data && data.__hop_error) {
    throw new Error(data.__hop_error);
  }
  return data.result as T;
}
