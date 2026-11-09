import { describe, it, expect } from "vitest";
import { OpTranslator } from "./opTranslator.ts";
import type { UnifiedOp } from "../types/operations.ts";
import type { TableCoordinateMeta } from "../types/hwp.ts";

// Regression tests for the raw-patch failure
// "cell (row=0, col=0) not found via rhwp at sec=0 para=0 ctrl=0".
// Root cause: resolveHwpTableCoords used to fabricate
// { section: 0, para: 0, control: 0 } when tablesMeta had no entry for
// the requested tableIndex. para 0 / ctrl 0 is usually not a table at
// all, so the downstream engine failed with a misleading cell error
// even though the target cell existed in a table at another paragraph.

const oneTableMeta: TableCoordinateMeta[] = [
  {
    tableIndex: 0,
    sec: 0,
    para: 7,
    ctrl: 0,
    rowCount: 4,
    colCount: 3,
    cells: [
      { idx: 0, row: 0, col: 0, rowSpan: 1, colSpan: 1, text: "과제구분" },
    ],
  },
];

function setCellOp(tableIndex: number): UnifiedOp {
  return { type: "set_cell_text", tableIndex, row: 0, col: 0, text: "X" };
}

function hwpOps(payload: { stdinJson: string }) {
  return JSON.parse(payload.stdinJson).operations;
}

describe("OpTranslator table coordinates", () => {
  it("uses tablesMeta coordinates when the table is known", () => {
    const payload = OpTranslator.toEnginePayload("doc.hwp", false, [setCellOp(0)], oneTableMeta, false);
    expect(hwpOps(payload)).toEqual([
      expect.objectContaining({ type: "set_cell_text", section: 0, para: 7, control: 0, row: 0, col: 0 }),
    ]);
  });

  it("never fabricates para/control 0,0,0 for an out-of-bounds tableIndex", () => {
    // Model asked for table 1, but the document only has table 0.
    const payload = OpTranslator.toEnginePayload("doc.hwp", false, [setCellOp(1)], oneTableMeta, false);
    const ops = hwpOps(payload);
    expect(ops).toHaveLength(1);
    expect(ops[0].type).toBe("set_cell_text");
    expect(ops[0].para).toBeUndefined();
    expect(ops[0].control).toBeUndefined();
    // The tableIndex is carried so the engine can resolve it against
    // the actual file at execution time (or throw a clear no-table error).
    expect(ops[0].tableIndex).toBe(1);
    expect(ops[0].section).toBe(0);
  });

  it("carries tableIndex for a newly appended table absent from tablesMeta", () => {
    const payload = OpTranslator.toEnginePayload("doc.hwp", false, [setCellOp(0)], [], false);
    const ops = hwpOps(payload);
    expect(ops[0].para).toBeUndefined();
    expect(ops[0].control).toBeUndefined();
    expect(ops[0].tableIndex).toBe(0);
  });

  it("preserves explicitly supplied sec/para/ctrl", () => {
    const op = { ...setCellOp(0), sec: 0, para: 3, ctrl: 2 } as UnifiedOp;
    const payload = OpTranslator.toEnginePayload("doc.hwp", false, [op], oneTableMeta, false);
    const ops = hwpOps(payload);
    expect(ops[0]).toEqual(
      expect.objectContaining({ section: 0, para: 3, control: 2 }),
    );
  });
});
