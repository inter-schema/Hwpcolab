import { describe, expect, it, vi } from 'vitest';
import {
  classifyHwpFontLang,
  lookupResolvedFontFamilies,
  resolvePerLangFontIds,
  resolveUniformFontIds,
} from '@/core/font-ids';

describe('font-ids (#8140)', () => {
  it('resolves uniform font IDs across all 7 HWP language tables independently', () => {
    const wasm = {
      findOrCreateFontId: vi.fn(() => 99),
      // Simulate divergent per-language FaceName table lengths
      findOrCreateFontIdForLang: vi.fn((lang: number, name: string) => lang * 10 + (name === '맑은 고딕' ? 3 : 1)),
    };

    const ids = resolveUniformFontIds(wasm, '맑은 고딕');
    expect(ids).toEqual([3, 13, 23, 33, 43, 53, 63]);
    expect(wasm.findOrCreateFontId).not.toHaveBeenCalled();
    expect(wasm.findOrCreateFontIdForLang).toHaveBeenCalledTimes(7);
    expect(lookupResolvedFontFamilies(wasm, ids!)).toEqual(Array(7).fill('맑은 고딕'));
  });

  it('resolves per-language font families across all 7 slots', () => {
    const wasm = {
      findOrCreateFontId: vi.fn(() => 0),
      findOrCreateFontIdForLang: vi.fn((lang: number, name: string) => lang * 100 + name.length),
    };
    const families = ['함초롬바탕', 'Arial', '바탕', 'MS Gothic', 'Noto Sans', 'Wingdings', '사용자글꼴'];
    const ids = resolvePerLangFontIds(wasm, families);

    expect(ids).toEqual([5, 105, 202, 309, 409, 509, 605]);
    expect(lookupResolvedFontFamilies(wasm, ids!)).toEqual(families);
  });

  it('classifies characters into HWP 7 language categories automatically', () => {
    expect(classifyHwpFontLang('한')).toBe(0);
    expect(classifyHwpFontLang('ㅎ')).toBe(0);
    expect(classifyHwpFontLang('A')).toBe(1);
    expect(classifyHwpFontLang('7')).toBe(1);
    expect(classifyHwpFontLang('漢')).toBe(2);
    expect(classifyHwpFontLang('あ')).toBe(3);
    expect(classifyHwpFontLang('ア')).toBe(3);
    expect(classifyHwpFontLang('Б')).toBe(4);
    expect(classifyHwpFontLang('★')).toBe(5);
    expect(classifyHwpFontLang('\uE001')).toBe(6);
    expect(classifyHwpFontLang(' ')).toBeUndefined();
    expect(classifyHwpFontLang('')).toBeUndefined();
  });
});
