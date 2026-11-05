import { describe, it, expect, vi, beforeEach } from "vitest";
import { orchestrateAiEdit, AiRequestOptions } from "./aiOrchestrator.ts";
import * as runtimeModule from "../core/runtimeInvoke.ts";
import type { ProviderId, AgentLoopMode } from "./agent/types.ts";
import { ToolsUnsupportedError } from "./agent/types.ts";

vi.mock("../core/runtimeInvoke.ts", () => ({
  runtimeInvoke: vi.fn(),
  isTauriEnv: () => false,
}));

describe("aiOrchestrator Multi-Provider & Loop Integration Tests", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  const SYNTHETIC_RRN = "880101-1234567";
  const SYNTHETIC_PHONE = "010-9876-5432";

  const providers: ProviderId[] = ["openai", "baze", "gemini", "ollama", "custom", "claude"];
  const loopModes: AgentLoopMode[] = ["legacy", "v2"];

  for (const provider of providers) {
    for (const loopMode of loopModes) {
      it(`[${provider} × ${loopMode}] completes [inspect_layout -> propose_hwp_edits] to DiffItem >= 1 without PII leak`, async () => {
        const capturedHttpBodies: string[] = [];
        let turnCounter = 0;

        const mockInvoke = vi.mocked(runtimeModule.runtimeInvoke);
        mockInvoke.mockImplementation(async (cmd: string, args: any) => {
          if (cmd === "call_llm_http") {
            const req = args?.req || {};
            const bodyStr = JSON.stringify(req.body_json || {});
            capturedHttpBodies.push(bodyStr);
            turnCounter++;

            const isClaude = provider === "claude";

            if (turnCounter === 1) {
              // Turn 1: Model calls inspect_layout
              if (isClaude) {
                return {
                  status: 200,
                  body: {
                    id: "msg_turn1",
                    type: "message",
                    role: "assistant",
                    content: [
                      { type: "text", text: "문서 레이아웃을 검측합니다." },
                      {
                        type: "tool_use",
                        id: "call_inspect_1",
                        name: "inspect_layout",
                        input: {},
                      },
                    ],
                    stop_reason: "tool_use",
                  },
                };
              } else {
                return {
                  status: 200,
                  body: {
                    id: "chatcmpl-turn1",
                    object: "chat.completion",
                    choices: [
                      {
                        index: 0,
                        message: {
                          role: "assistant",
                          content: "레이아웃을 검측합니다.",
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
                        finish_reason: "tool_calls",
                      },
                    ],
                  },
                };
              }
            } else {
              // Turn 2: Model proposes edits with DiffItem >= 1
              const sampleDiffItem = {
                title: "작성자 항목 수정",
                scope: "paragraph",
                beforeText: "작성자: 미상",
                afterText: "작성자: 검증담당",
                unifiedOps: [
                  {
                    type: "replace_text",
                    find: "작성자: 미상",
                    text: "작성자: 검증담당",
                  },
                ],
              };

              if (isClaude) {
                return {
                  status: 200,
                  body: {
                    id: "msg_turn2",
                    type: "message",
                    role: "assistant",
                    content: [
                      { type: "text", text: "수정안을 제안합니다." },
                      {
                        type: "tool_use",
                        id: "call_propose_2",
                        name: "propose_hwp_edits",
                        input: {
                          summary: "작성자 필드 수정 완료",
                          diffItems: [sampleDiffItem],
                        },
                      },
                    ],
                    stop_reason: "tool_use",
                  },
                };
              } else {
                return {
                  status: 200,
                  body: {
                    id: "chatcmpl-turn2",
                    object: "chat.completion",
                    choices: [
                      {
                        index: 0,
                        message: {
                          role: "assistant",
                          content: "수정안을 제안합니다.",
                          tool_calls: [
                            {
                              id: "call_propose_2",
                              type: "function",
                              function: {
                                name: "propose_hwp_edits",
                                arguments: JSON.stringify({
                                  summary: "작성자 필드 수정 완료",
                                  diffItems: [sampleDiffItem],
                                }),
                              },
                            },
                          ],
                        },
                        finish_reason: "tool_calls",
                      },
                    ],
                  },
                };
              }
            }
          }

          if (cmd === "write_file_bytes" || cmd === "cancel_llm_request") {
            return undefined;
          }
          if (cmd === "read_file_bytes") {
            return [];
          }
          if (cmd === "execute_claw_ops") {
            return { ok: true, stdout_json: {}, stderr: "" };
          }
          if (cmd === "inspect_document") {
            return { fileType: "hwpx", tables: [] };
          }

          return null;
        });

        const options: AiRequestOptions = {
          provider,
          apiKey: "sk-test-secret-key-12345",
          model: provider === "claude" ? "claude-3-7-sonnet" : "gpt-4o",
          customEndpoint: provider === "custom" ? "http://127.0.0.1:8000/v1/chat/completions" : undefined,
          loopMode,
          customSupportsTools: true,
          customSendReasoning: false,
          customName: provider === "custom" ? "로컬 테스트 서버" : undefined,
          docMarkdown: `# 공문서\n\n문서 작성자: 홍길동 (주민등록번호 ${SYNTHETIC_RRN}, 전화번호 ${SYNTHETIC_PHONE})\n내용: 서식 검토 요망`,
          tablesMeta: [],
          selection: null,
          excludedRegions: [],
          autoMaskPii: true,
          userPrompt: `문서 내 작성자 주민등록번호(${SYNTHETIC_RRN}) 및 연락처(${SYNTHETIC_PHONE})를 점검하고 수정안을 제출해주세요.`,
        };

        const res = await orchestrateAiEdit(options);

        // 1. Verify DiffItem >= 1
        expect(res.diffItems).toBeDefined();
        expect(res.diffItems.length).toBeGreaterThanOrEqual(1);
        expect(res.diffItems[0].unifiedOps.length).toBeGreaterThanOrEqual(1);

        // 2. Verify PII Protection: Synthetic RRN and Phone must NEVER appear in outbound call_llm_http bodies
        expect(capturedHttpBodies.length).toBeGreaterThanOrEqual(2);
        for (const bodyStr of capturedHttpBodies) {
          expect(bodyStr).not.toContain(SYNTHETIC_RRN);
          expect(bodyStr).not.toContain(SYNTHETIC_PHONE);
        }
      });
    }
  }

  it("handles 400 tools unsupported in legacy loop with single-shot fallback", async () => {
    let turnCount = 0;
    const mockInvoke = vi.mocked(runtimeModule.runtimeInvoke);
    mockInvoke.mockImplementation(async (cmd: string, args: any) => {
      if (cmd === "call_llm_http") {
        turnCount++;
        if (turnCount === 1) {
          // Tool-enabled call fails with 400
          return {
            status: 400,
            body: { error: { message: "This model does not support tools or function calling" } },
          };
        }
        // Fallback single-shot call succeeds
        return {
          status: 200,
          body: {
            choices: [
              {
                message: {
                  role: "assistant",
                  content: "```json\n" + JSON.stringify({
                    summary: "단일 샷으로 수정 제안 생성",
                    diffItems: [
                      {
                        title: "단일 샷 수정",
                        scope: "paragraph",
                        unifiedOps: [{ type: "replace_text", find: "이전", text: "이후" }],
                      },
                    ],
                  }) + "\n```",
                },
              },
            ],
          },
        };
      }
      return null;
    });

    const options: AiRequestOptions = {
      provider: "custom",
      apiKey: "sk-test",
      model: "custom-legacy-model",
      customEndpoint: "http://127.0.0.1:8000/v1/chat/completions",
      loopMode: "legacy",
      customSupportsTools: true,
      docMarkdown: "테스트 문서 본문",
      tablesMeta: [],
      selection: null,
      autoMaskPii: true,
      userPrompt: "문서를 수정해주세요",
    };

    const res = await orchestrateAiEdit(options);
    expect(res.diffItems.length).toBeGreaterThanOrEqual(1);
    expect(turnCount).toBe(2);
  });

  it("v2 + 400 tools-not-supported falls back to single-shot and reaches DiffItem with fallback step", async () => {
    let turnCount = 0;
    const mockInvoke = vi.mocked(runtimeModule.runtimeInvoke);
    mockInvoke.mockImplementation(async (cmd: string, args: any) => {
      if (cmd === "call_llm_http") {
        turnCount++;
        if (turnCount === 1) {
          // Tool-enabled call fails with 400
          return {
            status: 400,
            body: { error: { message: "This model does not support tools or function calling" } },
          };
        }
        // Single-shot fallback call succeeds
        return {
          status: 200,
          body: {
            choices: [
              {
                message: {
                  role: "assistant",
                  content: "```json\n" + JSON.stringify({
                    summary: "단일 샷으로 수정 제안 생성",
                    diffItems: [
                      {
                        title: "단일 샷 수정",
                        scope: "paragraph",
                        unifiedOps: [{ type: "replace_text", find: "이전", text: "이후" }],
                      },
                    ],
                  }) + "\n```",
                },
              },
            ],
          },
        };
      }
      return null;
    });

    const steps: string[] = [];
    const options: AiRequestOptions = {
      provider: "custom",
      apiKey: "sk-test",
      model: "custom-v2-model",
      customEndpoint: "http://127.0.0.1:8000/v1/chat/completions",
      loopMode: "v2",
      customSupportsTools: true,
      docMarkdown: "테스트 문서 본문",
      tablesMeta: [],
      selection: null,
      autoMaskPii: true,
      userPrompt: "문서를 수정해주세요",
      onStep: (e) => steps.push(e.message),
    };

    const res = await orchestrateAiEdit(options);
    expect(res.diffItems).toBeDefined();
    expect(res.diffItems.length).toBeGreaterThanOrEqual(1);
    expect(turnCount).toBe(2);
    expect(steps.some((s) => s.includes("단일 샷(Single-shot) 모드로 전환"))).toBe(true);
  });

  describe("AbortSignal cancellation in legacy and v2 loops", () => {
    for (const loopMode of ["legacy", "v2"] as AgentLoopMode[]) {
      it(`[${loopMode}] terminates with AbortError when signal is already aborted before execution (no LLM calls)`, async () => {
        let llmCallCount = 0;
        const mockInvoke = vi.mocked(runtimeModule.runtimeInvoke);
        mockInvoke.mockImplementation(async (cmd: string) => {
          if (cmd === "call_llm_http") {
            llmCallCount++;
            return {
              status: 200,
              body: { choices: [{ message: { role: "assistant", content: "ok" } }] },
            };
          }
          return null;
        });

        const controller = new AbortController();
        controller.abort();

        const options: AiRequestOptions = {
          provider: "openai",
          apiKey: "sk-test",
          model: "gpt-4o",
          loopMode,
          docMarkdown: "테스트 본문",
          tablesMeta: [],
          selection: null,
          autoMaskPii: true,
          userPrompt: "문서 수정",
          signal: controller.signal,
        };

        let thrownError: any = null;
        try {
          await orchestrateAiEdit(options);
        } catch (err: any) {
          thrownError = err;
        }

        expect(thrownError).toBeDefined();
        expect(thrownError?.name === "AbortError" || /abort/i.test(thrownError?.message || "")).toBe(true);
        expect(llmCallCount).toBe(0);
      });

      it(`[${loopMode}] terminates with AbortError on mid-turn cancellation and prevents additional LLM calls`, async () => {
        let llmCallCount = 0;
        const controller = new AbortController();

        const mockInvoke = vi.mocked(runtimeModule.runtimeInvoke);
        mockInvoke.mockImplementation(async (cmd: string) => {
          if (cmd === "call_llm_http") {
            llmCallCount++;
            if (llmCallCount === 1) {
              // Turn 1 returns inspect_layout, and we abort during execution
              controller.abort();
              return {
                status: 200,
                body: {
                  choices: [
                    {
                      index: 0,
                      message: {
                        role: "assistant",
                        content: "레이아웃 점검",
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
                      finish_reason: "tool_calls",
                    },
                  ],
                },
              };
            }
            // Subsequent calls should NOT happen
            return {
              status: 200,
              body: {
                choices: [
                  {
                    message: {
                      role: "assistant",
                      content: "should not be called",
                    },
                  },
                ],
              },
            };
          }
          return null;
        });

        const options: AiRequestOptions = {
          provider: "openai",
          apiKey: "sk-test",
          model: "gpt-4o",
          loopMode,
          docMarkdown: "테스트 본문",
          tablesMeta: [],
          selection: null,
          autoMaskPii: true,
          userPrompt: "문서 수정",
          signal: controller.signal,
        };

        let thrownError: any = null;
        try {
          await orchestrateAiEdit(options);
        } catch (err: any) {
          thrownError = err;
        }

        expect(thrownError).toBeDefined();
        expect(thrownError?.name === "AbortError" || /abort/i.test(thrownError?.message || "")).toBe(true);
        expect(llmCallCount).toBe(1);
      });
    }
  });
});
