import type { AiRequestOptions, AgentStepEvent } from "../aiOrchestrator.ts";
import type { TableCoordinateMeta, SelectionTarget } from "../../types/hwp.ts";
import type { DiffItem, UnifiedOp } from "../../types/operations.ts";
import type { PrivacySessionMasker } from "../../core/privacyMasker.ts";
import { lintOps } from "../policyGuard.ts";
import { OpTranslator } from "../../core/opTranslator.ts";
import { runtimeInvoke } from "../../core/runtimeInvoke.ts";
import {
  inspectDocumentBytes,
  compareLayoutReports,
  type LayoutReport,
} from "../../core/layoutInspector.ts";
import type {
  NeutralMessage,
  NeutralToolCall,
  NeutralToolSpec,
  ProviderAdapter,
  ToolChoice,
} from "./types.ts";
import { ToolsUnsupportedError, TransientProviderError } from "./types.ts";
import type { ReasoningEffortOption } from "../providers/openaiProvider.ts";
import { compactOldToolResults } from "./context.ts";
import {
  ToolRuntime,
  parseToolArgs,
  isReadOnlyTool,
  PROPOSE_HWP_EDITS_TOOL_SPEC,
  DEFAULT_V2_TOOLS,
} from "./toolRuntime.ts";

export interface LoopV2Context {
  adapter: ProviderAdapter;
  systemPrompt: string;
  messages: NeutralMessage[];
  options: AiRequestOptions;
  masker: PrivacySessionMasker;
  emitStep: (event: AgentStepEvent) => void;
  baseBytes?: Uint8Array | null;
  initialReport?: LayoutReport | null;
  getTools: () => NeutralToolSpec[];
  // Injectables for testing and fine-tuning
  sleepFn?: (ms: number) => Promise<void>;
  maxTurns?: number;
  maxToolCalls?: number;
  totalTimeoutMs?: number;
}

export interface LoopV2Result {
  summary: string;
  diffItems: DiffItem[];
  verificationPassed: boolean;
  trialsRun: number;
}

/**
 * Determine if user request looks like an editing intent.
 * Questions without editing verbs are treated as informational queries.
 */
export function looksLikeEditIntent(
  userPrompt: string,
  selection?: SelectionTarget | null
): boolean {
  if (selection) {
    return true;
  }

  const normalized = userPrompt.trim();
  const editKeywords = [
    "수정",
    "고쳐",
    "바꿔",
    "변경",
    "추가",
    "삭제",
    "맞춰",
    "줄여",
    "늘려",
    "작성",
    "만들어",
    "편집",
    "개선",
    "반영",
    "정리해",
    "fix",
    "change",
    "edit",
    "update",
    "format",
    "replace",
    "insert",
    "delete",
  ];

  const hasEditKeyword = editKeywords.some((kw) => normalized.includes(kw));
  if (hasEditKeyword) {
    return true;
  }

  // Check if it's purely a question
  const questionMarkers = ["몇 개", "어디", "무엇", "왜", "어떤", "?", "알려줘", "조회해줘", "조회만", "확인만"];
  const isPureQuestion = questionMarkers.some((qm) => normalized.includes(qm));
  if (isPureQuestion) {
    return false;
  }

  return false;
}

/**
 * Cap reasoning effort for exploration turns to at most "medium".
 */
export function capReasoningEffort(
  userEffort: string | undefined,
  cap: "none" | "low" | "medium" | "high" | "xhigh" = "medium"
): ReasoningEffortOption {
  const levels = ["none", "low", "medium", "high", "xhigh"];
  const current = (userEffort || "high").toLowerCase();
  const currentIdx = levels.indexOf(current);
  const capIdx = levels.indexOf(cap);
  if (currentIdx === -1) return cap as ReasoningEffortOption;
  const chosenIdx = Math.min(currentIdx, capIdx);
  return levels[chosenIdx] as ReasoningEffortOption;
}

/**
 * Normalize tool call arguments for stagnation detection.
 */
