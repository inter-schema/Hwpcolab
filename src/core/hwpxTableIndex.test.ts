import { describe, it, expect, vi } from "vitest";
import * as fs from "node:fs";
import * as path from "node:path";
import * as os from "node:os";
import { spawn } from "node:child_process";
import {
  scanTopLevel,
  parseHwpxSectionTables,
  mergeHwpxTablesWithRhwp,
  buildTablesMetaForHwpx,
  mapInspectedTables,
  syncHwpxTableIndex,
} from "./hwpxTableIndex.ts";
import type { TableCoordinateMeta } from "../types/hwp.ts";

describe("Stream A: hwpxTableIndex", () => {
  it("scanTopLevel skips nested same-name tags correctly", () => {
    const xml = `<hp:tbl id="outer"><hp:tr><hp:tc><hp:subList><hp:p><hp:run><hp:tbl id="inner"><hp:tr><hp:tc></hp:tc></hp:tr></hp:tbl></hp:run></hp:p></hp:subList></hp:tc></hp:tr></hp:tbl>`;
    const topTbls = scanTopLevel(xml, "hp:tbl");
    expect(topTbls).toHaveLength(1);
    expect(topTbls[0].attrs).toContain('id="outer"');

    // Inside topTbls[0].inner, we find inner tbl
    const innerTbls = scanTopLevel(topTbls[0].inner, "hp:tbl");
    expect(innerTbls).toHaveLength(1);
    expect(innerTbls[0].attrs).toContain('id="inner"');
  });

  it("parses single top-level table from synthetic section XML", () => {
    const xml = `
      <hs:sec>
        <hp:tbl id="100">
          <hp:tr>
            <hp:tc><hp:cellAddr colAddr="0" rowAddr="0"/><hp:cellSpan colSpan="1" rowSpan="1"/><hp:subList><hp:p><hp:run><hp:t>A1</hp:t></hp:run></hp:p></hp:subList></hp:tc>
            <hp:tc><hp:cellAddr colAddr="1" rowAddr="0"/><hp:cellSpan colSpan="1" rowSpan="1"/><hp:subList><hp:p><hp:run><hp:t>B1</hp:t></hp:run></hp:p></hp:subList></hp:tc>
          </hp:tr>
          <hp:tr>
            <hp:tc><hp:cellAddr colAddr="0" rowAddr="1"/><hp:cellSpan colSpan="1" rowSpan="1"/><hp:subList><hp:p><hp:run><hp:t>A2</hp:t></hp:run></hp:p></hp:subList></hp:tc>
            <hp:tc><hp:cellAddr colAddr="1" rowAddr="1"/><hp:cellSpan colSpan="1" rowSpan="1"/><hp:subList><hp:p><hp:run><hp:t>B2</hp:t></hp:run></hp:p></hp:subList></hp:tc>
          </hp:tr>
        </hp:tbl>
      </hs:sec>
    `;

    const tables = parseHwpxSectionTables(xml, 0);
    expect(tables).toHaveLength(1);
    expect(tables[0].depth).toBe(0);
    expect(tables[0].parentTableIndex).toBeUndefined();
    expect(tables[0].rowCount).toBe(2);
    expect(tables[0].colCount).toBe(2);
    expect(tables[0].cells).toHaveLength(4);
    expect(tables[0].cells[0].text).toBe("A1");
    expect(tables[0].cells[3].text).toBe("B2");
  });

  it("parses nested tables in pre-order with parent coordinates", () => {
    // Table 0 (Outer 2x2) contains Table 1 (Inner 2x2) in cell (row 1, col 0)
    const xml = `
      <hs:sec>
        <hp:tbl id="outer">
          <hp:tr>
            <hp:tc><hp:cellAddr colAddr="0" rowAddr="0"/><hp:subList><hp:p><hp:run><hp:t>Outer (0,0)</hp:t></hp:run></hp:p></hp:subList></hp:tc>
            <hp:tc><hp:cellAddr colAddr="1" rowAddr="0"/><hp:subList><hp:p><hp:run><hp:t>Outer (0,1)</hp:t></hp:run></hp:p></hp:subList></hp:tc>
          </hp:tr>
          <hp:tr>
            <hp:tc>
              <hp:cellAddr colAddr="0" rowAddr="1"/>
              <hp:subList>
                <hp:p><hp:run><hp:t>Parent Cell Text</hp:t></hp:run></hp:p>
                <hp:p><hp:run>
                  <hp:tbl id="nested">
                    <hp:tr>
                      <hp:tc><hp:cellAddr colAddr="0" rowAddr="0"/><hp:subList><hp:p><hp:run><hp:t>Sub A</hp:t></hp:run></hp:p></hp:subList></hp:tc>
                      <hp:tc><hp:cellAddr colAddr="1" rowAddr="0"/><hp:subList><hp:p><hp:run><hp:t>Sub B</hp:t></hp:run></hp:p></hp:subList></hp:tc>
                    </hp:tr>
                  </hp:tbl>
                </hp:run></hp:p>
              </hp:subList>
            </hp:tc>
            <hp:tc><hp:cellAddr colAddr="1" rowAddr="1"/><hp:subList><hp:p><hp:run><hp:t>Outer (1,1)</hp:t></hp:run></hp:p></hp:subList></hp:tc>
          </hp:tr>
        </hp:tbl>
      </hs:sec>
    `;

    const tables = parseHwpxSectionTables(xml, 0);
    expect(tables).toHaveLength(2);

    // Table 0: Outer
    expect(tables[0].tableIndex).toBe(0);
    expect(tables[0].depth).toBe(0);
    expect(tables[0].parentTableIndex).toBeUndefined();
    expect(tables[0].rowCount).toBe(2);
    expect(tables[0].colCount).toBe(2);

    // Table 1: Nested
    expect(tables[1].tableIndex).toBe(1);
    expect(tables[1].depth).toBe(1);
    expect(tables[1].parentTableIndex).toBe(0);
    expect(tables[1].parentCell).toEqual({ row: 1, col: 0 });
    expect(tables[1].rowCount).toBe(1);
    expect(tables[1].colCount).toBe(2);
    expect(tables[1].cells[0].text).toBe("Sub A");
    expect(tables[1].cells[1].text).toBe("Sub B");
  });

  it("resolves table index shift when merging pre-order HWPX with rhwpEngine top-level tables", () => {
    // Scenario: Document has:
    // - Table 0: Top-level table A (2x2) containing nested Table 1 (N, depth 1)
    // - Table 1: Nested table N (1x2)
    // - Table 2: Top-level table B (1x1)
    // rhwpEngine only indexes top-level tables, so it only produced 2 tables:
    // [0]: Table A, [1]: Table B.
    // If not merged, any op on table 1 in hwpx-edit would hit Table N, but in rhwp it would hit Table B!

    const rhwpTopTables: TableCoordinateMeta[] = [
      {
        tableIndex: 0,
        sec: 0,
        para: 0,
        ctrl: 0,
        rowCount: 2,
        colCount: 2,
        cells: [
          { idx: 0, row: 0, col: 0, rowSpan: 1, colSpan: 1, text: "A", bbox: { cellIdx: 0, row: 0, col: 0, rowSpan: 1, colSpan: 1, pageIndex: 0, x: 10, y: 10, w: 100, h: 20 } },
          { idx: 1, row: 0, col: 1, rowSpan: 1, colSpan: 1, text: "B", bbox: { cellIdx: 1, row: 0, col: 1, rowSpan: 1, colSpan: 1, pageIndex: 0, x: 110, y: 10, w: 100, h: 20 } },
          { idx: 2, row: 1, col: 0, rowSpan: 1, colSpan: 1, text: "C", bbox: { cellIdx: 2, row: 1, col: 0, rowSpan: 1, colSpan: 1, pageIndex: 0, x: 10, y: 30, w: 100, h: 20 } },
          { idx: 3, row: 1, col: 1, rowSpan: 1, colSpan: 1, text: "D", bbox: { cellIdx: 3, row: 1, col: 1, rowSpan: 1, colSpan: 1, pageIndex: 0, x: 110, y: 30, w: 100, h: 20 } },
        ],
      },
      {
        tableIndex: 1,
        sec: 0,
        para: 2,
        ctrl: 0,
        rowCount: 1,
        colCount: 1,
        cells: [
          { idx: 0, row: 0, col: 0, rowSpan: 1, colSpan: 1, text: "Table B Cell", bbox: { cellIdx: 0, row: 0, col: 0, rowSpan: 1, colSpan: 1, pageIndex: 0, x: 10, y: 100, w: 200, h: 30 } },
        ],
      },
    ];

    const hwpxParsedTables = [
      {
        sectionIndex: 0,
        tableIndex: 0,
        depth: 0,
        rowCount: 2,
        colCount: 2,
        cells: [
          { idx: 0, row: 0, col: 0, rowSpan: 1, colSpan: 1, text: "A", innerStart: 0, innerEnd: 50 },
          { idx: 1, row: 0, col: 1, rowSpan: 1, colSpan: 1, text: "B", innerStart: 51, innerEnd: 100 },
          { idx: 2, row: 1, col: 0, rowSpan: 1, colSpan: 1, text: "C", innerStart: 101, innerEnd: 200 },
          { idx: 3, row: 1, col: 1, rowSpan: 1, colSpan: 1, text: "D", innerStart: 201, innerEnd: 250 },
        ],
      },
      {
        sectionIndex: 0,
        tableIndex: 1,
        depth: 1,
        parentTableIndex: 0,
        parentCell: { row: 1, col: 0 },
        rowCount: 1,
        colCount: 2,
        cells: [
          { idx: 0, row: 0, col: 0, rowSpan: 1, colSpan: 1, text: "N1", innerStart: 0, innerEnd: 50 },
          { idx: 1, row: 0, col: 1, rowSpan: 1, colSpan: 1, text: "N2", innerStart: 51, innerEnd: 100 },
        ],
      },
      {
        sectionIndex: 0,
        tableIndex: 2,
        depth: 0,
        rowCount: 1,
        colCount: 1,
        cells: [
          { idx: 0, row: 0, col: 0, rowSpan: 1, colSpan: 1, text: "Table B Cell", innerStart: 0, innerEnd: 50 },
        ],
      },
    ];

    const merged = mergeHwpxTablesWithRhwp(hwpxParsedTables, rhwpTopTables);

    expect(merged).toHaveLength(3);

    // Table 0: Table A (top-level)
    expect(merged[0].tableIndex).toBe(0);
    expect(merged[0].depth).toBe(0);
    expect(merged[0].cells[0].bbox?.w).toBe(100);

    // Table 1: Nested Table N (depth 1)
    expect(merged[1].tableIndex).toBe(1);
    expect(merged[1].depth).toBe(1);
    expect(merged[1].parentTableIndex).toBe(0);
    expect(merged[1].parentCell).toEqual({ row: 1, col: 0 });
    expect(merged[1].cells[0].bbox).toBeUndefined();

    // Table 2: Table B (top-level, correctly shifted to index 2!)
    expect(merged[2].tableIndex).toBe(2);
    expect(merged[2].depth).toBe(0);
    expect(merged[2].para).toBe(2);
    expect(merged[2].cells[0].bbox?.w).toBe(200);
  });

  it("safely falls back to rhwpEngine tables when table counts or dimensions mismatch", () => {
    const warnSpy = vi.spyOn(console, "warn").mockImplementation(() => {});

    const rhwpTopTables: TableCoordinateMeta[] = [
      {
        tableIndex: 0,
        sec: 0,
        para: 0,
        ctrl: 0,
        rowCount: 2,
        colCount: 2,
        cells: [],
      },
    ];

    // HWPX parsed tables has 2 top-level tables (mismatch with rhwp 1 table)
    const hwpxParsedTables = [
      {
        sectionIndex: 0,
        tableIndex: 0,
        depth: 0,
        rowCount: 2,
        colCount: 2,
        cells: [],
      },
      {
        sectionIndex: 0,
        tableIndex: 1,
        depth: 0,
        rowCount: 3,
        colCount: 3,
        cells: [],
      },
    ];

    const merged = mergeHwpxTablesWithRhwp(hwpxParsedTables, rhwpTopTables);
    expect(warnSpy).toHaveBeenCalledWith(expect.stringContaining("mismatch"));
    expect(merged).toHaveLength(1);
    expect(merged[0].tableIndex).toBe(0);
    expect(merged[0].depth).toBe(0);

    warnSpy.mockRestore();
  });

  describe("DEFECT-03: buildTablesMetaForHwpx & mapInspectedTables", () => {
    const SAMPLE_HWPX = path.resolve("samples/(날짜)(교시)(과목명)교수.학습과정안(이름).hwpx");

    function runHwpxEdit(payload: any): Promise<{ code: number; stdout: any; stderr: string }> {
      const scriptPath = path.resolve("resources/claw-hwp/scripts/hwpx-edit.js");
      return new Promise((resolvePromise, rejectPromise) => {
        const child = spawn("node", [scriptPath], { stdio: ["pipe", "pipe", "pipe"] });
        const stdoutChunks: Buffer[] = [];
        const stderrChunks: Buffer[] = [];

        child.stdout.on("data", (d) => stdoutChunks.push(Buffer.from(d)));
        child.stderr.on("data", (d) => stderrChunks.push(Buffer.from(d)));
        child.on("error", (err) => rejectPromise(err));
        child.on("close", (code) => {
          const stdoutRaw = Buffer.concat(stdoutChunks).toString("utf8").trim();
          const stderrRaw = Buffer.concat(stderrChunks).toString("utf8");
          let parsed: any = null;
          try {
            parsed = JSON.parse(stdoutRaw);
          } catch {
            parsed = { raw: stdoutRaw };
          }
          resolvePromise({ code: code ?? 0, stdout: parsed, stderr: stderrRaw });
        });

        child.stdin.write(JSON.stringify(payload));
        child.stdin.end();
      });
    }

    it("(b) parses top-level tables only from real samples/ HWPX", () => {
      const bytes = fs.readFileSync(SAMPLE_HWPX);
      const meta = buildTablesMetaForHwpx(bytes);

      expect(meta.length).toBeGreaterThanOrEqual(1);
      // All tables in sample are top-level
      for (const t of meta) {
        expect(t.depth).toBe(0);
        expect(t.parentTableIndex).toBeUndefined();
        expect(t.parentCell).toBeUndefined();
        expect(t.cells.length).toBeGreaterThan(0);
      }
    });

    it("(b) preserves existing bboxes for top-level tables when rhwpTables are provided", () => {
      const bytes = fs.readFileSync(SAMPLE_HWPX);
      const rawMeta = buildTablesMetaForHwpx(bytes);
      expect(rawMeta.length).toBeGreaterThanOrEqual(1);

      // Simulate rhwp/existing tables that have bboxes
      const mockRhwpTables: TableCoordinateMeta[] = rawMeta.map((t) => ({
        ...t,
        cells: t.cells.map((c) => ({
          ...c,
          bbox: {
            cellIdx: c.idx,
            row: c.row,
            col: c.col,
            rowSpan: c.rowSpan,
            colSpan: c.colSpan,
            pageIndex: 0,
            x: 10 + c.col * 50,
            y: 20 + c.row * 20,
            w: 50,
            h: 20,
          },
        })),
      }));

      const mergedMeta = buildTablesMetaForHwpx(bytes, mockRhwpTables);
      expect(mergedMeta.length).toBe(rawMeta.length);
      for (let i = 0; i < mergedMeta.length; i++) {
        expect(mergedMeta[i].depth).toBe(0);
        expect(mergedMeta[i].cells[0].bbox?.x).toBe(10);
      }
    });

    it("(a) parses nested tables in pre-order with depth and parentCell on real HWPX edited with insert_nested_table", async () => {
      const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), "defect03-hwpx-"));
      try {
        const testFile = path.join(tempDir, "nested.hwpx");
        fs.copyFileSync(SAMPLE_HWPX, testFile);

        // Insert nested table into Table 0, Row 8, Col 1
        const editRes = await runHwpxEdit({
          path: testFile,
          output: testFile,
          operations: [
            {
              type: "insert_nested_table",
              table: 0,
              row: 8,
              col: 1,
              rows: 2,
              cols: 2,
              data: [
                ["소제목1", "소제목2"],
                ["내용1", "내용2"],
              ],
              headerRow: true,
              widthMode: "fit_cell",
            },
          ],
        });
        expect(editRes.code).toBe(0);
        expect(editRes.stdout.ok).toBe(true);

        const editedBytes = fs.readFileSync(testFile);
        const meta = buildTablesMetaForHwpx(editedBytes);

        // Must have Table 0 (depth 0) and Table 1 (depth 1, nested in Table 0 at row 8, col 1)
        expect(meta.length).toBe(2);

        // Table 0: Top-level parent
        expect(meta[0].tableIndex).toBe(0);
        expect(meta[0].depth).toBe(0);
        expect(meta[0].parentTableIndex).toBeUndefined();

        // Table 1: Nested child
        expect(meta[1].tableIndex).toBe(1);
        expect(meta[1].depth).toBe(1);
        expect(meta[1].parentTableIndex).toBe(0);
        expect(meta[1].parentCell).toEqual({ row: 8, col: 1 });
        expect(meta[1].rowCount).toBe(2);
        expect(meta[1].colCount).toBe(2);
        expect(meta[1].cells.some((c) => c.text === "소제목1")).toBe(true);
        expect(meta[1].cells.some((c) => c.text === "내용2")).toBe(true);
      } finally {
        fs.rmSync(tempDir, { recursive: true, force: true });
      }
    });

    it("(c) .hwp invariant: mapInspectedTables keeps depth=0 and preserves structure for .hwp tables", () => {
      const hwpInspectedTables = [
        {
          tableIndex: 0,
          sec: 0,
          para: 5,
          ctrl: 0,
          rowCount: 3,
          colCount: 4,
          cells: [
            { idx: 0, row: 0, col: 0, text: "문서제목" },
            { idx: 1, row: 0, col: 1, text: "작성일자" },
          ],
        },
      ];

      const mapped = mapInspectedTables(hwpInspectedTables);
      expect(mapped).toHaveLength(1);
      expect(mapped[0].tableIndex).toBe(0);
      expect(mapped[0].sec).toBe(0);
      expect(mapped[0].para).toBe(5);
      expect(mapped[0].ctrl).toBe(0);
      expect(mapped[0].rowCount).toBe(3);
      expect(mapped[0].colCount).toBe(4);
      expect(mapped[0].depth).toBe(0);
      expect(mapped[0].parentTableIndex).toBeUndefined();
      expect(mapped[0].parentCell).toBeUndefined();
      expect(mapped[0].cells[0].text).toBe("문서제목");
    });

    it("(d) mapInspectedTables preserves tableIndex, depth, parentTableIndex, parentCell, and bboxes without overwriting", () => {
      const existingMeta: TableCoordinateMeta[] = [
        {
          tableIndex: 0,
          sec: 0,
          para: 1,
          ctrl: 0,
          rowCount: 2,
          colCount: 2,
          depth: 0,
          cells: [
            {
              idx: 0,
              row: 0,
              col: 0,
              rowSpan: 1,
              colSpan: 1,
              text: "A",
              bbox: { cellIdx: 0, row: 0, col: 0, rowSpan: 1, colSpan: 1, pageIndex: 0, x: 50, y: 50, w: 100, h: 30 },
            },
          ],
        },
      ];

      const inspectedWithNested = [
        {
          tableIndex: 0,
          rowCount: 2,
          colCount: 2,
          depth: 0,
          cells: [{ idx: 0, row: 0, col: 0, text: "A-updated" }],
        },
        {
          tableIndex: 1, // Pre-order index preserved!
          sec: 0,
          para: 1,
          ctrl: 0,
          rowCount: 1,
          colCount: 2,
          depth: 1, // Depth preserved!
          parentTableIndex: 0, // Parent table index preserved!
          parentCell: { row: 0, col: 0 }, // Parent cell preserved!
          cells: [
            { idx: 0, row: 0, col: 0, text: "Sub1", bbox: { cellIdx: 0, row: 0, col: 0, rowSpan: 1, colSpan: 1, pageIndex: 0, x: 55, y: 55, w: 40, h: 20 } },
            { idx: 1, row: 0, col: 1, text: "Sub2" },
          ],
        },
      ];

      const mapped = mapInspectedTables(inspectedWithNested, existingMeta);
      expect(mapped).toHaveLength(2);

      // Table 0 preserves existing bbox from existingMeta
      expect(mapped[0].tableIndex).toBe(0);
      expect(mapped[0].depth).toBe(0);
      expect(mapped[0].cells[0].bbox?.x).toBe(50);
      expect(mapped[0].cells[0].text).toBe("A-updated");

      // Table 1 preserves non-zero depth, parentTableIndex, parentCell
      expect(mapped[1].tableIndex).toBe(1);
      expect(mapped[1].depth).toBe(1);
      expect(mapped[1].parentTableIndex).toBe(0);
      expect(mapped[1].parentCell).toEqual({ row: 0, col: 0 });
      expect(mapped[1].cells[0].bbox?.x).toBe(55);
      expect(mapped[1].cells[1].text).toBe("Sub2");
    });
  });
});
