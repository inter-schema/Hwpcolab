import { describe, it, expect, vi, beforeEach } from "vitest";
import * as fs from "node:fs";
import * as path from "node:path";
import * as os from "node:os";
import { spawn } from "node:child_process";
import { useDocStore } from "./useDocStore.ts";
import * as runtimeInvokeModule from "../core/runtimeInvoke.ts";
import { hopBridge } from "../core/hopBridge.ts";

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

describe("DEFECT-03 S2 Store-level Integration: useDocStore tablesMeta synchronization", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
    vi.spyOn(hopBridge, "loadDocumentIntoHop").mockResolvedValue(undefined);
  });

  it("populates tablesMeta on openDocumentBytes for HWPX even though inspect_document returns no tables", async () => {
    const sampleBytes = fs.readFileSync(SAMPLE_HWPX);

    // Mock runtimeInvoke
    const mockInvoke = vi.spyOn(runtimeInvokeModule, "runtimeInvoke").mockImplementation(async (cmd: string, args?: any) => {
      if (cmd === "create_blank_session") {
        return {
          staging_path: "mock/staging.hwpx",
          original_path: null,
          file_name: "sample.hwpx",
          is_hwpx: true,
          bytes: [],
        };
      }
      if (cmd === "write_file_bytes") {
        return true;
      }
      if (cmd === "extract_markdown") {
        return "# Sample Markdown Content";
      }
      if (cmd === "inspect_document") {
        // Reproduce actual extract_text.js behavior on HWPX: NO tables field!
        return {
          fileType: "hwpx",
          sectionCount: 1,
          paragraphCount: 15,
          tableCount: 1,
          cellCount: 40,
        };
      }
      if (cmd === "read_file_bytes") {
        return Array.from(sampleBytes);
      }
      return null;
    });

    // Initial store state: tablesMeta is empty
    useDocStore.setState({ tablesMeta: [] });
    expect(useDocStore.getState().tablesMeta).toHaveLength(0);

    // Open HWPX document bytes
    await useDocStore.getState().openDocumentBytes(sampleBytes, "sample.hwpx");

    // tablesMeta must NOT be empty: populated via buildTablesMetaForHwpx
    const tablesMeta = useDocStore.getState().tablesMeta;
    expect(tablesMeta.length).toBeGreaterThanOrEqual(1);
    expect(tablesMeta[0].depth).toBe(0);
    expect(tablesMeta[0].rowCount).toBeGreaterThan(0);
    expect(tablesMeta[0].cells.length).toBeGreaterThan(0);
  });

  it("synchronizes nested table hierarchy (depth, parentTableIndex, parentCell) on HWPX in useDocStore", async () => {
    const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), "defect03-store-"));
    try {
      const testFile = path.join(tempDir, "nested_doc.hwpx");
      fs.copyFileSync(SAMPLE_HWPX, testFile);

      // Apply insert_nested_table into Table 0, Row 8, Col 1
      await runHwpxEdit({
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
              ["헤더1", "헤더2"],
              ["값1", "값2"],
            ],
            headerRow: true,
            widthMode: "fit_cell",
          },
        ],
      });

      const nestedBytes = fs.readFileSync(testFile);

      vi.spyOn(runtimeInvokeModule, "runtimeInvoke").mockImplementation(async (cmd: string, args?: any) => {
        if (cmd === "create_blank_session") {
          return {
            staging_path: testFile,
            original_path: null,
            file_name: "nested_doc.hwpx",
            is_hwpx: true,
            bytes: [],
          };
        }
        if (cmd === "write_file_bytes") return true;
        if (cmd === "extract_markdown") return "# Nested Content";
        if (cmd === "inspect_document") {
          return { fileType: "hwpx", sectionCount: 1 };
        }
        if (cmd === "read_file_bytes") return Array.from(nestedBytes);
        return null;
      });

      await useDocStore.getState().openDocumentBytes(nestedBytes, "nested_doc.hwpx");

      const tablesMeta = useDocStore.getState().tablesMeta;
      expect(tablesMeta).toHaveLength(2);

      // Table 0 is parent (depth 0)
      expect(tablesMeta[0].tableIndex).toBe(0);
      expect(tablesMeta[0].depth).toBe(0);
      expect(tablesMeta[0].parentTableIndex).toBeUndefined();

      // Table 1 is nested child (depth 1, parent 0, cell [8, 1])
      expect(tablesMeta[1].tableIndex).toBe(1);
      expect(tablesMeta[1].depth).toBe(1);
      expect(tablesMeta[1].parentTableIndex).toBe(0);
      expect(tablesMeta[1].parentCell).toEqual({ row: 8, col: 1 });
      expect(tablesMeta[1].rowCount).toBe(2);
      expect(tablesMeta[1].colCount).toBe(2);
      expect(tablesMeta[1].cells.some((c) => c.text === "헤더1")).toBe(true);
    } finally {
      fs.rmSync(tempDir, { recursive: true, force: true });
    }
  });

  it("preserves .hwp path behavior using mapInspectedTables with depth=0", async () => {
    vi.spyOn(runtimeInvokeModule, "runtimeInvoke").mockImplementation(async (cmd: string, args?: any) => {
      if (cmd === "create_blank_session") {
        return {
          staging_path: "mock/sample.hwp",
          original_path: null,
          file_name: "sample.hwp",
          is_hwpx: false,
          bytes: [],
        };
      }
      if (cmd === "write_file_bytes") return true;
      if (cmd === "extract_markdown") return "# HWP Markdown";
      if (cmd === "inspect_document") {
        return {
          fileType: "hwp",
          tables: [
            {
              tableIndex: 0,
              sec: 0,
              para: 2,
              ctrl: 0,
              rowCount: 2,
              colCount: 2,
              cells: [
                { idx: 0, row: 0, col: 0, text: "HwpA" },
                { idx: 1, row: 0, col: 1, text: "HwpB" },
              ],
            },
          ],
        };
      }
      return null;
    });

    const fakeHwpBytes = new Uint8Array([0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1]);
    await useDocStore.getState().openDocumentBytes(fakeHwpBytes, "sample.hwp");

    const tablesMeta = useDocStore.getState().tablesMeta;
    expect(tablesMeta).toHaveLength(1);
    expect(tablesMeta[0].tableIndex).toBe(0);
    expect(tablesMeta[0].depth).toBe(0);
    expect(tablesMeta[0].parentTableIndex).toBeUndefined();
    expect(tablesMeta[0].parentCell).toBeUndefined();
    expect(tablesMeta[0].cells[0].text).toBe("HwpA");
  });

  it("(6) re-synchronizes tablesMeta when document is refreshed after AI edit and undo", async () => {
    const sampleBytes = fs.readFileSync(SAMPLE_HWPX);

    vi.spyOn(runtimeInvokeModule, "runtimeInvoke").mockImplementation(async (cmd: string, args?: any) => {
      if (cmd === "create_blank_session") {
        return {
          staging_path: "mock/edit_refresh.hwpx",
          original_path: null,
          file_name: "edit_refresh.hwpx",
          is_hwpx: true,
          bytes: [],
        };
      }
      if (cmd === "write_file_bytes") return true;
      if (cmd === "extract_markdown") return "# Content";
      if (cmd === "inspect_document") return { fileType: "hwpx" };
      if (cmd === "read_file_bytes") return Array.from(sampleBytes);
      return null;
    });

    // 1. Open document: 1 table
    await useDocStore.getState().openDocumentBytes(sampleBytes, "edit_refresh.hwpx");
    expect(useDocStore.getState().tablesMeta).toHaveLength(1);

    // 2. Simulate AI edit updating fileBytes with nested table
    const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), "defect03-ai-edit-"));
    try {
      const testFile = path.join(tempDir, "ai_edited.hwpx");
      fs.copyFileSync(SAMPLE_HWPX, testFile);

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
            data: [["AI_A", "AI_B"], ["AI_C", "AI_D"]],
          },
        ],
      });

      const updatedBytes = fs.readFileSync(testFile);
      useDocStore.setState({ fileBytes: updatedBytes });

      // Refresh store (as done in acceptDiffItem / acceptAllDiffs)
      await useDocStore.getState().refreshDocMarkdown();

      // Now store tablesMeta must reflect 2 tables
      const editedMeta = useDocStore.getState().tablesMeta;
      expect(editedMeta).toHaveLength(2);
      expect(editedMeta[0].depth).toBe(0);
      expect(editedMeta[1].depth).toBe(1);
      expect(editedMeta[1].parentTableIndex).toBe(0);
      expect(editedMeta[1].parentCell).toEqual({ row: 0, col: 1 });

      // 3. Simulate undo restoring original bytes
      useDocStore.setState({ fileBytes: sampleBytes });
      await useDocStore.getState().refreshDocMarkdown();

      // Store tablesMeta must revert back to 1 table
      const revertedMeta = useDocStore.getState().tablesMeta;
      expect(revertedMeta).toHaveLength(1);
      expect(revertedMeta[0].depth).toBe(0);
    } finally {
      fs.rmSync(tempDir, { recursive: true, force: true });
    }
  });
});
