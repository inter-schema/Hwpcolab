import { realpathSync, statSync } from 'node:fs';
import { extname, isAbsolute, relative, resolve, sep } from 'node:path';

function isWithinDirectory(root: string, candidate: string): boolean {
  const pathFromRoot = relative(root, candidate);
  return pathFromRoot !== ''
    && pathFromRoot !== '..'
    && !pathFromRoot.startsWith('..' + sep)
    && !isAbsolute(pathFromRoot);
}

/**
 * Resolve one raw /fonts request URL to a canonical WOFF2 asset below the
 * configured font directory. Returns null for malformed encodings, traversal,
 * sibling-prefix paths, symlink escapes, missing files and non-WOFF2 files.
 */
export function resolveFontAssetPath(fontsDirectory: string, rawRequestUrl: string): string | null {
  try {
    const rawPath = (rawRequestUrl ?? '/').split('?')[0];
    const decodedPath = decodeURIComponent(rawPath).replace(/^\/+/, '');
    if (!decodedPath || decodedPath.includes('\0')) return null;

    const canonicalRoot = realpathSync(fontsDirectory);
    const lexicalCandidate = resolve(canonicalRoot, decodedPath);
    if (!isWithinDirectory(canonicalRoot, lexicalCandidate)) return null;

    const canonicalCandidate = realpathSync(lexicalCandidate);
    if (!isWithinDirectory(canonicalRoot, canonicalCandidate)) return null;
    if (extname(canonicalCandidate).toLowerCase() !== '.woff2') return null;
    if (!statSync(canonicalCandidate).isFile()) return null;

    return canonicalCandidate;
  } catch {
    return null;
  }
}
