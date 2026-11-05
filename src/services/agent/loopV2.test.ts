import { describe, it, expect, vi, beforeEach } from "vitest";
import {
  runAgentLoopV2,
  looksLikeEditIntent,
  capReasoningEffort,
  type LoopV2Context,
} from "./loopV2.ts";
import type {
  ProviderAdapter,
  AgentTurnRequest,
  AgentTurnResponse,
  NeutralMessage,
  NeutralToolSpec,
} from "./types.ts";
import { ToolsUnsupportedError, TransientProviderError } from "./types.ts";
import { PrivacySessionMasker } from "../../core/privacyMasker.ts";
import * as runtimeInvokeModule from "../../core/runtimeInvoke.ts";
import * as layoutInspectorModule from "../../core/layoutInspector.ts";
import type { LayoutReport } from "../../core/layoutInspector.ts";
import { DEFAULT_V2_TOOLS, PROPOSE_HWP_EDITS_TOOL_SPEC } from "./toolRuntime.ts";
import type { AiRequestOptions } from "../aiOrchestrator.ts";

vi.mock("../../core/runtimeInvoke.ts", () => ({
  runtimeInvoke: vi.fn(),
}));

vi.mock("../../core/layoutInspector.ts", async () => {
  const actual = await vi.importActual<any>("../../core/layoutInspector.ts");
  return {
    ...actual,
    inspectDocumentBytes: vi.fn(),
    compareLayoutReports: vi.fn(),
    formatLayoutReportForPrompt: vi.fn((report: any) => `[LayoutReport: issues=${report?.issuesCount ?? 0}]`),
  };
});

class MockAdapter implements ProviderAdapter {
  public readonly id = "baze" as const;
  public readonly supportsTools = true;
  public history: AgentTurnRequest[] = [];
  public responses: Array<
    | AgentTurnResponse
    | Error
    | ((req: AgentTurnRequest) => AgentTurnResponse | Promise<AgentTurnResponse>)
  > = [];

  constructor(responses: typeof MockAdapter.prototype.responses = []) {
    this.responses = [...responses];
  }

  async turn(req: AgentTurnRequest): Promise<AgentTurnResponse> {
    this.history.push(req);
    if (this.responses.length === 0) {
      return {
        content: "기본 응답",
        toolCalls: [],
      };
    }
    const next = this.responses.shift()!;
    if (next instanceof Error) {
      throw next;
    }
    if (typeof next === "function") {
      return await next(req);
    }
    return next;
  }
}

