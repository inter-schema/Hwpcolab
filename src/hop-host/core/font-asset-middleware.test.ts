import {
  mkdirSync,
  mkdtempSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';
import { handleFontAssetRequest } from './font-asset-middleware';

const roots: string[] = [];

function fixture() {
  const root = mkdtempSync(join(tmpdir(), 'hwpcolab-font-middleware-'));
  roots.push(root);
  const fonts = join(root, 'fonts');
  const sibling = join(root, 'private');
  mkdirSync(fonts);
  mkdirSync(sibling);
  writeFileSync(join(fonts, 'regular.woff2'), 'font-bytes');
  writeFileSync(join(sibling, 'secret.txt'), 'must-not-be-served');
  return { fonts, sibling };
}

afterEach(() => {
  for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true });
});

function responseRecorder() {
  const headers = new Map<string, string>();
  let body: string | Uint8Array | undefined;
  const response = {
    statusCode: 200,
    setHeader(name: string, value: string) { headers.set(name, value); },
    end(value?: string | Uint8Array) { body = value; },
  };
  return { response, headers, get body() { return body; } };
}

describe('Vite font asset middleware', () => {
  it('serves a valid WOFF2 asset with the expected content type', () => {
    const { fonts } = fixture();
    const recorder = responseRecorder();
    let nextCalled = false;

    handleFontAssetRequest(fonts, '/regular.woff2?v=1', recorder.response, () => {
      nextCalled = true;
    });

    expect(nextCalled).toBe(false);
    expect(recorder.headers.get('Content-Type')).toBe('font/woff2');
    expect(Buffer.from(recorder.body as Uint8Array).toString()).toBe('font-bytes');
  });

  it('terminates rejected traversal requests instead of falling through to Vite static serving', () => {
    const { fonts } = fixture();
    const recorder = responseRecorder();
    let nextCalled = false;

    handleFontAssetRequest(fonts, '/../private/secret.txt', recorder.response, () => {
      nextCalled = true;
    });

    expect(nextCalled).toBe(false);
    expect(recorder.response.statusCode).toBe(404);
    expect(recorder.body).toBe('Not Found');
  });
});
