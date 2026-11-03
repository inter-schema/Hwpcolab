import { describe, expect, it } from 'vitest';
import {
  DEFAULT_LLM_TIMEOUT_MS,
  MAX_LLM_TIMEOUT_MS,
  MIN_LLM_TIMEOUT_MS,
  normalizeLlmTimeoutMs,
  readBoundedResponseText,
  serializeBoundedRequestBody,
  validateLlmEndpoint,
} from './llm-http-bridge';

describe('Vite LLM HTTP bridge limits', () => {
  it('allows HTTP(S) endpoints, including local providers, but rejects other schemes and userinfo', () => {
    expect(validateLlmEndpoint('http://127.0.0.1:11434/v1/chat').hostname).toBe('127.0.0.1');
    expect(validateLlmEndpoint('https://api.example.test/v1/chat').protocol).toBe('https:');
    expect(() => validateLlmEndpoint('file:///etc/passwd')).toThrow();
    expect(() => validateLlmEndpoint('https://user:secret@example.test/v1/chat')).toThrow();
  });

  it('bounds serialized request bodies by UTF-8 byte length', () => {
    expect(serializeBoundedRequestBody({ value: 'abc' }, 20)).toBe('{"value":"abc"}');
    expect(() => serializeBoundedRequestBody({ value: 'ééé' }, 10)).toThrow(/size|limit|bytes/i);
  });

  it('clamps request timeouts to the native bridge range', () => {
    expect(normalizeLlmTimeoutMs()).toBe(DEFAULT_LLM_TIMEOUT_MS);
    expect(normalizeLlmTimeoutMs(0)).toBe(MIN_TIMEOUT_MS);
    expect(normalizeLlmTimeoutMs(Number.MAX_SAFE_INTEGER)).toBe(MAX_LLM_TIMEOUT_MS);
    expect(normalizeLlmTimeoutMs(30_000)).toBe(30_000);
  });

  it('rejects an oversized Content-Length before reading the response', async () => {
    const response = new Response('12345', { headers: { 'content-length': '5' } });
    await expect(readBoundedResponseText(response, 4)).rejects.toThrow(/size|limit|bytes/i);
  });

  it('enforces the limit while streaming when Content-Length is absent', async () => {
    const body = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(new TextEncoder().encode('123'));
        controller.enqueue(new TextEncoder().encode('456'));
        controller.close();
      },
    });
    await expect(readBoundedResponseText(new Response(body), 4)).rejects.toThrow(/size|limit|bytes/i);
  });

  it('strips a UTF-8 BOM from a bounded response', async () => {
    const response = new Response('\uFEFF{"ok":true}');
    await expect(readBoundedResponseText(response)).resolves.toBe('{"ok":true}');
  });
});
