import type { ProviderAdapter, ProviderConfig } from "../types.ts";
import { validateCustomEndpoint } from "../endpointPolicy.ts";
import { ClaudeAdapter } from "./claude.ts";
import { OpenAiCompatAdapter } from "./openaiCompat.ts";

export { ClaudeAdapter } from "./claude.ts";
export { OpenAiCompatAdapter } from "./openaiCompat.ts";

export function createProviderAdapter(cfg: ProviderConfig): ProviderAdapter {
  switch (cfg.provider) {
    case "claude":
      return new ClaudeAdapter(cfg);

    case "openai":
      return new OpenAiCompatAdapter({
        ...cfg,
        endpoint: cfg.endpoint || "https://api.openai.com/v1/chat/completions",
      }, "openai");

    case "baze":
      return new OpenAiCompatAdapter({
        ...cfg,
        endpoint: cfg.endpoint || "https://api.baze.co/v1/gateway/chat/completions",
      }, "baze");

    case "ollama":
      return new OpenAiCompatAdapter({
        ...cfg,
        endpoint: cfg.endpoint || "http://127.0.0.1:11434/v1/chat/completions",
      }, "ollama");

    case "gemini":
      return new OpenAiCompatAdapter({
        ...cfg,
        endpoint: cfg.endpoint || "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions",
      }, "gemini");

    case "custom": {
      if (!cfg.endpoint) {
        throw new Error("사용자 지정 공급자(custom)는 엔드포인트 URL이 필수입니다.");
      }
      const val = validateCustomEndpoint(cfg.endpoint);
      if (!val.ok) {
        throw new Error(`사용자 지정 공급자 엔드포인트 검증 실패: ${val.reason}`);
      }
      return new OpenAiCompatAdapter({
        ...cfg,
        endpoint: val.normalized,
        customSupportsTools: cfg.customSupportsTools ?? true,
      }, "custom");
    }

    default:
      throw new Error(`지원되지 않는 공급자입니다: ${(cfg as any).provider}`);
  }
}
