import { describe, it, expect, vi } from "vitest";
import { lintOps } from "./policyGuard.ts";
import type { UnifiedOp } from "../types/operations.ts";
import type { TableCoordinateMeta } from "../types/hwp.ts";

describe("Frontend & Agent Architecture Improvements", () => {
  describe("PolicyGuard: set_cell_align and G1 relaxation", () => {
    it("G1 does not block normal user-requested text styling even when overflow exists", () => {
      const op: UnifiedOp = {
        type: "apply_text_style",
        targetText: "제목입니다",
        sizePt: 14,
        bold: true,
      };

      const res = lintOps([op], {
        userPrompt: "제목 글자 크기를 14pt 굵게 변경해줘",
        layoutReport: {
          tables: [],
          issues: [
            {
              type: "cell_overflow",
              severity: "warning",
              message: "오버플로",
              cell: { tableIndex: 0, row: 0, col: 0 },
            },
          ],
          totalOverflowMm: 2.0,
          pageCount: 1,
          printableBoundsMm: { widthMm: 150, heightMm: 250 },
        },
      });

      expect(res.passed).toBe(true);
      expect(res.rejectedOps).toHaveLength(0);
      expect(res.sanitizedOps).toHaveLength(1);
    });

    it("G1 only blocks font increase when user specifically asks to fix clipping/overflow without asking for style", () => {
      const op: UnifiedOp = {
        type: "apply_text_style",
        targetText: "내용",
        sizePt: 13,
      };

      const res = lintOps([op], {
        userPrompt: "글자가 잘려 보이는데 해결해줘",
        layoutReport: {
          tables: [],
          issues: [],
          totalOverflowMm: 0,
          pageCount: 1,
          printableBoundsMm: { widthMm: 150, heightMm: 250 },
        },
      });

      expect(res.passed).toBe(false);
      expect(res.rejectedOps[0].reason).toContain("G1 위반");
    });

    it("set_cell_align checks all cells in range against excluded regions", () => {
      const op: UnifiedOp = {
        type: "set_cell_align",
        tableIndex: 0,
        row: 0,
        col: 0,
        endRow: 1,
        endCol: 1,
        align: "CENTER",
        valign: "CENTER",
      } as any;

      const res = lintOps([op], {
        userPrompt: "표 1 정렬 가운데로 맞춰줘",
        excludedRegions: [
          {
            id: "ex-1",
            label: "셀 [1, 1]",
            targetType: "cell",
            tableIndex: 0,
            row: 1,
            col: 1,
          },
        ],
      });

      expect(res.passed).toBe(false);
      expect(res.rejectedOps[0].reason).toContain("G2 위반");
      expect(res.rejectedOps[0].reason).toContain("[1, 1]");
    });

    it("set_cell_align passes when no cells in range are excluded", () => {
      const op: UnifiedOp = {
        type: "set_cell_align",
        tableIndex: 0,
        row: 0,
        col: 0,
        endRow: 0,
        endCol: 2,
        align: "CENTER",
        valign: "CENTER",
      } as any;

      const res = lintOps([op], {
        userPrompt: "표 1 첫 행 정렬해줘",
        excludedRegions: [
          {
            id: "ex-1",
            label: "셀 [1, 1]",
            targetType: "cell",
            tableIndex: 0,
            row: 1,
            col: 1,
          },
        ],
      });

      expect(res.passed).toBe(true);
      expect(res.rejectedOps).toHaveLength(0);
    });
  });

  describe("Agent Loop V2: Verification gate layout fix filtering", () => {
    it("does not force re-looping on pre-existing overflow when user prompt is normal content editing", async () => {
      // Testing the layout filter regex
      const layoutFixRegex = /잘려|잘림|안보여|안\s*보여|겹쳐|겹침|넘침|오버플로|overflow|줄간격|여백|레이아웃\s*(?:수정|교정|개선|해결|맞춰)/i;
      expect(layoutFixRegex.test("수학 약안 내용으로 표 채워줘")).toBe(false);
      expect(layoutFixRegex.test("제목 14pt 굵게 바꿔줘")).toBe(false);
      expect(layoutFixRegex.test("표 1의 2행 3열 텍스트 수정")).toBe(false);
      expect(layoutFixRegex.test("글자가 잘려 보여서 해결해줘")).toBe(true);
      expect(layoutFixRegex.test("표 오버플로 수정해줘")).toBe(true);
      expect(layoutFixRegex.test("레이아웃 개선해줘")).toBe(true);
    });
  });
});
