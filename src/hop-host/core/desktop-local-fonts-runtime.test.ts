import { afterEach, describe, expect, it, vi } from 'vitest';

describe('isDesktopTauriRuntime', () => {
  afterEach(() => {
    vi.resetModules();
    delete (globalThis as { window?: unknown }).window;
  });

  it('recognizes the Tauri v2 runtime marker', async () => {
    (globalThis as { window?: unknown }).window = { __TAURI_INTERNALS__: {} };

    const { isDesktopTauriRuntime } = await import('./desktop-local-fonts');

    expect(isDesktopTauriRuntime()).toBe(true);
  });

  it('recognizes the tauri protocol fallback', async () => {
    (globalThis as { window?: unknown }).window = { location: { protocol: 'tauri:' } };

    const { isDesktopTauriRuntime } = await import('./desktop-local-fonts');

    expect(isDesktopTauriRuntime()).toBe(true);
  });

  it('does not mistake an ordinary browser for Tauri', async () => {
    (globalThis as { window?: unknown }).window = { location: { protocol: 'https:' } };

    const { isDesktopTauriRuntime } = await import('./desktop-local-fonts');

    expect(isDesktopTauriRuntime()).toBe(false);
  });

  it('returns false when no window exists', async () => {
    const { isDesktopTauriRuntime } = await import('./desktop-local-fonts');

    expect(isDesktopTauriRuntime()).toBe(false);
  });
});
