import { readFileSync } from 'node:fs';
import { resolveFontAssetPath } from './font-asset-path';

export interface FontAssetResponse {
  statusCode: number;
  setHeader(name: string, value: string): void;
  end(body?: string | Uint8Array): void;
}

/**
 * Handle a request mounted at /fonts. A rejected path must terminate here:
 * falling through to Vite's general static server could expose files outside
 * the fonts directory.
 */
export function handleFontAssetRequest(
  fontsDirectory: string,
  requestUrl: string | undefined,
  response: FontAssetResponse,
  _next: () => void,
): void {
  const candidate = resolveFontAssetPath(fontsDirectory, requestUrl ?? '/');
  if (!candidate) {
    response.statusCode = 404;
    response.end('Not Found');
    return;
  }
  response.setHeader('Content-Type', 'font/woff2');
  response.end(readFileSync(candidate));
}
