import { describe, it, expect, vi, beforeEach } from "vitest";
import { orchestrateAiEdit, AiRequestOptions } from "./aiOrchestrator.ts";
import * as legacyBridgeModule from "./agent/legacyBridge.ts";
import * as openaiProviderModule from "./providers/openaiProvider.ts";
import * as claudeProviderModule from "./providers/claudeProvider.ts";
import * as geminiProviderModule from "./providers/geminiProvider.ts";
import * as ollamaProviderModule from "./providers/ollamaProvider.ts";
import * as runtimeModule from "../core/runtimeInvoke.ts";
import { ToolsUnsupportedError } from "./agent/types.ts";
import { PrivacySessionMasker } from "../core/privacyMasker.ts";

vi.mock("../core/runtimeInvoke.ts", () => ({
  runtimeInvoke: vi.fn(),
  isTauriEnv: () => false,
}));

describe("aiOrchestrator Legacy Multi-Turn Agent Loop & Invariance", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  const baseOptions: AiRequestOptions = {
    provider: "openai",
    apiKey: "test-key",
    model: "gpt-4o",
    docMarkdown: "테스트 문서 본문",
    tablesMeta: [],
    selection: null,
    excludedRegions: [],
    autoMaskPii: true,
    userPrompt: "오버플로를 점검하고 표를 수정해주세요",
    loopMode: "legacy",
  };

  it("(i) verifies [inspect_layout -> propose_hwp_edits] sequence and message ordering for openai/baze", async () => {
    const capturedTurns: any[] = [];
    vi.spyOn(legacyBridgeModule, "callLegacyTurn").mockImplementation(async (params) => {
      capturedTurns.push({
        messages: JSON.parse(JSON.stringify(params.messages)),
        systemPrompt: params.systemPrompt,
      });

      if (capturedTurns.length === 1) {
        // Turn 1: Model calls inspect_layout
        return {
          message: {
            role: "assistant",
            tool_calls: [
              {
                id: "call_inspect_1",
                type: "function",
                function: {
                  name: "inspect_layout",
                  arguments: "{}",
                },
              },
            ],
          },
          toolCalls: [
            {
              id: "call_inspect_1",
              type: "function",
              function: {
                name: "inspect_layout",
                arguments: "{}",
              },
            },
          ],
          content: "레이아웃을 먼저 검측하겠습니다.",
          reasoningTokens: 10,
        };
      }

      // Turn 2: Model proposes edits
      return {
        message: {
          role: "assistant",
          tool_calls: [
            {
              id: "call_propose_1",
              type: "function",
              function: {
                name: "propose_hwp_edits",
                arguments: JSON.stringify({
                  summary: "표 레이아웃 수정 완료",
                  diffItems: [],
                }),
              },
            },
          ],
        },
        toolCalls: [
          {
            id: "call_propose_1",
            type: "function",
            function: {
              name: "propose_hwp_edits",
              arguments: JSON.stringify({
                summary: "표 레이아웃 수정 완료",
                diffItems: [],
              }),
            },
          },
        ],
        content: "수정안을 제안합니다.",
        reasoningTokens: 15,
      };
    });

    const steps: string[] = [];
    const res = await orchestrateAiEdit({
      ...baseOptions,
      onStep: (e) => steps.push(e.message),
    });

    expect(capturedTurns.length).toBe(2);

    // Turn 1 should contain user prompt
    const turn1Msgs = capturedTurns[0].messages;
    expect(turn1Msgs[turn1Msgs.length - 1].role).toBe("user");

    // Turn 2 messages must contain assistant tool_calls and tool result
    const turn2Msgs = capturedTurns[1].messages;
    expect(turn2Msgs.length).toBe(turn1Msgs.length + 2);
    expect(turn2Msgs[turn2Msgs.length - 2].role).toBe("assistant");
    expect(turn2Msgs[turn2Msgs.length - 2].tool_calls[0].function.name).toBe("inspect_layout");
    expect(turn2Msgs[turn2Msgs.length - 1].role).toBe("tool");
    expect(turn2Msgs[turn2Msgs.length - 1].tool_call_id).toBe("call_inspect_1");

    expect(res.summary).toBe("표 레이아웃 수정 완료");
  });

  it("(ii) masks synthetic PII (RRN and phone) inside inspect_layout tool result before sending to LLM", async () => {
    let turn2ToolContent = "";

    // Mock layout inspection to return an issue containing synthetic PII
    const layoutModule = await import("../core/layoutInspector.ts");
    vi.spyOn(layoutModule, "inspectDocumentBytes").mockResolvedValueOnce({
      timestamp: Date.now(),
      tableCount: 1,
      issuesCount: 1,
      issues: [
        {
          id: "issue-1",
          type: "overflow",
          severity: "high",
          message: "담당자 홍길동 880101-1234567 연락처 010-1234-5678 셀 너비 초과",
          cell: { tableIndex: 0, row: 0, col: 0 },
        },
      ],
      tables: [
        {
          tableIndex: 0,
          section: 0,
          para: 0,
          ctrl: 0,
          cellCount: 1,
          rowCount: 1,
          colCount: 1,
          tableWidthMm: 150,
          printableWidthMm: 150,
          cells: [
            {
              row: 0,
              col: 0,
              widthMm: 150,
              heightMm: 20,
              hasOverflow: true,
              overflowAmountMm: 5,
            },
          ],
        },
      ],
    });

    vi.spyOn(legacyBridgeModule, "callLegacyTurn").mockImplementation(async (params) => {
      if (params.messages.some((m) => m.role === "tool")) {
        const toolMsg = params.messages.find((m) => m.role === "tool");
        turn2ToolContent = toolMsg?.content || "";
        return {
          message: { role: "assistant" },
          toolCalls: [
            {
              id: "call_propose_2",
              type: "function",
              function: {
                name: "propose_hwp_edits",
                arguments: '{"summary":"완료","diffItems":[]}',
              },
            },
          ],
          content: "완료",
          reasoningTokens: 0,
        };
      }

      // First turn: requests specific cell
      return {
        message: { role: "assistant" },
        toolCalls: [
          {
            id: "call_inspect_pii",
            type: "function",
            function: {
              name: "inspect_layout",
              arguments: JSON.stringify({ tableIndex: 0, row: 0, col: 0 }),
            },
          },
        ],
        content: "점검 중",
        reasoningTokens: 0,
      };
    });

    const masker = new PrivacySessionMasker();
    await orchestrateAiEdit({
      ...baseOptions,
      fileBytes: new Uint8Array([0x50, 0x4b, 0x03, 0x04]),
      autoMaskPii: true,
      masker,
    });

    // The tool result must NOT contain raw resident registration numbers or phone numbers
    expect(turn2ToolContent).not.toMatch(/880101-1234567/);
    expect(turn2ToolContent).not.toMatch(/010-1234-5678/);

    // It MUST contain masked tokens
    expect(turn2ToolContent).toContain("[[PII_RRN_");
    expect(turn2ToolContent).toContain("[[PII_PHONE_");
  });

  it("(iii) ensures claude, gemini, ollama, and custom all enter the legacy agent loop", async () => {
    const providers = ["claude", "gemini", "ollama", "custom"] as const;

    for (const p of providers) {
      let loopCalled = false;
      const spy = vi.spyOn(legacyBridgeModule, "callLegacyTurn").mockImplementationOnce(async (params) => {
        loopCalled = true;
        return {
          message: { role: "assistant" },
          toolCalls: [
            {
              id: `call_${p}`,
              type: "function",
              function: {
                name: "propose_hwp_edits",
                arguments: JSON.stringify({ summary: `${p} loop success`, diffItems: [] }),
              },
            },
          ],
          content: "제안",
          reasoningTokens: 0,
        };
      });

      const res = await orchestrateAiEdit({
        ...baseOptions,
        provider: p,
        customEndpoint: p === "custom" ? "http://127.0.0.1:8000/v1" : undefined,
      });

      expect(loopCalled).toBe(true);
      expect(res.summary).toBe(`${p} loop success`);
      spy.mockRestore();
    }
  });

  it("(iv) falls back to single-shot when ToolsUnsupportedError is thrown", async () => {
    // Legacy turn throws ToolsUnsupportedError
    vi.spyOn(legacyBridgeModule, "callLegacyTurn").mockRejectedValueOnce(
      new ToolsUnsupportedError("This model does not support tool calls")
    );

    // Mock single-shot OpenAi fallback
    vi.spyOn(openaiProviderModule, "callOpenAiCompatibleApi").mockResolvedValueOnce({
      summary: "Single-shot fallback proposal",
      diffItems: [],
    });

    const recordedSteps: string[] = [];
    const res = await orchestrateAiEdit({
      ...baseOptions,
      provider: "openai",
      onStep: (e) => recordedSteps.push(e.message),
    });

    expect(res.summary).toBe("Single-shot fallback proposal");
    expect(recordedSteps.some((s) => s.includes("단일 샷") || s.includes("폴백"))).toBe(true);
  });
});
