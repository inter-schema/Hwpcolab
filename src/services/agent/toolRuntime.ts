import type { UnifiedOp } from "../../types/operations.ts";
import type { TableCoordinateMeta, SelectionTarget } from "../../types/hwp.ts";
import type { ExcludedRegion, PrivacySessionMasker } from "../../core/privacyMasker.ts";
import {
  inspectDocumentBytes,
  formatLayoutReportForPrompt,
  compareLayoutReports,
  type LayoutReport,
} from "../../core/layoutInspector.ts";
import { OpTranslator } from "../../core/opTranslator.ts";
import { runtimeInvoke } from "../../core/runtimeInvoke.ts";
import { lintOps } from "../policyGuard.ts";
import type { AgentStepEvent } from "../aiOrchestrator.ts";
import type { NeutralToolCall, NeutralToolSpec } from "./types.ts";
import { truncateToolResult, executeReadDocument } from "./context.ts";

export const INSPECT_LAYOUT_TOOL: NeutralToolSpec = {
  name: "inspect_layout",
  description: "현재 문서(또는 시험 적용 중인 문서)의 표 및 셀 레이아웃 측정값, 오버플로/넘침 수치를 정밀 조회합니다.",
  parameters: {
    type: "object",
    properties: {
      tableIndex: { type: "number", description: "조회할 표 인덱스 (0-based, 생략 시 전체 표)" },
      row: { type: "number", description: "조회할 셀 행 (0-based)" },
      col: { type: "number", description: "조회할 셀 열 (0-based)" },
    },
  },
};

export const READ_DOCUMENT_TOOL: NeutralToolSpec = {
  name: "read_document",
  description:
    "문서 본문의 특정 문단 범위 또는 특정 표의 셀 데이터를 부분 조회합니다. 긴 문서(12,000자 초과)에서 필요한 세부 내용을 읽을 때 사용합니다.",
  parameters: {
    type: "object",
    required: ["kind"],
    properties: {
      kind: {
        type: "string",
        enum: ["paragraphs", "table"],
        description: "조회할 대상 종류 ('paragraphs' 문단 또는 'table' 표)",
      },
      start: { type: "number", description: "조회 시작 인덱스 (0-based)" },
      count: { type: "number", description: "조회할 문단 또는 행 수 (기본 5)" },
      tableIndex: { type: "number", description: "kind가 'table'일 때 조회할 표 인덱스 (0-based)" },
    },
  },
};

export const APPLY_TRIAL_EDITS_TOOL: NeutralToolSpec = {
  name: "apply_trial_edits",
  description:
    "임시 복사본 파일에 시험 편집 연산을 시뮬레이션 적용하고 레이아웃 변화 및 넘침 해결 여부를 실측 비교합니다. 기본적으로 누적(cumulative) 적용되며, reset: true로 원본으로 복구할 수 있습니다.",
  parameters: {
    type: "object",
    required: ["reason", "ops"],
    properties: {
      reason: { type: "string", description: "시험 적용 목적 및 가설" },
      ops: {
        type: "array",
        items: { type: "object", required: ["type"] },
        description: "시험 적용할 UnifiedOp 배열",
      },
      reset: {
        type: "boolean",
        description: "true이면 이전 시험 변경사항을 모두 취소하고 원본 상태로 복귀 후 연산 적용",
      },
    },
  },
};

export const PROPOSE_HWP_EDITS_TOOL_SPEC: NeutralToolSpec = {
  name: "propose_hwp_edits",
  description:
    "HWP/HWPX 문서에 대한 부분 수정 또는 생성 작업(DiffItem 배열)을 제안합니다. 각 변경 단위는 사용자가 화면에서 개별적으로 [✔ 적용] 또는 [✖ 되돌리기]를 선택할 수 있도록 문단 또는 표 셀 단위로 분리하여 반환하세요.",
  parameters: {
    type: "object",
    required: ["summary", "diffItems"],
    properties: {
      summary: {
        type: "string",
        description: "사이드바에 표시할 전체 수정 사항 요약 메시지",
      },
      diffItems: {
        type: "array",
        items: {
          type: "object",
          required: ["scope", "title", "beforeText", "afterText", "unifiedOps"],
          properties: {
            scope: {
              type: "string",
              enum: ["paragraph", "cell", "table_structure", "style", "document"],
            },
            title: { type: "string", description: "변경 위치 요약 (예: '표 1 (2행 3열)', '문단 2')" },
            beforeText: { type: "string", description: "변경 전 원본 텍스트 (신규 추가 시 빈 문자열)" },
            afterText: { type: "string", description: "변경 후 텍스트 (시각적 Diff 표시용)" },
            targetCoord: {
              type: "object",
              properties: {
                sec: { type: "number" },
                para: { type: "number" },
                tableIndex: { type: "number" },
                row: { type: "number" },
                col: { type: "number" },
              },
            },
            unifiedOps: {
              type: "array",
              description: "이 DiffItem을 실제로 반영하기 위한 UnifiedOp 배열",
              items: {
                type: "object",
                required: ["type"],
              },
            },
          },
        },
      },
    },
  },
};

