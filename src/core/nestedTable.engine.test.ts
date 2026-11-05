import { describe, it, expect, afterAll } from "vitest";
import * as fs from "node:fs";
import * as path from "node:path";
import * as os from "node:os";
import { spawn } from "node:child_process";
import { unzipSync, strFromU8 } from "fflate";
import { parseHwpxTablesFromBytes } from "./hwpxTableIndex.ts";

const SCRIPT_PATH = path.resolve("resources/claw-hwp/scripts/hwpx-edit.js");
const SAMPLE_HWPX = path.resolve("samples/(날짜)(교시)(과목명)교수.학습과정안(이름).hwpx");

function runHwpxEdit(payload: any): Promise<{ code: number; stdout: any; stderr: string }> {
  return new Promise((resolvePromise, rejectPromise) => {
    const child = spawn("node", [SCRIPT_PATH], { stdio: ["pipe", "pipe", "pipe"] });
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

describe("Stream A: Real Engine Integration (nestedTable.engine.test.ts)", () => {
  const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), "nested-tbl-test-"));

  afterAll(() => {
    try {
      fs.rmSync(tempDir, { recursive: true, force: true });
    } catch {}
  });

  it("applies insert_nested_table on real HWPX sample and verifies structure", async () => {
    const testFile = path.join(tempDir, "test_nested.hwpx");
    fs.copyFileSync(SAMPLE_HWPX, testFile);

    // Initial inspection
    const beforeInspect = await runHwpxEdit({ path: testFile, inspect: "cells" });
    expect(beforeInspect.code).toBe(0);
    expect(beforeInspect.stdout.tables).toHaveLength(1);
    const initialCellCount = beforeInspect.stdout.tables[0].cells.length;

    // Apply insert_nested_table into Table 0, Row 8, Col 1
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
            ["세부과정A", "시간"],
            ["단계1", "10분"],
          ],
          headerRow: true,
          widthMode: "fit_cell",
        },
      ],
    });

    expect(editRes.code).toBe(0);
    expect(editRes.stdout.ok).toBe(true);

    // 1. Verify tablesDeep count incremented by 1 (1 -> 2 tables)
    const afterInspect = await runHwpxEdit({ path: testFile, inspect: "cells" });
    expect(afterInspect.code).toBe(0);
    expect(afterInspect.stdout.tables).toHaveLength(2);

    // Table 1 should be the newly inserted 2x2 nested table
    const nestedTableMeta = afterInspect.stdout.tables[1];
    expect(nestedTableMeta.rows).toBe(2);
    expect(nestedTableMeta.cells.some((c: any) => c.text === "세부과정A")).toBe(true);
    expect(nestedTableMeta.cells.some((c: any) => c.text === "단계1")).toBe(true);

    // 2. Read ZIP and inspect section XML directly
    const bytes = fs.readFileSync(testFile);
    expect(bytes[0]).toBe(0x50);
    expect(bytes[1]).toBe(0x4b);

    const unzipped = unzipSync(bytes);
    const sec0Xml = strFromU8(unzipped["Contents/section0.xml"]);

    // Must have nested <hp:tbl> inside <hp:tc>
    expect(sec0Xml).toContain("<hp:tbl");
    // Verify target cell still preserves original text
    expect(sec0Xml).toContain("분수의 나눗셈으로 바꾸어 24÷3과 비교하기");
    // Verify nested text is present in XML
    expect(sec0Xml).toContain("세부과정A");
    expect(sec0Xml).toContain("10분");

    // 3. Verify parseHwpxTablesFromBytes parses depth and parent coordinates correctly
    const parsedTables = parseHwpxTablesFromBytes(bytes);
    expect(parsedTables).toHaveLength(2);

    expect(parsedTables[0].depth).toBe(0);
    expect(parsedTables[0].parentTableIndex).toBeUndefined();

    expect(parsedTables[1].depth).toBe(1);
    expect(parsedTables[1].parentTableIndex).toBe(0);
    expect(parsedTables[1].parentCell).toEqual({ row: 8, col: 1 });
    expect(parsedTables[1].rowCount).toBe(2);
    expect(parsedTables[1].colCount).toBe(2);
  });

  it("supports depth 2 nested table and rejects depth >= 2 parent (max depth 2)", async () => {
    const testFile = path.join(tempDir, "test_deep.hwpx");
    fs.copyFileSync(SAMPLE_HWPX, testFile);

    // Insert level 1 nested table into Table 0
    await runHwpxEdit({
      path: testFile,
      output: testFile,
      operations: [
        {
          type: "insert_nested_table",
          table: 0,
          row: 0,
          col: 1,
          rows: 2,
          cols: 2,
          data: [["L1_A", "L1_B"], ["L1_C", "L1_D"]],
        },
      ],
    });

    // Insert level 2 nested table into Table 1 (target depth 1 -> results in depth 2)
    const level2Res = await runHwpxEdit({
      path: testFile,
      output: testFile,
      operations: [
        {
          type: "insert_nested_table",
          table: 1,
          row: 0,
          col: 0,
          rows: 1,
          cols: 1,
          data: [["L2_Nested"]],
        },
      ],
    });
    expect(level2Res.code).toBe(0);
    expect(level2Res.stdout.ok).toBe(true);

    const inspectAfterL2 = await runHwpxEdit({ path: testFile, inspect: "cells" });
    expect(inspectAfterL2.stdout.tables).toHaveLength(3);

    const bytes = fs.readFileSync(testFile);
    const parsed = parseHwpxTablesFromBytes(bytes);
    expect(parsed).toHaveLength(3);
    expect(parsed[0].depth).toBe(0);
    expect(parsed[1].depth).toBe(1);
    expect(parsed[1].parentTableIndex).toBe(0);
    expect(parsed[2].depth).toBe(2);
    expect(parsed[2].parentTableIndex).toBe(1);

    // Now attempt to insert into Table 2 (current depth 2 -> would exceed max depth 2)
    const level3Res = await runHwpxEdit({
      path: testFile,
      output: testFile,
      operations: [
        {
          type: "insert_nested_table",
          table: 2,
          row: 0,
          col: 0,
          rows: 1,
          cols: 1,
          data: [["L3_Forbidden"]],
        },
      ],
    });
    expect(level3Res.code).toBe(1);
    expect(level3Res.stdout.ok).toBe(false);
    expect(level3Res.stdout.error).toContain("nesting depth limit exceeded");
  });

  it("regression: insert_table still functions and produces valid HWPX package", async () => {
    const testFile = path.join(tempDir, "test_regression_insert.hwpx");
    fs.copyFileSync(SAMPLE_HWPX, testFile);

    const res = await runHwpxEdit({
      path: testFile,
      output: testFile,
      operations: [
        {
          type: "insert_table",
          index: -1,
          rows: 2,
          cols: 3,
          cells: [
            ["헤더1", "헤더2", "헤더3"],
            ["데이터1", "데이터2", "데이터3"],
          ],
        },
      ],
    });

    expect(res.code).toBe(0);
    expect(res.stdout.ok).toBe(true);

    const inspectRes = await runHwpxEdit({ path: testFile, inspect: "cells" });
    expect(inspectRes.code).toBe(0);
    // Table count should increase by 1
    expect(inspectRes.stdout.tables).toHaveLength(2);
    expect(inspectRes.stdout.tables[0].cells.some((c: any) => c.text === "헤더1")).toBe(true);
  });
});
