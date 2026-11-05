import { describe, it, expect } from "vitest";
import { OpTranslator } from "./opTranslator.ts";
import { PrivacySessionMasker } from "./privacyMasker.ts";
import type { UnifiedOp } from "../types/operations.ts";
import type { TableCoordinateMeta } from "../types/hwp.ts";

describe("Stream A: OpTranslator - insert_nested_table", () => {
  const dummyTableMeta: TableCoordinateMeta[] = [
    {
      tableIndex: 0,
      sec: 0,
      para: 0,
      ctrl: 0,
      rowCount: 2,
      colCount: 2,
      cells: [
        { idx: 0, row: 0, col: 0, rowSpan: 1, colSpan: 1, text: "A" },
        { idx: 1, row: 0, col: 1, rowSpan: 1, colSpan: 1, text: "B" },
        { idx: 2, row: 1, col: 0, rowSpan: 1, colSpan: 1, text: "C" },
        { idx: 3, row: 1, col: 1, rowSpan: 1, colSpan: 1, text: "D" },
      ],
    },
  ];

  it("throws clear error for .hwp binary files", () => {
    const op: UnifiedOp = {
      type: "insert_nested_table",
      tableIndex: 0,
      row: 1,
      col: 0,
      rows: 2,
      cols: 2,
      data: [["1", "2"], ["3", "4"]],
      headerRow: false,
      widthMode: "fit_cell",
    };

    expect(() => {
      OpTranslator.toEnginePayload("doc.hwp", false, [op], dummyTableMeta, false);
    }).toThrow("HWP 바이너리는 중첩 표 미지원 — HWPX로 저장 후 시도");
  });

  it("translates insert_nested_table for .hwpx files and sanitizes markdown", () => {
    const op: UnifiedOp = {
      type: "insert_nested_table",
      tableIndex: 0,
      row: 1,
      col: 0,
      rows: 2,
      cols: 2,
      data: [
        ["**머리글1**", "*머리글2*"],
        ["데이터1", "**데이터2**"],
      ],
      headerRow: true,
      widthMode: "fit_cell",
    };

    const payload = OpTranslator.toEnginePayload("doc.hwpx", true, [op], dummyTableMeta, false);
    expect(payload.isHwpx).toBe(true);

    const parsed = JSON.parse(payload.stdinJson);
    expect(parsed.operations).toHaveLength(1);

    const hwpxOp = parsed.operations[0];
    expect(hwpxOp.type).toBe("insert_nested_table");
    expect(hwpxOp.table).toBe(0);
    expect(hwpxOp.row).toBe(1);
    expect(hwpxOp.col).toBe(0);
    expect(hwpxOp.rows).toBe(2);
    expect(hwpxOp.cols).toBe(2);
    expect(hwpxOp.headerRow).toBe(true);
    expect(hwpxOp.widthMode).toBe("fit_cell");

    // Markdown asterisks should be sanitized
    expect(hwpxOp.data).toEqual([
      ["머리글1", "머리글2"],
      ["데이터1", "데이터2"],
    ]);
    expect(hwpxOp.cells).toEqual(hwpxOp.data);
  });

  it("unmasks data strings and respects excluded regions in privacyMasker", () => {
    const masker = new PrivacySessionMasker();
    // Register PII token via maskTextForPrompt
    const rawPhone = "010-1234-5678";
    const maskedText = masker.maskTextForPrompt(`연락처: ${rawPhone}`, [], true);
    const phoneTokenMatch = maskedText.match(/\[\[PII_PHONE_\d+\]\]/);
    expect(phoneTokenMatch).not.toBeNull();
    const token = phoneTokenMatch![0];

    const allowedOp: UnifiedOp = {
      type: "insert_nested_table",
      tableIndex: 0,
      row: 0,
      col: 0,
      rows: 1,
      cols: 1,
      data: [[token]],
    };

    const blockedOp: UnifiedOp = {
      type: "insert_nested_table",
      tableIndex: 0,
      row: 1,
      col: 1,
      rows: 1,
      cols: 1,
      data: [["test"]],
    };

    const excludedRegions = [
      {
        id: "ex1",
        targetType: "cell" as const,
        tableIndex: 0,
        row: 1,
        col: 1,
        rawText: "D",
      },
    ];

    const result = masker.unmaskAndFilterOps([allowedOp, blockedOp], excludedRegions);

    // blockedOp targeting excluded cell (0, 1, 1) should be filtered out
    expect(result).toHaveLength(1);
    const unmasked = result[0];
    expect(unmasked.type).toBe("insert_nested_table");
    if (unmasked.type === "insert_nested_table") {
      expect(unmasked.data).toEqual([[rawPhone]]);
    }
  });
});
