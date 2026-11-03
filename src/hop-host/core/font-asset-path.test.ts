import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  realpathSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';
import { resolveFontAssetPath } from './font-asset-path';

const temporaryDirectories: string[] = [];

function fixture() {
  const root = mkdtempSync(join(tmpdir(), 'hwpcolab-fonts-'));
  temporaryDirectories.push(root);
  const fonts = join(root, 'fonts');
  const sibling = join(root, 'fonts-private');
  mkdirSync(fonts);
  mkdirSync(sibling);
  writeFileSync(join(fonts, 'regular.woff2'), 'font');
  writeFileSync(join(fonts, 'nested.woff2'), 'nested-font');
  writeFileSync(join(fonts, 'wrong.ttf'), 'not-woff2');
  writeFileSync(join(sibling, 'secret.woff2'), 'secret');
  return { root, fonts, sibling };
}

afterEach(() => {
  for (const directory of temporaryDirectories.splice(0)) {
    rmSync(directory, { recursive: true, force: true });
  }
});

describe('font asset path containment', () => {
  it('resolves a regular WOFF2 file and ignores the query string', () => {
    const { fonts } = fixture();
    expect(resolveFontAssetPath(fonts, '/regular.woff2?v=1'))
      .toBe(realpathSync(join(fonts, 'regular.woff2')));
  });

  it('resolves nested WOFF2 paths', () => {
    const { fonts } = fixture();
    const nested = join(fonts, 'nested');
    mkdirSync(nested);
    writeFileSync(join(nested, 'font.woff2'), 'nested');
    expect(resolveFontAssetPath(fonts, '/nested/font.woff2'))
      .toBe(realpathSync(join(nested, 'font.woff2')));
  });

  it('rejects parent traversal and sibling-prefix directories', () => {
    const { fonts } = fixture();
    expect(resolveFontAssetPath(fonts, '/../fonts-private/secret.woff2')).toBeNull();
    expect(resolveFontAssetPath(fonts, '/../../secret.woff2')).toBeNull();
  });

  it('rejects malformed percent-encoding without throwing', () => {
    const { fonts } = fixture();
    expect(resolveFontAssetPath(fonts, '/bad%2.woff2')).toBeNull();
    expect(resolveFontAssetPath(fonts, '/bad%ZZ.woff2')).toBeNull();
  });

  it('rejects missing files and non-WOFF2 extensions', () => {
    const { fonts } = fixture();
    expect(resolveFontAssetPath(fonts, '/missing.woff2')).toBeNull();
    expect(resolveFontAssetPath(fonts, '/wrong.ttf')).toBeNull();
  });

  it('rejects symlinks that resolve outside the font root', () => {
    const { fonts, sibling } = fixture();
    const link = join(fonts, 'escape.woff2');
    try {
      symlinkSync(join(sibling, 'secret.woff2'), link);
    } catch {
      // Platforms that prohibit symlink creation cannot run this assertion.
      return;
    }
    expect(existsSync(link)).toBe(true);
    expect(resolveFontAssetPath(fonts, '/escape.woff2')).toBeNull();
  });
});
