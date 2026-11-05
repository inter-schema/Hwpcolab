export const HWP_FONT_LANG_COUNT = 7;

export interface FontIdResolverWasm {
  findOrCreateFontId(name: string): number;
  findOrCreateFontIdForLang?(lang: number, name: string): number;
}

const resolvedFontNamesByWasm = new WeakMap<object, Map<string, string>>();

export function recordResolvedFontName(
  wasm: unknown,
  lang: number,
  fontId: number,
  fontName: string,
): void {
  if (!wasm || typeof wasm !== 'object' || fontId < 0 || !fontName) return;
  let table = resolvedFontNamesByWasm.get(wasm);
  if (!table) {
    table = new Map<string, string>();
    resolvedFontNamesByWasm.set(wasm, table);
  }
  table.set(`${lang}:${fontId}`, fontName);
}

export function lookupResolvedFontFamilies(
  wasm: unknown,
  fontIds: readonly number[] | undefined,
): string[] | undefined {
  if (!wasm || typeof wasm !== 'object' || !fontIds || fontIds.length < HWP_FONT_LANG_COUNT) {
    return undefined;
  }
  const table = resolvedFontNamesByWasm.get(wasm);
  if (!table) return undefined;
  const families: string[] = [];
  for (let lang = 0; lang < HWP_FONT_LANG_COUNT; lang++) {
    const name = table.get(`${lang}:${fontIds[lang]}`);
    if (!name) return undefined;
    families.push(name);
  }
  return families;
}

/**
 * Resolves a single font name across all 7 HWP language FaceName tables (0..6).
 * Fixes rhwp#8140 where singular findOrCreateFontId only registered in Korean (slot 0).
 */
export function resolveUniformFontIds(
  wasm: FontIdResolverWasm,
  fontName: string,
): number[] | null {
  if (!fontName) return null;
  if (typeof wasm.findOrCreateFontIdForLang === 'function') {
    const fontIds: number[] = [];
    for (let lang = 0; lang < HWP_FONT_LANG_COUNT; lang++) {
      const id = wasm.findOrCreateFontIdForLang(lang, fontName);
      if (id < 0) return null;
      recordResolvedFontName(wasm, lang, id, fontName);
      fontIds.push(id);
    }
    return fontIds;
  }
  const fallbackId = wasm.findOrCreateFontId(fontName);
  if (fallbackId < 0) return null;
  for (let lang = 0; lang < HWP_FONT_LANG_COUNT; lang++) {
    recordResolvedFontName(wasm, lang, fallbackId, fontName);
  }
  return Array(HWP_FONT_LANG_COUNT).fill(fallbackId) as number[];
}

/**
 * Resolves 7 per-language font names into 7 HWP language FaceName IDs (0..6).
 */
export function resolvePerLangFontIds(
  wasm: FontIdResolverWasm,
  fontFamilies: readonly string[],
  fallbackName = '함초롬바탕',
): number[] | null {
  const normalized = Array.from({ length: HWP_FONT_LANG_COUNT }, (_, lang) =>
    fontFamilies[lang] || fontFamilies[0] || fallbackName,
  );
  if (typeof wasm.findOrCreateFontIdForLang === 'function') {
    const fontIds: number[] = [];
    for (let lang = 0; lang < HWP_FONT_LANG_COUNT; lang++) {
      const id = wasm.findOrCreateFontIdForLang(lang, normalized[lang]);
      if (id < 0) return null;
      recordResolvedFontName(wasm, lang, id, normalized[lang]);
      fontIds.push(id);
    }
    return fontIds;
  }
  const fontIds: number[] = [];
  for (let lang = 0; lang < HWP_FONT_LANG_COUNT; lang++) {
    const id = wasm.findOrCreateFontId(normalized[lang]);
    if (id < 0) return null;
    recordResolvedFontName(wasm, lang, id, normalized[lang]);
    fontIds.push(id);
  }
  return fontIds;
}

/**
 * Classifies a character into HWP's 7 language font slots:
 * 0=한글, 1=영문, 2=한자, 3=일어, 4=외국어, 5=기호, 6=사용자.
 * Returns undefined for empty/whitespace characters so callers fall back to default slot 0.
 */
export function classifyHwpFontLang(ch: string | undefined): number | undefined {
  if (!ch) return undefined;
  const cp = ch.codePointAt(0);
  if (cp === undefined) return undefined;
  // Ignore ASCII/control whitespace so spaces don't flip font display away from surrounding text
  if (cp <= 0x0020 || cp === 0x007f || cp === 0x00a0 || cp === 0x3000) {
    return undefined;
  }
  // 0: Hangul (Syllables, Jamo, Compatibility Jamo, Extended Jamo, Halfwidth Hangul)
  if (
    (cp >= 0xac00 && cp <= 0xd7a3) ||
    (cp >= 0x1100 && cp <= 0x11ff) ||
    (cp >= 0x3130 && cp <= 0x318f) ||
    (cp >= 0xa960 && cp <= 0xa97f) ||
    (cp >= 0xd7b0 && cp <= 0xd7ff) ||
    (cp >= 0xffa0 && cp <= 0xffdc)
  ) {
    return 0;
  }
  // 1: English / Basic Latin & Fullwidth ASCII & Latin-1/Extended letters
  if (
    (cp >= 0x0021 && cp <= 0x007e) ||
    (cp >= 0xff01 && cp <= 0xff5e) ||
    (cp >= 0x0100 && cp <= 0x024f) ||
    (cp >= 0x00c0 && cp <= 0x00ff && cp !== 0x00d7 && cp !== 0x00f7)
  ) {
    return 1;
  }
  // 2: Hanja / CJK Ideographs
  if (
    (cp >= 0x4e00 && cp <= 0x9fff) ||
    (cp >= 0x3400 && cp <= 0x4dbf) ||
    (cp >= 0xf900 && cp <= 0xfaff) ||
    (cp >= 0x20000 && cp <= 0x2ceaf)
  ) {
    return 2;
  }
  // 3: Japanese (Hiragana, Katakana, Halfwidth Katakana)
  if (
    (cp >= 0x3040 && cp <= 0x309f) ||
    (cp >= 0x30a0 && cp <= 0x30ff) ||
    (cp >= 0x31f0 && cp <= 0x31ff) ||
    (cp >= 0xff65 && cp <= 0xff9f)
  ) {
    return 3;
  }
  // 6: User / Private Use Area
  if ((cp >= 0xe000 && cp <= 0xf8ff) || cp >= 0xf0000) {
    return 6;
  }
  // 4: Other foreign alphabets/letters (Greek, Cyrillic, Arabic, Thai, etc.)
  const charStr = String.fromCodePoint(cp);
  if (/\p{L}/u.test(charStr)) {
    return 4;
  }
  // 5: Symbols & non-ASCII punctuation
  return 5;
}