export const DEFAULT_V2_TOOLS: NeutralToolSpec[] = [
  INSPECT_LAYOUT_TOOL,
  READ_DOCUMENT_TOOL,
  APPLY_TRIAL_EDITS_TOOL,
  PROPOSE_HWP_EDITS_TOOL_SPEC,
];

/**
 * Check if a tool is a read-only query that can run concurrently with other read tools.
 */
export function isReadOnlyTool(name: string): boolean {
  return name === "inspect_layout" || name === "read_document";
}

/**
 * Robust JSON arguments parser for tool calls.
 * Handles:
 * - Code fences (```json ... ``` or ``` ... ```)
 * - Double-encoded JSON strings ("{...}")
 * - Trailing commas before closing brace or bracket
 * - Leading / trailing whitespace
 */
export function parseToolArgs(raw: string | undefined | null): Record<string, any> {
  if (!raw || typeof raw !== "string" || raw.trim().length === 0) {
    return {};
  }

  let text = raw.trim();

  // Strip markdown code fences if present
  const fenceMatch = text.match(/^```(?:json)?\s*([\s\S]*?)\s*```$/i);
  if (fenceMatch) {
    text = fenceMatch[1].trim();
  }

  // Attempt 1: Standard JSON parse
  try {
    const parsed = JSON.parse(text);
    if (typeof parsed === "string") {
      // Double encoded JSON string: parse once more
      try {
        const doubleParsed = JSON.parse(parsed);
        if (doubleParsed && typeof doubleParsed === "object") {
          return doubleParsed;
        }
      } catch {}
    }
    if (parsed && typeof parsed === "object") {
      return parsed;
    }
  } catch {}

  // Attempt 2: Clean trailing commas
  try {
    const cleaned = text.replace(/,\s*([\]}])/g, "$1");
    const parsed = JSON.parse(cleaned);
    if (typeof parsed === "string") {
      const doubleParsed = JSON.parse(parsed);
      if (doubleParsed && typeof doubleParsed === "object") {
        return doubleParsed;
      }
    }
    if (parsed && typeof parsed === "object") {
      return parsed;
    }
  } catch {}

  // Attempt 3: If wrapped in an outer JSON string (escaped quotes)
  try {
    if (text.startsWith('"') && text.endsWith('"')) {
      const unescaped = JSON.parse(text);
      if (typeof unescaped === "string") {
        return parseToolArgs(unescaped);
      }
    }
  } catch {}

  throw new Error(`Invalid JSON arguments: ${raw.slice(0, 100)}`);
}

export interface ToolRuntimeOptions {
  requestId?: string;
  baseBytes?: Uint8Array | null;
  initialReport?: LayoutReport | null;
  stagingPath?: string;
  isHwpx?: boolean;
  tablesMeta?: TableCoordinateMeta[];
  excludedRegions?: ExcludedRegion[];
  autoMaskPii?: boolean;
  userPrompt?: string;
  docMarkdown?: string;
  masker: PrivacySessionMasker;
  emitStep: (event: AgentStepEvent) => void;
  availableTools?: NeutralToolSpec[];
}

/**
 * Stateful Tool Execution Runtime for v2 Agent Loop.
 * Handles cumulative trial edits on a fixed request-scoped file path,
 * result masking, and length truncation.
 */
export class ToolRuntime {
  public trialsRun: number = 0;
  public currentBytes: Uint8Array | null = null;
  public currentReport: LayoutReport | null = null;
  public readonly baseBytes: Uint8Array | null = null;
  public readonly initialReport: LayoutReport | null = null;

  private fixedTrialPath: string | null = null;
  private readonly requestId: string;
  private readonly stagingPath?: string;
  private readonly isHwpx: boolean;
  private readonly tablesMeta: TableCoordinateMeta[];
  private readonly excludedRegions: ExcludedRegion[];
  private readonly autoMaskPii: boolean;
  private readonly userPrompt: string;
  private readonly docMarkdown: string;
  private readonly masker: PrivacySessionMasker;
  private readonly emitStep: (event: AgentStepEvent) => void;
  private readonly availableTools: NeutralToolSpec[];

