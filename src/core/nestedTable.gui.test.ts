// @vitest-environment happy-dom
import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import * as fs from "node:fs";
import * as path from "node:path";
import * as os from "node:os";
import { spawn } from "node:child_process";
import { unzipSync, strFromU8 } from "fflate";
import { useDocStore } from "../store/useDocStore.ts";
import * as runtimeInvokeModule from "./runtimeInvoke.ts";
import { hopBridge } from "./hopBridge.ts";
import { parseHwpxTablesFromBytes } from "./hwpxTableIndex.ts";
import { InlineQuickToolbar } from "../components/floating/InlineQuickToolbar";

(globalThis as any).IS_REACT_ACT_ENVIRONMENT = true;

const SAMPLE_HWPX = path.resolve("samples/(날짜)(교시)(과목명)교수.학습과정안(이름).hwpx");
const SCRIPT_PATH = path.resolve("resources/claw-hwp/scripts/hwpx-edit.js");

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

describe("Nested Table GUI & Store Integration (nestedTable.gui.test.ts)", () => {
  let tempDir: string;
  let container: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    vi.restoreAllMocks();
    vi.spyOn(hopBridge, "loadDocumentIntoHop").mockResolvedValue(undefined);
    window.alert = vi.fn();
    tempDir = fs.mkdtempSync(path.join(os.tmpdir(), "nested-gui-test-"));
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);
  });

  afterEach(() => {
    act(() => {
      root.unmount();
    });
    container.remove();
    try {
      fs.rmSync(tempDir, { recursive: true, force: true });
    } catch {}
  });

  it("1. applyHumanNestedTableInsert rejects when current document is not HWPX", async () => {
    useDocStore.setState({
      stagingPath: "mock/file.hwp",
      isHwpx: false,
      tablesMeta: [],
    });

    await expect(
      useDocStore.getState().applyHumanNestedTableInsert(0, 0, 0, 2, 2)
    ).rejects.toThrow("중첩 표는 국가 표준 HWPX 문서에서만 지원됩니다");
  });

  it("2. applyHumanNestedTableInsert executes hwpx-edit.js engine, updates XML structure, and supports undo", async () => {
    const testFile = path.join(tempDir, "sample_nested.hwpx");
    fs.copyFileSync(SAMPLE_HWPX, testFile);
    const initialBytes = fs.readFileSync(testFile);

    // Initial tables verification (1 outer table)
    const initialTables = parseHwpxTablesFromBytes(initialBytes);
    expect(initialTables).toHaveLength(1);
    expect(initialTables[0].depth).toBe(0);

    // Mock runtimeInvoke to delegate execute_claw_ops directly to real hwpx-edit.js
    vi.spyOn(runtimeInvokeModule, "runtimeInvoke").mockImplementation(async (cmd: string, args?: any) => {
      if (cmd === "execute_claw_ops") {
        const payload = JSON.parse(args.payloadJson);
        const result = await runHwpxEdit(payload);
        return {
          ok: result.code === 0,
          stdout_json: result.stdout,
          stderr: result.stderr,
        };
      }
      if (cmd === "read_file_bytes") {
        const bytes = fs.readFileSync(args.filePath);
        return Array.from(bytes);
      }
      if (cmd === "write_file_bytes") {
        fs.writeFileSync(args.filePath, Buffer.from(args.bytes));
        return true;
      }
      if (cmd === "extract_markdown") {
        return "# Markdown";
      }
      return null;
    });

    // Initialize useDocStore state
    useDocStore.setState({
      stagingPath: testFile,
      fileName: "sample_nested.hwpx",
      isHwpx: true,
      fileBytes: initialBytes,
      undoHistoryBytes: [],
      tablesMeta: initialTables,
    });

    // Execute human nested table insertion: 2x2 nested table inside Table 0, Row 8, Col 1
    await useDocStore.getState().applyHumanNestedTableInsert(0, 8, 1, 2, 2);

    // Verify updated store state
    const stateAfterInsert = useDocStore.getState();
    expect(stateAfterInsert.isDirty).toBe(true);
    expect(stateAfterInsert.undoHistoryBytes).toHaveLength(1);
    expect(stateAfterInsert.fileBytes).toBeDefined();

    // Verify parsed tables hierarchy after insertion
    const updatedBytes = stateAfterInsert.fileBytes!;
    const parsedTables = parseHwpxTablesFromBytes(updatedBytes);
    expect(parsedTables).toHaveLength(2);

    // Parent table
    expect(parsedTables[0].tableIndex).toBe(0);
    expect(parsedTables[0].depth).toBe(0);

    // Nested table
    expect(parsedTables[1].tableIndex).toBe(1);
    expect(parsedTables[1].depth).toBe(1);
    expect(parsedTables[1].parentTableIndex).toBe(0);
    expect(parsedTables[1].parentCell).toEqual({ row: 8, col: 1 });
    expect(parsedTables[1].rowCount).toBe(2);
    expect(parsedTables[1].colCount).toBe(2);

    // Verify physical OWPML XML inside the zip
    const unzipped = unzipSync(new Uint8Array(updatedBytes));
    const sectionXmlKey = Object.keys(unzipped).find((k) => /section0\.xml$/i.test(k));
    expect(sectionXmlKey).toBeDefined();
    const sectionXml = strFromU8(unzipped[sectionXmlKey!]);

    // Check that nested <hp:tbl> exists
    const tableTagMatches = sectionXml.match(/<hp:tbl\b/g);
    expect(tableTagMatches).toBeDefined();
    expect(tableTagMatches!.length).toBeGreaterThanOrEqual(2);

    // Verify undoLastChange restores original state
    await useDocStore.getState().undoLastChange();
    const stateAfterUndo = useDocStore.getState();
    expect(stateAfterUndo.undoHistoryBytes).toHaveLength(0);
    const tablesAfterUndo = parseHwpxTablesFromBytes(stateAfterUndo.fileBytes!);
    expect(tablesAfterUndo).toHaveLength(1);
    expect(tablesAfterUndo[0].depth).toBe(0);
  });

  it("3. InlineQuickToolbar renders '표 안에 표' button for table selection and calls applyHumanNestedTableInsert", async () => {
    const applySpy = vi.fn().mockResolvedValue(undefined);

    useDocStore.setState({
      isHwpx: true,
      selection: {
        kind: "table_cells",
        tableIndex: 0,
        sec: 0,
        para: 1,
        ctrl: 0,
        startRow: 1,
        startCol: 1,
        endRow: 1,
        endCol: 1,
        selectedCells: [{ cellIdx: 0, row: 1, col: 1, text: "cell" }],
      },
      applyHumanNestedTableInsert: applySpy,
    });

    await act(async () => {
      root.render(React.createElement(InlineQuickToolbar, { position: { x: 300, y: 300 } }));
    });

    const button = container.querySelector("#quick-btn-nested-table") as HTMLButtonElement;
    expect(button).not.toBeNull();
    expect(button.textContent).toContain("표 안에 표");

    // Click to open modal
    await act(async () => {
      button.click();
    });

    const modal = container.querySelector("#nested-table-modal");
    expect(modal).not.toBeNull();

    const rowsInput = container.querySelector("#nested-table-rows") as HTMLInputElement;
    const colsInput = container.querySelector("#nested-table-cols") as HTMLInputElement;
    expect(rowsInput.value).toBe("2");
    expect(colsInput.value).toBe("2");

    // Change row and col values
    const nativeInputValueSetter = Object.getOwnPropertyDescriptor(
      window.HTMLInputElement.prototype,
      "value"
    )?.set;

    await act(async () => {
      nativeInputValueSetter?.call(rowsInput, "3");
      rowsInput.dispatchEvent(new Event("change", { bubbles: true }));

      nativeInputValueSetter?.call(colsInput, "4");
      colsInput.dispatchEvent(new Event("change", { bubbles: true }));
    });

    const confirmBtn = container.querySelector("#nested-table-confirm") as HTMLButtonElement;
    await act(async () => {
      confirmBtn.click();
    });

    expect(applySpy).toHaveBeenCalledWith(0, 1, 1, 3, 4);
  });

  it("4. InlineQuickToolbar alerts when clicked on non-HWPX file", async () => {
    const alertSpy = vi.spyOn(window, "alert").mockImplementation(() => {});

    useDocStore.setState({
      isHwpx: false,
      selection: {
        kind: "table_cells",
        tableIndex: 0,
        sec: 0,
        para: 1,
        ctrl: 0,
        startRow: 0,
        startCol: 0,
        endRow: 0,
        endCol: 0,
        selectedCells: [{ cellIdx: 0, row: 0, col: 0, text: "" }],
      },
    });

    await act(async () => {
      root.render(React.createElement(InlineQuickToolbar, { position: { x: 300, y: 300 } }));
    });

    const button = container.querySelector("#quick-btn-nested-table") as HTMLButtonElement;
    expect(button).not.toBeNull();

    await act(async () => {
      button.click();
    });

    expect(alertSpy).toHaveBeenCalledWith(
      expect.stringContaining("HWPX")
    );
  });
});
