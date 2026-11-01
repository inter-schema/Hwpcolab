import type { UnifiedOp } from "../types/operations.ts";

export interface OfficialOutlineLevel {
  level: 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8;
  pattern: RegExp;
  example: string;
  indentPt: number;
  indentHu: number; // 1pt = 100 HWPUNIT
}

export const KOREAN_OFFICIAL_OUTLINE_LEVELS: OfficialOutlineLevel[] = [
  { level: 1, pattern: /^\s*\d+\.\s+/,             example: "1. ",   indentPt: 0,  indentHu: 0 },
  { level: 2, pattern: /^\s*[가-하]\.\s+/,         example: "가. ",  indentPt: 10, indentHu: 1000 },
  { level: 3, pattern: /^\s*\d+\)\s+/,             example: "1) ",   indentPt: 20, indentHu: 2000 },
  { level: 4, pattern: /^\s*[가-하]\)\s+/,         example: "가) ",  indentPt: 30, indentHu: 3000 },
  { level: 5, pattern: /^\s*\(\d+\)\s+/,           example: "(1) ",  indentPt: 40, indentHu: 4000 },
  { level: 6, pattern: /^\s*\([가-하]\)\s+/,       example: "(가) ", indentPt: 50, indentHu: 5000 },
  { level: 7, pattern: /^\s*[①-⑮]\s+/,             example: "① ",    indentPt: 60, indentHu: 6000 },
  { level: 8, pattern: /^\s*[㉮-㉻]\s+/,           example: "㉮ ",   indentPt: 70, indentHu: 7000 },
];

export const OFFICIAL_DOC_TYPOGRAPHY = {
  bodyFont: "함초롬바탕",
  headingFont: "함초롬바탕",
  bodySizePt: 11,
  lineSpacingPercent: 160,
  charRatio: 100,
  letterSpacing: 0,
  align: "JUSTIFY" as const,
};

/**
 * 일반 문단 목록을 공문서 규격(함초롬바탕, 줄간격 160%, 양쪽정렬, 개조식 단계별 들여쓰기)으로 교정하는 UnifiedOp 목록 생성
 */
export function buildOfficialDocStandardOps(paragraphs: string[]): UnifiedOp[] {
  const ops: UnifiedOp[] = [];

  paragraphs.forEach((text, idx) => {
    const trimmed = text.trim();
    if (!trimmed) return;

    // Detect outline level
    let matchedLevel: OfficialOutlineLevel | null = null;
    for (const lvl of KOREAN_OFFICIAL_OUTLINE_LEVELS) {
      if (lvl.pattern.test(trimmed)) {
        matchedLevel = lvl;
        break;
      }
    }

    const indentHu = matchedLevel ? matchedLevel.indentHu : 0;
    const targetSnippet = trimmed.slice(0, Math.min(20, trimmed.length));

    ops.push({
      type: "apply_paragraph_style",
      paraIndex: idx,
      targetText: targetSnippet,
      align: OFFICIAL_DOC_TYPOGRAPHY.align,
      lineSpacingPercent: OFFICIAL_DOC_TYPOGRAPHY.lineSpacingPercent,
      indentHu,
    });

    ops.push({
      type: "apply_text_style",
      targetText: targetSnippet,
      fontFamily: OFFICIAL_DOC_TYPOGRAPHY.bodyFont,
      sizePt: OFFICIAL_DOC_TYPOGRAPHY.bodySizePt,
    });
  });

  return ops;
}
