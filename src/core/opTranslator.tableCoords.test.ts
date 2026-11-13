import { describe, it, expect } from "vitest";
import { OpTranslator } from "./opTranslator.ts";
import type { UnifiedOp } from "../types/operations.ts";
import type { TableCoordinateMeta } from "../types/hwp.ts";

// Regression tests for table coordinate resolution across all table operations.
// Verifies that OpTranslator never fabricates para: 0, control: 0 when tablesMeta
// is missing or out-of-bounds, correctly carries tableIndex, and preserves explicit
// or meta coordinates across all 18 table operations.

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
      expect.objectContaining({ type: "set_cell_text", section: 0, para: 7, control: 0, tableIndex: 0, row: 0, col: 0 }),
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
      expect.objectContaining({ section: 0, para: 3, control: 2, tableIndex: 0 }),
    );
  });

  describe("layout operations coordinate resolution", () => {
    it("handles set_col_width when table is known vs out-of-bounds", () => {
      const known = OpTranslator.toEnginePayload(
        "doc.hwp", false,
        [{ type: "set_col_width", tableIndex: 0, col: 1, widthMm: 35 } as UnifiedOp],
        oneTableMeta, false
      );
      expect(hwpOps(known)[0]).toEqual(
        expect.objectContaining({ type: "set_col_width", section: 0, para: 7, control: 0, tableIndex: 0, col: 1, width_mm: 35 })
      );

      const unknown = OpTranslator.toEnginePayload(
        "doc.hwp", false,
        [{ type: "set_col_width", tableIndex: 1, col: 1, widthMm: 35 } as UnifiedOp],
        oneTableMeta, false
      );
      const op = hwpOps(unknown)[0];
      expect(op.type).toBe("set_col_width");
      expect(op.para).toBeUndefined();
      expect(op.control).toBeUndefined();
      expect(op.tableIndex).toBe(1);
      expect(op.section).toBe(0);
    });

    it("handles set_row_height when tablesMeta is empty", () => {
      const payload = OpTranslator.toEnginePayload(
        "doc.hwp", false,
        [{ type: "set_row_height", tableIndex: 0, row: 2, heightMm: 12 } as UnifiedOp],
        [], false
      );
      const op = hwpOps(payload)[0];
      expect(op).toEqual(
        expect.objectContaining({ type: "set_row_height", section: 0, tableIndex: 0, row: 2, height_mm: 12 })
      );
      expect(op.para).toBeUndefined();
      expect(op.control).toBeUndefined();
    });

    it("handles set_cell_padding without fabricating para/control", () => {
      const payload = OpTranslator.toEnginePayload(
        "doc.hwp", false,
        [{ type: "set_cell_padding", tableIndex: 3, row: 0, col: 0, leftMm: 3 } as UnifiedOp],
        oneTableMeta, false
      );
      const op = hwpOps(payload)[0];
      expect(op.type).toBe("set_cell_padding");
      expect(op.tableIndex).toBe(3);
      expect(op.para).toBeUndefined();
      expect(op.control).toBeUndefined();
    });

    it("handles fit_table_to_page without fabricating para/control", () => {
      const payload = OpTranslator.toEnginePayload(
        "doc.hwp", false,
        [{ type: "fit_table_to_page", tableIndex: 2, targetWidthMm: 160 } as UnifiedOp],
        oneTableMeta, false
      );
      const op = hwpOps(payload)[0];
      expect(op.type).toBe("fit_table_to_page");
      expect(op.tableIndex).toBe(2);
      expect(op.para).toBeUndefined();
      expect(op.control).toBeUndefined();
    });

    it("handles shrink_cell_font without fabricating para/control", () => {
      const payload = OpTranslator.toEnginePayload(
        "doc.hwp", false,
        [{ type: "shrink_cell_font", tableIndex: 1, row: 1, col: 1, sizePt: 8 } as UnifiedOp],
        oneTableMeta, false
      );
      const op = hwpOps(payload)[0];
      expect(op.type).toBe("shrink_cell_font");
      expect(op.tableIndex).toBe(1);
      expect(op.para).toBeUndefined();
      expect(op.control).toBeUndefined();
    });
  });

  describe("structural operations coordinate resolution", () => {
    it("handles merge_cells without fabricating para/control", () => {
      const payload = OpTranslator.toEnginePayload(
        "doc.hwp", false,
        [{ type: "merge_cells", tableIndex: 5, fromRow: 0, fromCol: 0, toRow: 0, toCol: 2 } as UnifiedOp],
        [], false
      );
      const op = hwpOps(payload)[0];
      expect(op.type).toBe("merge_cells");
      expect(op.tableIndex).toBe(5);
      expect(op.para).toBeUndefined();
      expect(op.control).toBeUndefined();
    });

    it("handles insert_table_row without fabricating para/control", () => {
      const payload = OpTranslator.toEnginePayload(
        "doc.hwp", false,
        [{ type: "insert_table_row", tableIndex: 2, row: 1, position: "below" } as UnifiedOp],
        oneTableMeta, false
      );
      const op = hwpOps(payload)[0];
      expect(op.type).toBe("insert_table_row");
      expect(op.tableIndex).toBe(2);
      expect(op.para).toBeUndefined();
      expect(op.control).toBeUndefined();
    });
  });

  describe("insert_image (cell case) coordinate resolution", () => {
    it("spreads resolved coords into cell object without fabricating para/control", () => {
      const unknown = OpTranslator.toEnginePayload(
        "doc.hwp", false,
        [{ type: "insert_image", path: "img.png", cell: { tableIndex: 4, row: 1, col: 2 } } as UnifiedOp],
        oneTableMeta, false
      );
      const op = hwpOps(unknown)[0];
      expect(op.type).toBe("insert_image");
      expect(op.cell).toEqual({
        row: 1,
        col: 2,
        section: 0,
        tableIndex: 4,
      });
      expect(op.cell.para).toBeUndefined();
      expect(op.cell.control).toBeUndefined();

      const known = OpTranslator.toEnginePayload(
        "doc.hwp", false,
        [{ type: "insert_image", path: "img.png", cell: { tableIndex: 0, row: 1, col: 2 } } as UnifiedOp],
        oneTableMeta, false
      );
      const knownOp = hwpOps(known)[0];
      expect(knownOp.cell).toEqual({
        row: 1,
        col: 2,
        section: 0,
        para: 7,
        control: 0,
        tableIndex: 0,
      });
    });
  });
});
