import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';

const configPath = resolve(
  dirname(fileURLToPath(import.meta.url)),
  '../../src-tauri/tauri.conf.json',
);
const config = JSON.parse(readFileSync(configPath, 'utf8'));
const security = config.app.security;

describe('Tauri Content Security Policy', () => {
  it('enables a restrictive production CSP instead of disabling CSP', () => {
    expect(security.csp).not.toBeNull();
    expect(security.csp['default-src']).toContain("'self'");
    expect(security.csp['default-src']).toContain('customprotocol:');
    expect(security.csp['script-src']).toContain("'wasm-unsafe-eval'");
    expect(security.csp['script-src']).not.toContain("'unsafe-eval'");
    expect(security.csp['connect-src']).toContain('ipc:');
    expect(security.csp['connect-src']).not.toContain('http://localhost:1420');
    expect(security.csp['object-src']).toBe("'none'");
  });

  it('allows only the Vite dev server and HMR websocket in development', () => {
    expect(security.devCsp['connect-src']).toContain('http://localhost:1420');
    expect(security.devCsp['connect-src']).toContain('ws://localhost:1420');
    expect(security.devCsp['object-src']).toBe("'none'");
  });
});
