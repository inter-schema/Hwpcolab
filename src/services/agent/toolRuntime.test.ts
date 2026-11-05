import { describe, it, expect, vi, beforeEach } from "vitest";
import {
  parseToolArgs,
  ToolRuntime,
  isReadOnlyTool,
  DEFAULT_V2_TOOLS,
} from "./toolRuntime.ts";
import { PrivacySessionMasker } from "../../core/privacyMasker.ts";
import * as runtimeInvokeModule from "../../core/runtimeInvoke.ts";
import * as layoutInspectorModule from "../../core/layoutInspector.ts";
import type { LayoutReport } from "../../core/layoutInspector.ts";

vi.mock("../../core/runtimeInvoke.ts", () => ({
  runtimeInvoke: vi.fn(),
}));

vi.mock("../../core/layoutInspector.ts", async () => {
  const actual = await vi.importActual<any>("../../core/layoutInspector.ts");
  return {
    ...actual,
    inspectDocumentBytes: vi.fn(),
    compareLayoutReports: vi.fn(),
    formatLayoutReportForPrompt: vi.fn((report: any) => `[Formatted Report: issues=${report?.issuesCount ?? 0}]`),
  };
});

describe("toolRuntime.ts", () => {
  describe("isReadOnlyTool", () => {
    it("identifies inspect_layout and read_document as read-only", () => {
      expect(isReadOnlyTool("inspect_layout")).toBe(true);
      expect(isReadOnlyTool("read_document")).toBe(true);
      expect(isReadOnlyTool("apply_trial_edits")).toBe(false);
      expect(isReadOnlyTool("propose_hwp_edits")).toBe(false);
    });
  });

  describe("parseToolArgs", () => {
    it("parses valid JSON string", () => {
      const res = parseToolArgs('{"reason": "test", "tableIndex": 0}');
      expect(res).toEqual({ reason: "test", tableIndex: 0 });
    });

    it("parses JSON wrapped in markdown code fence", () => {
      const fenced = '```json\n{"reason": "fenced", "ops": []}\n```';
      const res = parseToolArgs(fenced);
      expect(res).toEqual({ reason: "fenced", ops: [] });
    });

    it("parses JSON with trailing commas", () => {
      const withTrailing = '{"a": 1, "b": [10, 20,],}';
      const res = parseToolArgs(withTrailing);
      expect(res).toEqual({ a: 1, b: [10, 20] });
    });

    it("parses double-encoded JSON string", () => {
      const doubleEncoded = JSON.stringify('{"reason": "double", "val": 42}');
      const res = parseToolArgs(doubleEncoded);
      expect(res).toEqual({ reason: "double", val: 42 });
    });

    it("returns empty object for empty or whitespace string", () => {
      expect(parseToolArgs("")).toEqual({});
      expect(parseToolArgs("   ")).toEqual({});
      expect(parseToolArgs(undefined)).toEqual({});
    });

    it("throws Error for completely unparseable string", () => {
      expect(() => parseToolArgs("not a json at all {[[")).toThrow();
    });
  });

  describe("ToolRuntime Execution", () => {
    let mockEmitStep: any;
    let masker: PrivacySessionMasker;
    const baseBytes = new Uint8Array([0x50, 0x4b, 0x03, 0x04]); // PK.. (hwpx)

    const sampleReport: LayoutReport = {
      tables: [
        {
          tableIndex: 0,
          rowCount: 2,
          colCount: 2,
          cellCount: 4,
          tableWidthMm: 150,
          printableWidthMm: 150,
          cells: [
            { row: 0, col: 0, widthMm: 75, heightMm: 10, overflowAmountMm: 0, textLength: 5, lines: 1 },
            { row: 0, col: 1, widthMm: 75, heightMm: 10, overflowAmountMm: 2.5, textLength: 20, lines: 2 },
          ],
        },
      ],
      issues: [
        {
          severity: "high",
          type: "cell_overflow",
          tableIndex: 0,
          cell: { tableIndex: 0, row: 0, col: 1 },
          message: "표 0 (0행 1열): 2.5mm 넘침",
        },
      ],
      issuesCount: 1,
      pageCount: 1,
      printableBoundsMm: { widthMm: 150, heightMm: 250 },
    };

    beforeEach(() => {
      vi.clearAllMocks();
      mockEmitStep = vi.fn();
      masker = new PrivacySessionMasker();
    });

    it("executes inspect_layout with tableIndex and cell coordinates", async () => {
      const runtime = new ToolRuntime({
        initialReport: sampleReport,
        masker,
        emitStep: mockEmitStep,
      });

      const resStr = await runtime.executeToolCall({
        id: "call_inspect_1",
        name: "inspect_layout",
        argumentsJson: JSON.stringify({ tableIndex: 0, row: 0, col: 1 }),
      });

      const res = JSON.parse(resStr);
      expect(res.ok).toBe(true);
      expect(res.cell.overflowAmountMm).toBe(2.5);
      expect(res.issues.length).toBe(1);
      expect(mockEmitStep).toHaveBeenCalledWith(
        expect.objectContaining({ type: "inspect" })
      );
    });

    it("returns unknown tool error without throwing", async () => {
      const runtime = new ToolRuntime({
        masker,
        emitStep: mockEmitStep,
      });

      const resStr = await runtime.executeToolCall({
        id: "call_bad_tool",
        name: "non_existent_tool",
        argumentsJson: "{}",
      });

      const res = JSON.parse(resStr);
      expect(res.ok).toBe(false);
      expect(res.error).toContain("알 수 없는 도구");
      expect(res.availableTools).toBeDefined();
    });

    it("returns argument parse error on broken JSON without throwing", async () => {
      const runtime = new ToolRuntime({
        masker,
        emitStep: mockEmitStep,
      });

      const resStr = await runtime.executeToolCall({
        id: "call_broken_json",
        name: "inspect_layout",
        argumentsJson: "this is totally { not json",
      });

      const res = JSON.parse(resStr);
      expect(res.ok).toBe(false);
      expect(res.error).toContain("인자 파싱 실패");
      expect(res.hint).toBeDefined();
    });

    it("masks synthetic PII in tool results", async () => {
      const runtime = new ToolRuntime({
        masker,
        emitStep: mockEmitStep,
        autoMaskPii: true,
        docMarkdown: "홍길동 010-1234-5678 hong@example.com",
      });

      const resStr = await runtime.executeToolCall({
        id: "call_read_pii",
        name: "read_document",
        argumentsJson: JSON.stringify({ kind: "paragraphs", start: 0, count: 1 }),
      });

      expect(resStr).not.toContain("010-1234-5678");
      expect(resStr).not.toContain("hong@example.com");
    });

    it("uses a single fixed trial file path across multiple apply_trial_edits (파일 단일성)", async () => {
      const writtenPaths = new Set<string>();

      vi.mocked(runtimeInvokeModule.runtimeInvoke).mockImplementation(async (cmd: string, args: any) => {
        if (cmd === "write_file_bytes") {
          writtenPaths.add(args.filePath);
          return null as any;
        }
        if (cmd === "execute_claw_ops") {
          return { ok: true, stdout_json: {}, stderr: "" } as any;
        }
        if (cmd === "read_file_bytes") {
          return Array.from(baseBytes) as any;
        }
        return null as any;
      });

      vi.mocked(layoutInspectorModule.inspectDocumentBytes).mockResolvedValue(sampleReport);
      vi.mocked(layoutInspectorModule.compareLayoutReports).mockReturnValue({
        ok: true,
        summaryText: "이슈 1건 모두 해소",
        resolvedIssues: [sampleReport.issues[0]],
        remainingIssues: [],
        newIssues: [],
      });

      const runtime = new ToolRuntime({
        requestId: "test-req-123",
        stagingPath: "C:\\Users\\test\\doc.hwpx",
        baseBytes,
        initialReport: sampleReport,
        masker,
        emitStep: mockEmitStep,
      });

      // Trial 1
      await runtime.executeToolCall({
        id: "call_trial_1",
        name: "apply_trial_edits",
        argumentsJson: JSON.stringify({
          reason: "시험 1",
          ops: [{ type: "set_col_width", tableIndex: 0, col: 1, widthMm: 80 }],
        }),
      });

      // Trial 2
      await runtime.executeToolCall({
        id: "call_trial_2",
        name: "apply_trial_edits",
        argumentsJson: JSON.stringify({
          reason: "시험 2",
          ops: [{ type: "set_row_height", tableIndex: 0, row: 0, heightMm: 15 }],
        }),
      });

      // Check single file path
      expect(writtenPaths.size).toBe(1);
      const singlePath = Array.from(writtenPaths)[0];
      expect(singlePath).toContain("trial_test_req_123.hwpx");
      expect(runtime.trialsRun).toBe(2);
    });

    it("resets cumulative state back to baseBytes when reset: true", async () => {
      const writtenBytesList: number[][] = [];

      vi.mocked(runtimeInvokeModule.runtimeInvoke).mockImplementation(async (cmd: string, args: any) => {
        if (cmd === "write_file_bytes") {
          writtenBytesList.push(args.bytes);
          return null as any;
        }
        if (cmd === "execute_claw_ops") {
          return { ok: true, stdout_json: {}, stderr: "" } as any;
        }
        if (cmd === "read_file_bytes") {
          // Return modified bytes
          return [0x50, 0x4b, 0x99, 0x99] as any;
        }
        return null as any;
      });

      vi.mocked(layoutInspectorModule.inspectDocumentBytes).mockResolvedValue(sampleReport);
      vi.mocked(layoutInspectorModule.compareLayoutReports).mockReturnValue({
        ok: true,
        summaryText: "해소",
        resolvedIssues: [],
        remainingIssues: [],
        newIssues: [],
      });

      const runtime = new ToolRuntime({
        requestId: "test-reset",
        stagingPath: "C:\\Users\\test\\doc.hwpx",
        baseBytes,
        initialReport: sampleReport,
        masker,
        emitStep: mockEmitStep,
      });

      // Call 1: cumulative
      await runtime.executeToolCall({
        id: "t1",
        name: "apply_trial_edits",
        argumentsJson: JSON.stringify({
          reason: "Call 1",
          ops: [{ type: "set_col_width", tableIndex: 0, col: 0, widthMm: 70 }],
        }),
      });
      // After Call 1, currentBytes is modified [0x50, 0x4b, 0x99, 0x99]
      expect(runtime.currentBytes?.[2]).toBe(0x99);

      // Call 2: reset: true
      await runtime.executeToolCall({
        id: "t2",
        name: "apply_trial_edits",
        argumentsJson: JSON.stringify({
          reason: "Call 2 reset",
          reset: true,
          ops: [{ type: "set_col_width", tableIndex: 0, col: 0, widthMm: 65 }],
        }),
      });

      // For call 2, written bytes should have started from baseBytes ([0x50, 0x4b, 0x03, 0x04])
      expect(writtenBytesList[1][2]).toBe(0x03);
    });
  });
});