  constructor(opts: ToolRuntimeOptions) {
    this.requestId = opts.requestId || "req_" + Math.random().toString(36).slice(2, 10);
    this.baseBytes = opts.baseBytes ?? null;
    this.currentBytes = opts.baseBytes ? new Uint8Array(opts.baseBytes) : null;
    this.initialReport = opts.initialReport ?? null;
    this.currentReport = opts.initialReport ?? null;
    this.stagingPath = opts.stagingPath;
    this.isHwpx = Boolean(
      opts.isHwpx ??
        (opts.stagingPath?.endsWith(".hwpx") ||
          (opts.baseBytes && opts.baseBytes[0] === 0x50 && opts.baseBytes[1] === 0x4b))
    );
    this.tablesMeta = opts.tablesMeta || [];
    this.excludedRegions = opts.excludedRegions || [];
    this.autoMaskPii = opts.autoMaskPii ?? false;
    this.userPrompt = opts.userPrompt || "";
    this.docMarkdown = opts.docMarkdown || "";
    this.masker = opts.masker;
    this.emitStep = opts.emitStep;
    this.availableTools = opts.availableTools || DEFAULT_V2_TOOLS;
  }

  /**
   * Get the single, fixed trial file path for this request.
   * Ensures that successive apply_trial_edits overwrite the same file.
   */
  public getFixedTrialPath(): string | null {
    if (!this.stagingPath) return null;
    if (!this.fixedTrialPath) {
      const ext = this.isHwpx ? "hwpx" : "hwp";
      const safeReqId = this.requestId.replace(/[^a-zA-Z0-9]/g, "_");
      this.fixedTrialPath = this.stagingPath.replace(/[^\\/]+$/, `trial_${safeReqId}.${ext}`);
    }
    return this.fixedTrialPath;
  }

  /**
   * Execute a single tool call and return the masked, truncated result string.
   * Guarantees never throwing an exception; all errors are returned as result content.
   */
  public async executeToolCall(call: NeutralToolCall): Promise<string> {
    let rawOutput: string;
    try {
      rawOutput = await this.executeInternal(call);
    } catch (err: any) {
      rawOutput = JSON.stringify({
        ok: false,
        error: `도구 실행 오류: ${err?.message || String(err)}`,
      });
    }

    // Pass every tool output through privacy masking and length truncation
    const masked = this.masker.maskTextForPrompt(
      rawOutput,
      this.excludedRegions,
      this.autoMaskPii
    );
    return truncateToolResult(masked, 4000);
  }

  private async executeInternal(call: NeutralToolCall): Promise<string> {
    // 1. Argument parsing with recovery
    let args: Record<string, any>;
    try {
      args = parseToolArgs(call.argumentsJson);
    } catch (parseErr: any) {
      return JSON.stringify({
        ok: false,
        error: "인자 파싱 실패: 유효한 JSON 객체 형식이어야 합니다.",
        hint: "코드펜스나 불필요한 따옴표 없이 올바른 JSON 객체 형태로 전달하세요.",
        raw: call.argumentsJson?.slice(0, 80),
      });
    }

    // 2. Dispatch to specific tool
    switch (call.name) {
      case "inspect_layout":
        return this.handleInspectLayout(args);

      case "read_document":
        return this.handleReadDocument(args);

      case "apply_trial_edits":
        return await this.handleApplyTrialEdits(args);

      default:
        return JSON.stringify({
          ok: false,
          error: `알 수 없는 도구: '${call.name}'`,
          availableTools: this.availableTools.map((t) => t.name),
        });
    }
  }

  private handleInspectLayout(args: Record<string, any>): string {
    this.emitStep({ type: "inspect", message: "도구 호출: 레이아웃 세부 수치 조회" });

    if (!this.currentReport) {
      return "레이아웃 리포트를 사용할 수 없습니다.";
    }

    if (typeof args.tableIndex === "number") {
      const tbl = this.currentReport.tables.find((t) => t.tableIndex === args.tableIndex);
      if (!tbl) {
        return JSON.stringify({ ok: false, error: `Table ${args.tableIndex} not found` });
      }

      if (typeof args.row === "number" && typeof args.col === "number") {
        const cell = tbl.cells.find((c) => c.row === args.row && c.col === args.col);
        const cellIssues = this.currentReport.issues.filter(
          (i) =>
            i.cell?.tableIndex === args.tableIndex &&
            i.cell?.row === args.row &&
            i.cell?.col === args.col
        );
        return JSON.stringify({ ok: true, cell, issues: cellIssues });
      }

      const tableIssues = this.currentReport.issues.filter((i) => i.tableIndex === args.tableIndex);
      return JSON.stringify({
        ok: true,
        tableWidthMm: tbl.tableWidthMm,
        printableWidthMm: tbl.printableWidthMm,
        cellCount: tbl.cellCount,
        issues: tableIssues,
      });
    }

    return formatLayoutReportForPrompt(this.currentReport);
  }

