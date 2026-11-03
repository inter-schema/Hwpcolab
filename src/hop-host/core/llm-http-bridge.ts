export const DEFAULT_LLM_TIMEOUT_MS = 180_000;
export const MIN_LLM_TIMEOUT_MS = 1_000;
export const MAX_LLM_TIMEOUT_MS = 600_000;
export const MAX_LLM_REQUEST_BODY_BYTES = 20 * 1024 * 1024;
export const MAX_LLM_RESPONSE_BODY_BYTES = 32 * 1024 * 1024;

export function validateLlmEndpoint(rawUrl: string): URL {
  const url = new URL(rawUrl);
  if (url.protocol !== 'http:' && url.protocol !== 'https:') {
    throw new Error('LLM endpoint must use HTTP or HTTPS.');
  }
  if (url.username || url.password) {
    throw new Error('LLM endpoint URLs must not contain userinfo.');
  }
  return url;
}

export function serializeBoundedRequestBody(
  body: unknown,
  maxBytes = MAX_LLM_REQUEST_BODY_BYTES,
): string {
  const serialized = JSON.stringify(body);
  if (serialized === undefined) {
    throw new Error('LLM request body is not JSON serializable.');
  }
  const byteLength = new TextEncoder().encode(serialized).byteLength;
  if (byteLength > maxBytes) {
    throw new Error(`LLM request body exceeds the ${maxBytes}-byte limit.`);
  }
  return serialized;
}

export function normalizeLlmTimeoutMs(requested?: number): number {
  const timeout = typeof requested === 'number' && Number.isFinite(requested)
    ? Math.trunc(requested)
    : DEFAULT_LLM_TIMEOUT_MS;
  return Math.min(MAX_LLM_TIMEOUT_MS, Math.max(MIN_LLM_TIMEOUT_MS, timeout));
}

export async function readBoundedResponseText(
  response: Response,
  maxBytes = MAX_LLM_RESPONSE_BODY_BYTES,
): Promise<string> {
  const contentLengthHeader = response.headers.get('content-length');
  if (contentLengthHeader) {
    const contentLength = Number(contentLengthHeader);
    if (Number.isFinite(contentLength) && contentLength > maxBytes) {
      throw new Error(`LLM response body exceeds the ${maxBytes}-byte limit.`);
    }
  }

  const reader = response.body?.getReader();
  if (!reader) return '';

  const chunks: Uint8Array[] = [];
  let totalBytes = 0;
  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      if (!value) continue;

      totalBytes += value.byteLength;
      if (totalBytes > maxBytes) {
        try {
          await reader.cancel();
        } catch {
          // Preserve the size-limit error if the stream is already closed.
        }
        throw new Error(`LLM response body exceeds the ${maxBytes}-byte limit.`);
      }
      chunks.push(value);
    }
  } finally {
    reader.releaseLock();
  }

  const bytes = new Uint8Array(totalBytes);
  let offset = 0;
  for (const chunk of chunks) {
    bytes.set(chunk, offset);
    offset += chunk.byteLength;
  }

  const text = new TextDecoder().decode(bytes);
  return text.charCodeAt(0) === 0xfeff ? text.slice(1) : text;
}
