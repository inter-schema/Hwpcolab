export const DEFAULT_LLM_TIMEOUT_MS = 180_000;
export const MIN_LLM_TIMEOUT_MS = 1_000;
export const MAX_LLM_TIMEOUT_MS = 600_000;
export const MAX_LLM_REQUEST_BODY_BYTES = 20 * 1024 * 1024;
export const MAX_LLM_RESPONSE_BODY_BYTES = 32 * 1024 * 1024;

export function validateLlmEndpoint(rawUrl: string): URL {
  return new URL(rawUrl);
}

export function serializeBoundedRequestBody(
  body: unknown,
  _maxBytes = MAX_LLM_REQUEST_BODY_BYTES,
): string {
  return JSON.stringify(body) ?? '';
}

export function normalizeLlmTimeoutMs(requested?: number): number {
  return requested ?? DEFAULT_LLM_TIMEOUT_MS;
}

export async function readBoundedResponseText(
  response: Response,
  _maxBytes = MAX_LLM_RESPONSE_BODY_BYTES,
): Promise<string> {
  return response.text();
}