  private handleReadDocument(args: Record<string, any>): string {
    this.emitStep({
      type: "inspect",
      message: `도구 호출: 문서 부분 조회 (${args.kind || "paragraphs"})`,
    });
    return executeReadDocument(args as any, this.docMarkdown, this.tablesMeta);
  }

  private async handleApplyTrialEdits(args: Record<string, any>): Promise<string> {
    this.trialsRun += 1;
    this.emitStep({
      type: "trial",
      message: `도구 호출: 시험 적용 (${args.reason || "시뮬레이션"})`,
    });

    // Reset to base if requested
    if (args.reset === true && this.baseBytes) {
      this.currentBytes = new Uint8Array(this.baseBytes);
      this.currentReport = this.initialReport;
    }

    const trialOps: UnifiedOp[] = Array.isArray(args.ops) ? args.ops : [];

    // 1. Static Policy Guard
    const guardContext = {
      userPrompt: this.userPrompt,
      isNewDocument: false,
      excludedRegions: this.excludedRegions,
      layoutReport: this.currentReport || this.initialReport,
      tablesMeta: this.tablesMeta,
    };
    const guardRes = lintOps(trialOps, guardContext);

    if (!guardRes.passed) {
      this.emitStep({
        type: "guard",
        message: "시험 적용 정책 가드 위반",
        detail: guardRes.rejectedOps.map((r) => r.reason).join("; "),
      });
      return JSON.stringify({
        ok: false,
        guardRejected: guardRes.rejectedOps,
        feedback: `정책 가드 위반으로 시험 적용이 취소되었습니다:\n${guardRes.rejectedOps.map((r) => r.reason).join("\n")}`,
      });
    }

    // If no binary or staging path available, report static validation only
    const trialPath = this.getFixedTrialPath();
    const sourceBytes = this.currentBytes || this.baseBytes;
    if (!sourceBytes || !trialPath) {
      return JSON.stringify({
        ok: true,
        note: "파일 바이너리 부재로 정적 가드만 통과했습니다.",
      });
    }

    // 2. Write to the single fixed trial file (cumulative overwrite)
    try {
      await runtimeInvoke("write_file_bytes", {
        filePath: trialPath,
        bytes: Array.from(sourceBytes),
      });

      const unmaskedTrialOps = this.masker.unmaskAndFilterOps(
        guardRes.sanitizedOps,
        this.excludedRegions
      );
      const payload = OpTranslator.toEnginePayload(
        trialPath,
        this.isHwpx,
        unmaskedTrialOps,
        this.tablesMeta,
        false
      );

      const clawRes = await runtimeInvoke<{ ok: boolean; stdout_json: any; stderr: string }>(
        "execute_claw_ops",
        {
          isHwpx: this.isHwpx,
          payloadJson: payload.stdinJson,
        }
      );

      if (!clawRes.ok) {
        return JSON.stringify({
          ok: false,
          error: clawRes.stderr || "Claw execution error",
        });
      }

      // Read back patched bytes for cumulative state
      const patchedBytesArr = await runtimeInvoke<number[]>("read_file_bytes", {
        filePath: trialPath,
      });
      const patchedBytes = new Uint8Array(patchedBytesArr);
      const trialReport = await inspectDocumentBytes(patchedBytes);
      const comparison = compareLayoutReports(this.initialReport || trialReport, trialReport);

      // Update cumulative state
      this.currentBytes = patchedBytes;
      this.currentReport = trialReport;

      this.emitStep({
        type: "verify",
        message: `시험 적용 실측: ${comparison.summaryText}`,
      });

      return JSON.stringify({
        ok: true,
        summary: comparison.summaryText,
        resolvedCount: comparison.resolvedIssues.length,
        remainingCount: comparison.remainingIssues.length,
        newCount: comparison.newIssues.length,
        details: formatLayoutReportForPrompt(trialReport),
      });
    } catch (err: any) {
      return JSON.stringify({
        ok: false,
        error: `시험 적용 처리 중 오류: ${err?.message || String(err)}`,
      });
    }
  }
}