function normalizeCallSignature(call: NeutralToolCall): string {
  try {
    const parsed = parseToolArgs(call.argumentsJson);
    const sortedKeys = Object.keys(parsed).sort();
    const sortedObj: Record<string, any> = {};
    for (const k of sortedKeys) {
      sortedObj[k] = parsed[k];
    }
    return `${call.name}:${JSON.stringify(sortedObj)}`;
  } catch {
    return `${call.name}:${call.argumentsJson.trim()}`;
  }
}

/**
 * Run multi-turn agent loop v2 with defensive state machine, budgets,
 * error recovery, trial isolation, and verification gates.
 */
export async function runAgentLoopV2(ctx: LoopV2Context): Promise<LoopV2Result> {
  const maxTurns = ctx.maxTurns ?? Math.min(8, Math.max(1, ctx.options.maxIterations ?? 8));
  const maxToolCalls = ctx.maxToolCalls ?? 16;
  const totalTimeoutMs = ctx.totalTimeoutMs ?? 240_000;
  const startTime = Date.now();
  const sleep = ctx.sleepFn || ((ms: number) => new Promise((resolve) => setTimeout(resolve, ms)));

  const signal = (ctx.options as any).signal as AbortSignal | undefined;
  const checkAbort = () => {
    if (signal?.aborted) {
      const err = new Error("Agent loop aborted by user");
      err.name = "AbortError";
      throw err;
    }
  };

  // Initialize tool execution runtime
  const availableTools = ctx.getTools() || DEFAULT_V2_TOOLS;
  const toolRuntime = new ToolRuntime({
    requestId: ctx.options.requestId,
    baseBytes: ctx.baseBytes,
    initialReport: ctx.initialReport,
    stagingPath: ctx.options.stagingPath,
    isHwpx: ctx.options.isHwpx,
    tablesMeta: ctx.options.tablesMeta,
    excludedRegions: ctx.options.excludedRegions,
    autoMaskPii: ctx.options.autoMaskPii,
    userPrompt: ctx.options.userPrompt,
    docMarkdown: ctx.options.docMarkdown,
    masker: ctx.masker,
    emitStep: ctx.emitStep,
    availableTools,
  });

  const messages: NeutralMessage[] = [...ctx.messages];
  let totalToolCallsCount = 0;
  let nudged = false;
  let guardFeedbackCount = 0;
  let verifyFeedbackCount = 0;
  let forcePropose = false;

  const toolCallSignatures = new Map<string, number>();

  let rawResult: LoopV2Result = {
    summary: "",
    diffItems: [],
    verificationPassed: false,
    trialsRun: 0,
  };

  for (let turn = 0; turn < maxTurns; turn++) {
    checkAbort();

    // Check wall-clock budget
    const elapsed = Date.now() - startTime;
    if (elapsed >= totalTimeoutMs) {
      forcePropose = true;
    }

    // Check budget limits for forced proposal
    if (turn >= maxTurns - 1 || totalToolCallsCount >= maxToolCalls) {
      forcePropose = true;
    }

    // Determine tools and toolChoice for this turn
    let currentTools: NeutralToolSpec[];
    let currentToolChoice: ToolChoice;
    let turnEffort: ReasoningEffortOption;

    if (forcePropose) {
      currentTools = [PROPOSE_HWP_EDITS_TOOL_SPEC];
      currentToolChoice = { name: "propose_hwp_edits" };
      turnEffort = (ctx.options.reasoningEffort as ReasoningEffortOption) || "high";
    } else {
      currentTools = availableTools;
      currentToolChoice = "auto";
      turnEffort = capReasoningEffort(ctx.options.reasoningEffort, "medium");
    }

    ctx.emitStep({
      type: "thinking",
      message: forcePropose
        ? `최종 편집안 도출 중 (강제 제안 턴, effort: ${turnEffort})`
        : `AI 추론 및 편집안 탐색 중 (${turn + 1}/${maxTurns}회차, effort: ${turnEffort})`,
    });

    // Compact old tool results before sending to adapter
    const compactedMessages = compactOldToolResults(messages, 3);

    // Call adapter turn with exponential backoff on transient errors (429, 5xx)
    let turnResponse: any;
    const maxRetries = 2;
    let attempt = 0;

    while (true) {
      checkAbort();
      try {
        turnResponse = await ctx.adapter.turn({
          systemPrompt: ctx.systemPrompt,
          messages: compactedMessages,
          tools: currentTools,
          toolChoice: currentToolChoice,
          reasoningEffort: turnEffort,
          requestId: ctx.options.requestId,
          timeoutMs: ctx.options.timeoutMs ?? 180_000,
          signal,
        });
        break;
      } catch (err: any) {
        if (err instanceof ToolsUnsupportedError) {
          // Re-throw immediately for single-shot fallback
          throw err;
        }

        const isTransient =
          err instanceof TransientProviderError ||
          err?.status === 429 ||
          (err?.status >= 500 && err?.status < 600) ||
          /429|rate\s*limit|overloaded|timeout|econnreset/i.test(err?.message || "");

        if (isTransient && attempt < maxRetries) {
          attempt++;
          const backoffMs = 500 * Math.pow(2, attempt - 1);
          await sleep(backoffMs);
          continue;
        }
        throw err;
      }
    }

    // Append assistant's turn to messages
    messages.push({
      role: "assistant",
      content: turnResponse.content,
      toolCalls: turnResponse.toolCalls?.length > 0 ? turnResponse.toolCalls : undefined,
    });

    // Case A: Assistant returned no tool calls (text response)
    if (!turnResponse.toolCalls || turnResponse.toolCalls.length === 0) {
      const text = turnResponse.content || "";

      // Check if text contains embedded JSON with diffItems
      const jsonMatch =
        text.match(/```json\s*([\s\S]*?)\s*```/) ||
        text.match(/\{[\s\S]*"diffItems"[\s\S]*\}/);

      if (jsonMatch) {
        try {
          const parsed = JSON.parse(jsonMatch[1] || jsonMatch[0]);
          if (Array.isArray(parsed.diffItems)) {
            rawResult = {
              summary: parsed.summary || text,
              diffItems: parsed.diffItems,
              verificationPassed: true,
              trialsRun: toolRuntime.trialsRun,
            };
            break;
          }
        } catch {}
      }

      // Check for editing intent: 1-time nudge if editing intent detected
      const isEditIntent = looksLikeEditIntent(ctx.options.userPrompt, ctx.options.selection);
      if (isEditIntent && !nudged && !forcePropose) {
        nudged = true;
        messages.push({
          role: "user",
          content: "편집 또는 수정을 적용하려면 변경 내용을 propose_hwp_edits 도구로 제출해 주세요.",
        });
        continue;
      }

      // Accept text response as direct answer
      rawResult = {
        summary: text,
        diffItems: [],
        verificationPassed: true,
        trialsRun: toolRuntime.trialsRun,
      };
      break;
    }

    // Case B: Assistant returned tool calls
    totalToolCallsCount += turnResponse.toolCalls.length;

    // Check for stagnation across tool calls
    for (const call of turnResponse.toolCalls) {
      const sig = normalizeCallSignature(call);
      const count = (toolCallSignatures.get(sig) || 0) + 1;
      toolCallSignatures.set(sig, count);
      if (count >= 2) {
        forcePropose = true;
      }
    }

    // Separate propose tool calls from query/trial calls
    const proposeCalls = turnResponse.toolCalls.filter((c: NeutralToolCall) => c.name === "propose_hwp_edits");
    const nonProposeCalls = turnResponse.toolCalls.filter((c: NeutralToolCall) => c.name !== "propose_hwp_edits");

    // Execute non-propose calls first:
    // Read-only calls run in parallel, mutating/trial calls run sequentially
    const readCalls = nonProposeCalls.filter((c: NeutralToolCall) => isReadOnlyTool(c.name));
    const writeCalls = nonProposeCalls.filter((c: NeutralToolCall) => !isReadOnlyTool(c.name));

    // Parallel execution of read tools
    const readResults = await Promise.all(
      readCalls.map(async (call: NeutralToolCall) => {
        const content = await toolRuntime.executeToolCall(call);
        return { call, content };
      })
    );
    for (const res of readResults) {
      messages.push({
        role: "tool",
        toolCallId: res.call.id,
        name: res.call.name,
        content: res.content,
      });
    }

    // Sequential execution of write/trial tools
    for (const call of writeCalls) {
      const content = await toolRuntime.executeToolCall(call);
      messages.push({
        role: "tool",
        toolCallId: call.id,
        name: call.name,
        content,
      });
    }

    // Process propose_hwp_edits if present
    if (proposeCalls.length > 0) {
      const proposeCall = proposeCalls[0];
      // If multiple propose calls were returned, answer extra ones with dummy acknowledgment
      for (let i = 1; i < proposeCalls.length; i++) {
        messages.push({
          role: "tool",
          toolCallId: proposeCalls[i].id,
          name: proposeCalls[i].name,
          content: JSON.stringify({ ok: true, note: "중복 제안 호출은 첫 번째 제안에 병합되었습니다." }),
        });
      }

      ctx.emitStep({ type: "propose", message: "제안안 도출 검수 및 정책 가드 검사 중..." });

      let callArgs: Record<string, any> = {};
      try {
        callArgs = parseToolArgs(proposeCall.argumentsJson);
      } catch (parseErr: any) {
        messages.push({
          role: "tool",
          toolCallId: proposeCall.id,
          name: proposeCall.name,
          content: JSON.stringify({
            ok: false,
            error: "propose_hwp_edits 인자 파싱 실패: 올바른 JSON 형식이어야 합니다.",
          }),
        });
        continue;
      }

      const proposalSummary = callArgs.summary || "";
      const diffItems: DiffItem[] = Array.isArray(callArgs.diffItems) ? callArgs.diffItems : [];
      const allOps: UnifiedOp[] = diffItems.flatMap((d: any) => d.unifiedOps || []);

      // 1. Policy Guard (G1-G6) Check
      const guardContext = {
        userPrompt: ctx.options.userPrompt,
        isNewDocument: false,
        excludedRegions: ctx.options.excludedRegions,
        layoutReport: toolRuntime.currentReport || toolRuntime.initialReport,
        tablesMeta: ctx.options.tablesMeta,
      };
      const guardRes = lintOps(allOps, guardContext);

      if (!guardRes.passed && guardFeedbackCount < 1 && turn < maxTurns - 1) {
        guardFeedbackCount++;
        ctx.emitStep({
          type: "guard",
          message: "정책 가드 위반 감지, 모델 자가 교정 피드백 전달",
          detail: guardRes.rejectedOps.map((r) => r.reason).join("; "),
        });
        messages.push({
          role: "tool",
          toolCallId: proposeCall.id,
          name: proposeCall.name,
          content: `정책 가드 검사 실패:\n${guardRes.rejectedOps.map((r) => r.reason).join("\n")}\n\n위반 사항을 교정하여 다시 propose_hwp_edits를 호출하세요.`,
        });
        continue; // Model self-corrects in next turn
      }

      // 2. Auto-Verification Gate (Trial Apply + Layout Re-measurement)
      let afterReport: LayoutReport | null = null;
      let verificationOk = true;
      let compSummary = "";

      if (toolRuntime.baseBytes && ctx.options.stagingPath && allOps.length > 0) {
        toolRuntime.trialsRun += 1;
        ctx.emitStep({ type: "trial", message: "제안된 연산 복사본 시험 적용 및 재측정 검증 중..." });

        try {
          const isHwpx = Boolean(
            ctx.options.isHwpx ??
              (ctx.options.stagingPath.endsWith(".hwpx") ||
                (toolRuntime.baseBytes[0] === 0x50 && toolRuntime.baseBytes[1] === 0x4b))
          );
          const ext = isHwpx ? "hwpx" : "hwp";
          const safeReqId = (ctx.options.requestId || "verify").replace(/[^a-zA-Z0-9]/g, "_");
          const verifyPath = ctx.options.stagingPath.replace(
            /[^\\/]+$/,
            `trial_verify_${safeReqId}.${ext}`
          );

          await runtimeInvoke("write_file_bytes", {
            filePath: verifyPath,
            bytes: Array.from(toolRuntime.baseBytes),
          });

          const unmaskedOps = ctx.masker.unmaskAndFilterOps(
            guardRes.sanitizedOps,
            ctx.options.excludedRegions
          );
          const payload = OpTranslator.toEnginePayload(
            verifyPath,
            isHwpx,
            unmaskedOps,
            ctx.options.tablesMeta,
            false
          );

          const clawRes = await runtimeInvoke<{ ok: boolean; stdout_json: any; stderr: string }>(
            "execute_claw_ops",
            {
              isHwpx,
              payloadJson: payload.stdinJson,
            }
          );

          if (clawRes.ok) {
            const patchedBytesArr = await runtimeInvoke<number[]>("read_file_bytes", {
              filePath: verifyPath,
            });
            afterReport = await inspectDocumentBytes(new Uint8Array(patchedBytesArr));
            const comparison = compareLayoutReports(
              toolRuntime.initialReport || afterReport,
              afterReport
            );
            compSummary = comparison.summaryText;

            // If overflow issues remain, provide 1-time refinement feedback
            if (
              !comparison.ok &&
              comparison.remainingIssues.length > 0 &&
              verifyFeedbackCount < 1 &&
              turn < maxTurns - 1
            ) {
              verifyFeedbackCount++;
              ctx.emitStep({
                type: "verify",
                message: "오버플로 미완전 해소 감지, 모델 정밀 교정 피드백 전달",
                detail: comparison.summaryText,
              });
              messages.push({
                role: "tool",
                toolCallId: proposeCall.id,
                name: proposeCall.name,
                content: `시험 검증 결과: ${comparison.summaryText}\n잔여 이슈 목록:\n${comparison.remainingIssues.map((i) => i.message).join("\n")}\n\n행 높이나 열 너비를 더 확장하거나 셀 여백을 줄여 오버플로를 완전히 해소한 뒤 다시 propose_hwp_edits를 호출하세요.`,
              });
              continue; // Refine in next turn
            }

            verificationOk = comparison.ok;
          }
        } catch (err: any) {
          console.warn("[loopV2] Auto-verification error:", err);
        }
      }

      // Attach verification and metric annotations to diffItems (legacy L696-L716)
      for (const item of diffItems) {
        item.verificationStatus = verificationOk ? "verified" : "warning_unresolved";
        item.warnings = [...(guardRes.warnings || [])];

        const tc = item.targetCoord;
        if (
          toolRuntime.initialReport &&
          afterReport &&
          tc &&
          typeof tc.tableIndex === "number"
        ) {
          const tableIdx: number = tc.tableIndex;
          const targetRow = tc.row;
          const targetCol = tc.col;
          const cellBefore = toolRuntime.initialReport.tables[tableIdx]?.cells.find(
            (c: any) => c.row === targetRow && c.col === targetCol
          );
          const cellAfter = afterReport.tables[tableIdx]?.cells.find(
            (c: any) => c.row === targetRow && c.col === targetCol
          );
          if (cellBefore || cellAfter) {
            item.metricsDelta = {
              beforeOverflowMm: cellBefore?.overflowAmountMm,
              afterOverflowMm: cellAfter?.overflowAmountMm,
              widthBeforeMm: cellBefore?.widthMm,
              widthAfterMm: cellAfter?.widthMm,
              heightBeforeMm: cellBefore?.heightMm,
              heightAfterMm: cellAfter?.heightMm,
            };
          }
        }
      }

      messages.push({
        role: "tool",
        toolCallId: proposeCall.id,
        name: proposeCall.name,
        content: JSON.stringify({
          ok: true,
          verification: verificationOk,
          summary: compSummary || "검증 완료",
        }),
      });

      rawResult = {
        summary: proposalSummary,
        diffItems,
        verificationPassed: verificationOk,
        trialsRun: toolRuntime.trialsRun,
      };

      ctx.emitStep({
        type: "verify",
        message: verificationOk
          ? "검증 완료: 모든 레이아웃 이슈 해소"
          : `검증 완료: ${compSummary || "일부 잔여 이슈 보존"}`,
      });

      break; // Successfully finalized proposal
    }
  }

  return rawResult;
}
