import { describe, it, expect } from "vitest";
import { lintOps } from "./policyGuard.ts";
import type { UnifiedOp } from "../types/operations.ts";
import type { TableCoordinateMeta } from "../types/hwp.ts";

describe("Stream A: PolicyGuard - insert_nested_table", () => {
  const dummyTablesMeta: TableCoordinateMeta[] = [
    {
      tableIndex: 0,
      sec: 0,
      para: 0,
      ctrl: 0,
      rowCount: 3,
      colCount: 3,
      depth: 0,
      cells: [
        { idx: 0, row: 0, col: 0, rowSpan: 1, colSpan: 1, text: "A" },
        { idx: 1, row: 0, col: 1, rowSpan: 1, colSpan: 1, text: "B" },
        { idx: 2, row: 0, col: 2, rowSpan: 1, colSpan: 1, text: "C" },
        { idx: 3, row: 1, col: 0, rowSpan: 1, colSpan: 1, text: "D" },
        { idx: 4, row: 1, col: 1, rowSpan: 1, colSpan: 1, text: "E" },
        { idx: 5, row: 1, col: 2, rowSpan: 1, colSpan: 1, text: "F" },
        { idx: 6, row: 2, col: 0, rowSpan: 1, colSpan: 1, text: "G" },
        { idx: 7, row: 2, col: 1, rowSpan: 1, colSpan: 1, text: "H" },
        { idx: 8, row: 2, col: 2, rowSpan: 1, colSpan: 1, text: "I" },
      ],
    },
    {
      tableIndex: 1,
      sec: 0,
      para: 0,
      ctrl: 0,
      rowCount: 2,
      colCount: 2,
      depth: 1,
      parentTableIndex: 0,
      parentCell: { row: 1, col: 1 },
      cells: [
        { idx: 0, row: 0, col: 0, rowSpan: 1, colSpan: 1, text: "N1" },
        { idx: 1, row: 0, col: 1, rowSpan: 1, colSpan: 1, text: "N2" },
        { idx: 2, row: 1, col: 0, rowSpan: 1, colSpan: 1, text: "N3" },
        { idx: 3, row: 1, col: 1, rowSpan: 1, colSpan: 1, text: "N4" },
      ],
    },
    {
      tableIndex: 2,
      sec: 0,
      para: 0,
      ctrl: 0,
      rowCount: 1,
      colCount: 1,
      depth: 2,
      parentTableIndex: 1,
      parentCell: { row: 0, col: 0 },
      cells: [{ idx: 0, row: 0, col: 0, rowSpan: 1, colSpan: 1, text: "Deep" }],
    },
  ];

  it("rejects for .hwp documents with clear message", () => {
    const op: UnifiedOp = {
      type: "insert_nested_table",
      tableIndex: 0,
      row: 0,
      col: 0,
      rows: 2,
      cols: 2,
    };

    const res = lintOps([op], {
      userPrompt: "표 안에 표 넣어줘",
      isHwpx: false,
      tablesMeta: dummyTablesMeta,
    });

    expect(res.passed).toBe(false);
    expect(res.rejectedOps).toHaveLength(1);
    expect(res.rejectedOps[0].reason).toContain("HWP 바이너리는 중첩 표 미지원");
  });

  it("rejects when size exceeds 8x8 limit", () => {
    const op: UnifiedOp = {
      type: "insert_nested_table",
      tableIndex: 0,
      row: 0,
      col: 0,
      rows: 9,
      cols: 2,
    };

    const res = lintOps([op], {
      userPrompt: "중첩 표 삽입",
      isHwpx: true,
      tablesMeta: dummyTablesMeta,
    });

    expect(res.passed).toBe(false);
    expect(res.rejectedOps[0].reason).toContain("크기 제한 위반");
  });

  it("rejects when widthMode is not fit_cell", () => {
    const op: UnifiedOp = {
      type: "insert_nested_table",
      tableIndex: 0,
      row: 0,
      col: 0,
      rows: 2,
      cols: 2,
      widthMode: "stretch_page" as any,
    };

    const res = lintOps([op], {
      userPrompt: "중첩 표 삽입",
      isHwpx: true,
      tablesMeta: dummyTablesMeta,
    });

    expect(res.passed).toBe(false);
    expect(res.rejectedOps[0].reason).toContain("지원되지 않는 widthMode");
  });

  it("rejects when target table depth >= 2 (max depth <= 2)", () => {
    // Target table 2 has depth 2. Inserting inside it would make depth 3.
    const op: UnifiedOp = {
      type: "insert_nested_table",
      tableIndex: 2,
      row: 0,
      col: 0,
      rows: 1,
      cols: 1,
    };

    const res = lintOps([op], {
      userPrompt: "깊은 중첩 표 삽입",
      isHwpx: true,
      tablesMeta: dummyTablesMeta,
    });

    expect(res.passed).toBe(false);
    expect(res.rejectedOps[0].reason).toContain("중첩 깊이 초과");
  });

  it("rejects when target cell is out of range", () => {
    const op: UnifiedOp = {
      type: "insert_nested_table",
      tableIndex: 0,
      row: 5,
      col: 0,
      rows: 2,
      cols: 2,
    };

    const res = lintOps([op], {
      userPrompt: "중첩 표 삽입",
      isHwpx: true,
      tablesMeta: dummyTablesMeta,
    });

    expect(res.passed).toBe(false);
    expect(res.rejectedOps[0].reason).toContain("대상 셀이 표 범위를 벗어납니다");
  });

  it("rejects under G2 when target cell is in excludedRegions", () => {
    const op: UnifiedOp = {
      type: "insert_nested_table",
      tableIndex: 0,
      row: 1,
      col: 1,
      rows: 2,
      cols: 2,
    };

    const res = lintOps([op], {
      userPrompt: "중첩 표 삽입",
      isHwpx: true,
      tablesMeta: dummyTablesMeta,
      excludedRegions: [
        {
          id: "ex1",
          targetType: "cell",
          tableIndex: 0,
          row: 1,
          col: 1,
          rawText: "E",
        },
      ],
    });

    expect(res.passed).toBe(false);
    expect(res.rejectedOps[0].reason).toContain("G2 위반");
  });

  it("passes when valid and sanitizes markdown in data", () => {
    const op: UnifiedOp = {
      type: "insert_nested_table",
      tableIndex: 0,
      row: 0,
      col: 0,
      rows: 2,
      cols: 2,
      data: [
        ["**헤더1**", "*헤더2*"],
        ["값1", "**값2**"],
      ],
      headerRow: true,
      widthMode: "fit_cell",
    };

    const res = lintOps([op], {
      userPrompt: "표 안에 하위 표 만들어줘",
      isHwpx: true,
      tablesMeta: dummyTablesMeta,
    });

    expect(res.passed).toBe(true);
    expect(res.sanitizedOps).toHaveLength(1);
    const sanitized = res.sanitizedOps[0];
    if (sanitized.type === "insert_nested_table") {
      expect(sanitized.data).toEqual([
        ["헤더1", "헤더2"],
        ["값1", "값2"],
      ]);
    }
  });
});