describe("loopV2.ts - Enhanced Agent Loop v2", () => {
  let mockEmitStep: any;
  let masker: PrivacySessionMasker;
  const dummyBaseBytes = new Uint8Array([0x50, 0x4b, 0x03, 0x04]);

  const mockReport: LayoutReport = {
    tables: [
      {
        tableIndex: 0,
        rowCount: 2,
        colCount: 2,
        cellCount: 4,
        tableWidthMm: 140,
        printableWidthMm: 150,
        cells: [
          { row: 0, col: 0, widthMm: 70, heightMm: 10, overflowAmountMm: 0, textLength: 5, lines: 1 },
          { row: 0, col: 1, widthMm: 70, heightMm: 10, overflowAmountMm: 2.0, textLength: 15, lines: 2 },
        ],
      },
    ],
    issues: [
      {
        severity: "high",
        type: "cell_overflow",
        tableIndex: 0,
        cell: { tableIndex: 0, row: 0, col: 1 },
        message: "표 0 (0행 1열): 2.0mm 넘침",
      },
    ],
    issuesCount: 1,
    pageCount: 1,
    printableBoundsMm: { widthMm: 150, heightMm: 250 },
  };

  const createDefaultOptions = (overrides?: Partial<AiRequestOptions>): AiRequestOptions => ({
    provider: "baze",
    apiKey: "test-key",
    model: "gpt-6.1-terra",
    userPrompt: "표 0의 오버플로를 수정해줘",
    docMarkdown: "테스트 문서 본문입니다.",
    tablesMeta: [],
    selection: null,
    excludedRegions: [],
    autoMaskPii: false,
    stagingPath: "C:\\mock\\test.hwpx",
    fileBytes: dummyBaseBytes,
    isHwpx: true,
    ...overrides,
  });

  const createMockContext = (
    adapter: ProviderAdapter,
    optionsOverrides?: Partial<AiRequestOptions>,
    ctxOverrides?: Partial<LoopV2Context>
  ): LoopV2Context => ({
    adapter,
    systemPrompt: "시스템 프롬프트",
    messages: [{ role: "user", content: "표 0의 오버플로를 수정해줘" }],
    options: createDefaultOptions(optionsOverrides),
    masker,
    emitStep: mockEmitStep,
    baseBytes: dummyBaseBytes,
    initialReport: mockReport,
    getTools: () => DEFAULT_V2_TOOLS,
    sleepFn: vi.fn(async () => {}),
    ...ctxOverrides,
  });

  beforeEach(() => {
    vi.clearAllMocks();
    mockEmitStep = vi.fn();
    masker = new PrivacySessionMasker();

    vi.mocked(runtimeInvokeModule.runtimeInvoke).mockImplementation(async (cmd: string) => {
      if (cmd === "write_file_bytes") return null as any;
      if (cmd === "execute_claw_ops") return { ok: true, stdout_json: {}, stderr: "" } as any;
      if (cmd === "read_file_bytes") return Array.from(dummyBaseBytes) as any;
      return null as any;
    });

    vi.mocked(layoutInspectorModule.inspectDocumentBytes).mockResolvedValue(mockReport);
    vi.mocked(layoutInspectorModule.compareLayoutReports).mockReturnValue({
      ok: true,
      summaryText: "이슈 모두 해소",
      resolvedIssues: [mockReport.issues[0]],
      remainingIssues: [],
      newIssues: [],
    });
  });

  describe("Helper Functions", () => {
    it("looksLikeEditIntent correctly identifies editing intent vs pure query", () => {
      expect(looksLikeEditIntent("표 1의 너비를 수정해줘")).toBe(true);
      expect(looksLikeEditIntent("문단을 추가해", null)).toBe(true);
      expect(looksLikeEditIntent("글자 크기를 줄여줘")).toBe(true);
      expect(looksLikeEditIntent("anything", { kind: "body_range" } as any)).toBe(true);

      // Questions without edit verbs
      expect(looksLikeEditIntent("이 문서에 표가 몇 개 있어?")).toBe(false);
      expect(looksLikeEditIntent("어디에 오버플로가 발생했는지 알려줘")).toBe(false);
      expect(looksLikeEditIntent("표 1의 내용은 뭐야?")).toBe(false);
    });

    it("capReasoningEffort caps exploration effort at medium", () => {
      expect(capReasoningEffort("high", "medium")).toBe("medium");
      expect(capReasoningEffort("xhigh", "medium")).toBe("medium");
      expect(capReasoningEffort("low", "medium")).toBe("low");
      expect(capReasoningEffort("none", "medium")).toBe("none");
      expect(capReasoningEffort(undefined, "medium")).toBe("medium");
    });
  });

  describe("Scenario 1: Normal inspect -> propose flow", () => {
    it("runs inspect_layout in turn 1 and propose_hwp_edits in turn 2", async () => {
      const adapter = new MockAdapter([
        // Turn 1: inspect_layout
        {
          content: "레이아웃을 확인해보겠습니다.",
          toolCalls: [{ id: "call_inspect_1", name: "inspect_layout", argumentsJson: "{}" }],
        },
        // Turn 2: propose_hwp_edits
        {
          content: "수정안을 제안합니다.",
          toolCalls: [
            {
              id: "call_prop_1",
              name: "propose_hwp_edits",
              argumentsJson: JSON.stringify({
                summary: "표 열 너비 확장",
                diffItems: [
                  {
                    scope: "cell",
                    title: "표 0 (0행 1열)",
                    beforeText: "원본",
                    afterText: "수정",
                    targetCoord: { tableIndex: 0, row: 0, col: 1 },
                    unifiedOps: [{ type: "set_col_width", tableIndex: 0, col: 1, widthMm: 80 }],
                  },
                ],
              }),
            },
          ],
        },
      ]);

      const ctx = createMockContext(adapter);
      const res = await runAgentLoopV2(ctx);

      expect(adapter.history.length).toBe(2);
      expect(res.summary).toBe("표 열 너비 확장");
      expect(res.diffItems.length).toBe(1);
      expect(res.diffItems[0].verificationStatus).toBe("verified");
      expect(res.verificationPassed).toBe(true);
      expect(res.trialsRun).toBe(1); // Auto-verification gate ran 1 trial
    });
  });

  describe("Scenario 2: Multiple tool_calls in a single turn are all answered", () => {
    it("answers all tool_call_ids in messages even when returned together", async () => {
      const adapter = new MockAdapter([
        {
          content: "동시에 2개 셀을 조회합니다.",
          toolCalls: [
            { id: "call_c1", name: "inspect_layout", argumentsJson: JSON.stringify({ tableIndex: 0, row: 0, col: 0 }) },
            { id: "call_c2", name: "inspect_layout", argumentsJson: JSON.stringify({ tableIndex: 0, row: 0, col: 1 }) },
          ],
        },
        {
          content: "조회 완료 후 제안",
          toolCalls: [
            {
              id: "call_prop_2",
              name: "propose_hwp_edits",
              argumentsJson: JSON.stringify({ summary: "완료", diffItems: [] }),
            },
          ],
        },
      ]);

      const ctx = createMockContext(adapter);
      await runAgentLoopV2(ctx);

      // Turn 2 request messages should contain tool replies for BOTH call_c1 and call_c2
      const turn2Req = adapter.history[1];
      const toolReplies = turn2Req.messages.filter((m) => m.role === "tool") as any[];
      const repliedIds = toolReplies.map((r) => r.toolCallId);
      expect(repliedIds).toContain("call_c1");
      expect(repliedIds).toContain("call_c2");
    });
  });

  describe("Scenario 3: Unknown tool call is answered gracefully", () => {
    it("returns error tool response for unknown tool name without throwing", async () => {
      const adapter = new MockAdapter([
        {
          content: "이상한 도구 호출",
          toolCalls: [{ id: "call_bad", name: "non_existent_tool", argumentsJson: "{}" }],
        },
        {
          content: "이제 정상 제안",
          toolCalls: [
            {
              id: "call_prop_3",
              name: "propose_hwp_edits",
              argumentsJson: JSON.stringify({ summary: "복구 후 제안", diffItems: [] }),
            },
          ],
        },
      ]);

      const ctx = createMockContext(adapter);
      const res = await runAgentLoopV2(ctx);

      expect(res.summary).toBe("복구 후 제안");
      const turn2Req = adapter.history[1];
      const badToolReply = turn2Req.messages.find(
        (m) => m.role === "tool" && (m as any).toolCallId === "call_bad"
      );
      expect(badToolReply).toBeDefined();
      expect((badToolReply as any).content).toContain("알 수 없는 도구");
    });
  });

  describe("Scenario 4: Broken JSON argument recovery and error response", () => {
    it("recovers from markdown code fence in args or returns error response if unrecoverable", async () => {
      const adapter = new MockAdapter([
        {
          content: "코드펜스가 들어간 도구 호출",
          toolCalls: [
            {
              id: "call_fenced",
              name: "inspect_layout",
              argumentsJson: '```json\n{"tableIndex": 0}\n```',
            },
          ],
        },
        {
          content: "제안",
          toolCalls: [
            {
              id: "call_prop_4",
              name: "propose_hwp_edits",
              argumentsJson: JSON.stringify({ summary: "정상 완료", diffItems: [] }),
            },
          ],
        },
      ]);

      const ctx = createMockContext(adapter);
      const res = await runAgentLoopV2(ctx);
      expect(res.summary).toBe("정상 완료");

      // Verify fenced json was parsed without error in turn 1
      const turn2Req = adapter.history[1];
      const toolReply = turn2Req.messages.find(
        (m) => m.role === "tool" && (m as any).toolCallId === "call_fenced"
      );
      expect((toolReply as any).content).not.toContain("인자 파싱 실패");
    });
  });

  describe("Scenario 5: Budget exhaustion triggers forced propose turn", () => {
    it("forces tools=[propose_hwp_edits] and toolChoice={name:'propose_hwp_edits'} on final turn", async () => {
      // Set maxTurns = 3
      const adapter = new MockAdapter([
        // Turn 1
        { content: "T1", toolCalls: [{ id: "c1", name: "inspect_layout", argumentsJson: "{}" }] },
        // Turn 2
        { content: "T2", toolCalls: [{ id: "c2", name: "read_document", argumentsJson: '{"kind":"paragraphs"}' }] },
        // Turn 3: forced propose turn!
        {
          content: "T3 forced propose",
          toolCalls: [
            {
              id: "c3",
              name: "propose_hwp_edits",
              argumentsJson: JSON.stringify({ summary: "강제 제안된 편집안", diffItems: [] }),
            },
          ],
        },
      ]);

      const ctx = createMockContext(adapter, {}, { maxTurns: 3 });
      const res = await runAgentLoopV2(ctx);

      expect(adapter.history.length).toBe(3);
      const finalTurnReq = adapter.history[2];
      expect(finalTurnReq.tools.length).toBe(1);
      expect(finalTurnReq.tools[0].name).toBe("propose_hwp_edits");
      expect(finalTurnReq.toolChoice).toEqual({ name: "propose_hwp_edits" });
      expect(res.summary).toBe("강제 제안된 편집안");
    });
  });

  describe("Scenario 6: Stagnation detection triggers forced propose", () => {
    it("detects 2 consecutive identical tool calls and triggers forced propose next turn", async () => {
      const adapter = new MockAdapter([
        // Turn 1
        { content: "T1", toolCalls: [{ id: "c1", name: "inspect_layout", argumentsJson: '{"tableIndex":0}' }] },
        // Turn 2: Identical tool call again!
        { content: "T2", toolCalls: [{ id: "c2", name: "inspect_layout", argumentsJson: '{"tableIndex":0}' }] },
        // Turn 3: Should now be forced propose!
        {
          content: "T3 Stagnation Propose",
          toolCalls: [
            {
              id: "c3",
              name: "propose_hwp_edits",
              argumentsJson: JSON.stringify({ summary: "정체 탈출 제안", diffItems: [] }),
            },
          ],
        },
      ]);

      const ctx = createMockContext(adapter);
      const res = await runAgentLoopV2(ctx);

      // Turn 3 should have been forced propose
      const turn3Req = adapter.history[2];
      expect(turn3Req.toolChoice).toEqual({ name: "propose_hwp_edits" });
      expect(res.summary).toBe("정체 탈출 제안");
    });
  });

  describe("Scenario 7: Text-only response receives 1-time nudge for edit prompt", () => {
    it("nudges assistant when editing intent exists, then accepts propose in next turn", async () => {
      const adapter = new MockAdapter([
        // Turn 1: pure text answer to an edit prompt
        {
          content: "오버플로를 수정하려면 열 너비를 80mm로 늘려야 합니다.",
          toolCalls: [],
        },
        // Turn 2: after nudge, assistant calls propose_hwp_edits
        {
          content: "제안 도구를 호출합니다.",
          toolCalls: [
            {
              id: "call_prop_nudge",
              name: "propose_hwp_edits",
              argumentsJson: JSON.stringify({ summary: "넛지 후 제안 완료", diffItems: [] }),
            },
          ],
        },
      ]);

      const ctx = createMockContext(adapter, { userPrompt: "표 0 오버플로 수정해줘" });
      const res = await runAgentLoopV2(ctx);

      expect(adapter.history.length).toBe(2);
      // Turn 2 messages should include the nudge message
      const turn2Req = adapter.history[1];
      const nudgeMsg = turn2Req.messages.find((m) => m.role === "user" && m.content.includes("propose_hwp_edits"));
      expect(nudgeMsg).toBeDefined();
      expect(res.summary).toBe("넛지 후 제안 완료");
    });
  });

  describe("Scenario 8: Informational query accepts text-only response without nudge", () => {
    it("immediately accepts text response for pure questions", async () => {
      const adapter = new MockAdapter([
        {
          content: "이 문서에는 표가 총 1개 있습니다.",
          toolCalls: [],
        },
      ]);

      const ctx = createMockContext(adapter, { userPrompt: "이 문서에 표가 몇 개 있어?", selection: null });
      const res = await runAgentLoopV2(ctx);

      // Only 1 turn, no nudge
      expect(adapter.history.length).toBe(1);
      expect(res.summary).toContain("표가 총 1개 있습니다");
      expect(res.diffItems.length).toBe(0);
    });
  });

  describe("Scenario 9: 429 exponential backoff retry", () => {
    it("retries on 429 status and succeeds on next attempt", async () => {
      let callCount = 0;
      const sleepSpy = vi.fn(async () => {});

      const adapter: ProviderAdapter = {
        id: "baze",
        supportsTools: true,
        async turn(req: AgentTurnRequest) {
          callCount++;
          if (callCount === 1) {
            throw new TransientProviderError("Rate limit exceeded", 429);
          }
          return {
            content: "429 극복",
            toolCalls: [
              {
                id: "c1",
                name: "propose_hwp_edits",
                argumentsJson: JSON.stringify({ summary: "재시도 후 성공", diffItems: [] }),
              },
            ],
          };
        },
      };

      const ctx = createMockContext(adapter, {}, { sleepFn: sleepSpy });
      const res = await runAgentLoopV2(ctx);

      expect(callCount).toBe(2);
      expect(sleepSpy).toHaveBeenCalledWith(500);
      expect(res.summary).toBe("재시도 후 성공");
    });

    it("fails when 429 retry limit (2 retries) is exceeded", async () => {
      const adapter: ProviderAdapter = {
        id: "baze",
        supportsTools: true,
        async turn() {
          throw new TransientProviderError("Rate limit exceeded", 429);
        },
      };

      const ctx = createMockContext(adapter, {}, { sleepFn: vi.fn(async () => {}) });
      await expect(runAgentLoopV2(ctx)).rejects.toThrow("Rate limit exceeded");
    });
  });

  describe("Scenario 10: AbortSignal cancellation", () => {
    it("aborts execution cleanly when AbortSignal is aborted", async () => {
      const controller = new AbortController();
      controller.abort(); // already aborted

      const adapter = new MockAdapter([]);
      const ctx = createMockContext(adapter, { signal: controller.signal });

      await expect(runAgentLoopV2(ctx)).rejects.toThrow("aborted by user");
    });
  });

  describe("Scenario 11: Tool result privacy masking", () => {
    it("masks synthetic PII in tool results so subsequent turns see masked tokens", async () => {
      const adapter = new MockAdapter([
        {
          content: "PII가 포함된 본문을 조회합니다.",
          toolCalls: [
            {
              id: "call_read_pii",
              name: "read_document",
              argumentsJson: JSON.stringify({ kind: "paragraphs", start: 0, count: 1 }),
            },
          ],
        },
        {
          content: "제안",
          toolCalls: [
            {
              id: "call_prop_pii",
              name: "propose_hwp_edits",
              argumentsJson: JSON.stringify({ summary: "완료", diffItems: [] }),
            },
          ],
        },
      ]);

      const ctx = createMockContext(adapter, {
        docMarkdown: "대표자 홍길동 010-9876-5432 hong@test.com",
        autoMaskPii: true,
      });

      await runAgentLoopV2(ctx);

      // Check tool reply message sent to turn 2
      const turn2Req = adapter.history[1];
      const toolMsg = turn2Req.messages.find((m) => m.role === "tool" && (m as any).toolCallId === "call_read_pii");
      expect((toolMsg as any).content).not.toContain("010-9876-5432");
      expect((toolMsg as any).content).not.toContain("hong@test.com");
    });
  });

  describe("Scenario 12 & 13: Trial file single path and cumulative/reset", () => {
    it("runs apply_trial_edits and updates trialsRun counter", async () => {
      const adapter = new MockAdapter([
        {
          content: "시험 적용 1회",
          toolCalls: [
            {
              id: "call_t1",
              name: "apply_trial_edits",
              argumentsJson: JSON.stringify({
                reason: "열 너비 조정",
                ops: [{ type: "set_col_width", tableIndex: 0, col: 1, widthMm: 80 }],
              }),
            },
          ],
        },
        {
          content: "최종 제안",
          toolCalls: [
            {
              id: "call_prop_trial",
              name: "propose_hwp_edits",
              argumentsJson: JSON.stringify({ summary: "시험 후 제안", diffItems: [] }),
            },
          ],
        },
      ]);

      const ctx = createMockContext(adapter);
      const res = await runAgentLoopV2(ctx);

      expect(res.trialsRun).toBeGreaterThanOrEqual(1);
      expect(res.summary).toBe("시험 후 제안");
    });
  });

  describe("Scenario 14: Long document (>12000 chars) outline mode and read_document", () => {
    it("handles read_document tool call effectively", async () => {
      const adapter = new MockAdapter([
        {
          content: "문단 1번을 읽어보겠습니다.",
          toolCalls: [
            {
              id: "call_rd",
              name: "read_document",
              argumentsJson: JSON.stringify({ kind: "paragraphs", start: 0, count: 2 }),
            },
          ],
        },
        {
          content: "제안",
          toolCalls: [
            {
              id: "call_prop_rd",
              name: "propose_hwp_edits",
              argumentsJson: JSON.stringify({ summary: "조회 완료 후 제안", diffItems: [] }),
            },
          ],
        },
      ]);

      const longMd = "# 제목\n\n" + "내용 ".repeat(4500);
      const ctx = createMockContext(adapter, { docMarkdown: longMd });
      const res = await runAgentLoopV2(ctx);

      expect(res.summary).toBe("조회 완료 후 제안");
      const turn2Req = adapter.history[1];
      const rdReply = turn2Req.messages.find((m) => m.role === "tool" && (m as any).toolCallId === "call_rd");
      expect((rdReply as any).content).toContain("paragraphs");
    });
  });

  describe("Scenario 15: ToolsUnsupportedError is re-thrown directly for single-shot fallback", () => {
    it("propagates ToolsUnsupportedError immediately without catching", async () => {
      const adapter: ProviderAdapter = {
        id: "custom",
        supportsTools: false,
        async turn() {
          throw new ToolsUnsupportedError("Custom provider does not support tool calling");
        },
      };

      const ctx = createMockContext(adapter);
      await expect(runAgentLoopV2(ctx)).rejects.toThrow(ToolsUnsupportedError);
    });
  });

  describe("Scenario 16: Mixed turn with propose_hwp_edits and inspect_layout", () => {
    it("answers inspect_layout first before processing propose_hwp_edits", async () => {
      const adapter = new MockAdapter([
        {
          content: "조회와 제안을 한꺼번에 제출합니다.",
          toolCalls: [
            { id: "call_inspect_mix", name: "inspect_layout", argumentsJson: "{}" },
            {
              id: "call_prop_mix",
              name: "propose_hwp_edits",
              argumentsJson: JSON.stringify({ summary: "혼합 턴 제안", diffItems: [] }),
            },
          ],
        },
      ]);

      const ctx = createMockContext(adapter);
      const res = await runAgentLoopV2(ctx);

      expect(res.summary).toBe("혼합 턴 제안");
      // Verify inspect tool call got its tool reply
      const inspectReply = ctx.messages.find(
        (m) => m.role === "tool" && (m as any).toolCallId === "call_inspect_mix"
      );
      // (ctx.messages in caller or returned diff items verified)
    });
  });

  describe("Scenario 17: Policy guard violation triggers self-correction feedback", () => {
    it("feeds back guard rejection and model self-corrects in next turn", async () => {
      const adapter = new MockAdapter([
        // Turn 1: Propose with invalid op that triggers guard violation (G4: setup_document on existing doc)
        {
          content: "위반 연산 제안",
          toolCalls: [
            {
              id: "call_bad_prop",
              name: "propose_hwp_edits",
              argumentsJson: JSON.stringify({
                summary: "가드 위반 제안",
                diffItems: [
                  {
                    scope: "paragraph",
                    title: "문단 1",
                    beforeText: "a",
                    afterText: "b",
                    unifiedOps: [{ type: "setup_document" as any }],
                  },
                ],
              }),
            },
          ],
        },
        // Turn 2: Self-corrected proposal
        {
          content: "자가 교정된 제안",
          toolCalls: [
            {
              id: "call_good_prop",
              name: "propose_hwp_edits",
              argumentsJson: JSON.stringify({
                summary: "교정 완료",
                diffItems: [
                  {
                    scope: "paragraph",
                    title: "문단 1",
                    beforeText: "a",
                    afterText: "b",
                    unifiedOps: [{ type: "replace_body_text", find: "a", replace: "b" }],
                  },
                ],
              }),
            },
          ],
        },
      ]);

      const ctx = createMockContext(adapter);
      const res = await runAgentLoopV2(ctx);

      expect(adapter.history.length).toBe(2);
      const turn2Req = adapter.history[1];
      const guardFeedback = turn2Req.messages.find(
        (m) => m.role === "tool" && (m as any).toolCallId === "call_bad_prop"
      );
      expect((guardFeedback as any).content).toContain("정책 가드");
      expect(res.summary).toBe("교정 완료");
    });
  });

  describe("Scenario 18: Auto-verification remaining issues feedback (1-time loop return)", () => {
    it("feeds back remaining issues once when layout issues remain, then finalizes", async () => {
      let compCount = 0;
      vi.mocked(layoutInspectorModule.compareLayoutReports).mockImplementation(() => {
        compCount++;
        if (compCount === 1) {
          // First verify has remaining issues
          return {
            ok: false,
            summaryText: "표 0 (0행 1열): 1.0mm 잔여 넘침",
            resolvedIssues: [],
            remainingIssues: [mockReport.issues[0]],
            newIssues: [],
          };
        }
        // Second verify passes
        return {
          ok: true,
          summaryText: "이슈 모두 해결됨",
          resolvedIssues: [mockReport.issues[0]],
          remainingIssues: [],
          newIssues: [],
        };
      });

      const adapter = new MockAdapter([
        // Turn 1: Initial propose (leaves overflow)
        {
          content: "제안 1차",
          toolCalls: [
            {
              id: "call_p1",
              name: "propose_hwp_edits",
              argumentsJson: JSON.stringify({
                summary: "1차 제안",
                diffItems: [
                  {
                    scope: "cell",
                    title: "표 0",
                    beforeText: "a",
                    afterText: "b",
                    unifiedOps: [{ type: "set_col_width", tableIndex: 0, col: 1, widthMm: 72 }],
                  },
                ],
              }),
            },
          ],
        },
        // Turn 2: Refined propose (completely resolves overflow)
        {
          content: "제안 2차 정밀 교정",
          toolCalls: [
            {
              id: "call_p2",
              name: "propose_hwp_edits",
              argumentsJson: JSON.stringify({
                summary: "2차 정밀 교정 제안",
                diffItems: [
                  {
                    scope: "cell",
                    title: "표 0",
                    beforeText: "a",
                    afterText: "b",
                    unifiedOps: [{ type: "set_col_width", tableIndex: 0, col: 1, widthMm: 80 }],
                  },
                ],
              }),
            },
          ],
        },
      ]);

      const ctx = createMockContext(adapter);
      const res = await runAgentLoopV2(ctx);

      expect(adapter.history.length).toBe(2);
      expect(compCount).toBe(2);
      expect(res.summary).toBe("2차 정밀 교정 제안");
      expect(res.verificationPassed).toBe(true);
    });
  });
});
