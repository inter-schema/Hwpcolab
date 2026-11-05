import React, { useState, useRef } from "react";
import { useDocStore } from "../../store/useDocStore";
import { DiffReviewSummaryBar } from "./DiffReviewSummaryBar";
import { FileDropZone } from "./FileDropZone";
import { orchestrateAiEdit } from "../../services/aiOrchestrator";
import { buildOfficialDocStandardOps } from "../../core/officialDocRules";
import { runtimeInvoke } from "../../core/runtimeInvoke";
import {
  Send,
  Loader2,
  Shield,
  SlidersHorizontal,
  Bot,
  FileCheck2,
  ChevronDown,
  ChevronUp,
  Plus,
  Zap,
  GitCompare,
  Square,
  Brain,
} from "lucide-react";

interface AgentStepLog {
  docChecks: number;
  pagesRead: number;
  editsCount: number;
  steps: string[];
}

interface ChatMessage {
  id: string;
  role: "user" | "assistant";
  introText?: string;
  text: string;
  agentLog?: AgentStepLog;
}

export const AiChatSidebar: React.FC = () => {
  const {
    sidebarOpen,
    sidebarDock,
    sidebarWidth,
    aiApplyMode,
    provider,
    apiKey,
    model,
    customEndpoint,
    autoMaskPii,
    docMarkdown,
    pageCount,
    tablesMeta,
    selection,
    excludedRegions,
    attachments,
    reasoningEffort,
    setReasoningEffort,
    stagingPath,
    isHwpx,
    fileBytes,
    sessionMasker,
    addProposedDiffs,
    setAutoMaskPii,
    setAiApplyMode,
    setSettingsModalOpen,
    syncHopToStaging,
  } = useDocStore();

  const [inputMessage, setInputMessage] = useState("");
  const [loading, setLoading] = useState(false);
  const [liveSteps, setLiveSteps] = useState<string[]>([]);
  const currentRequestIdRef = useRef<string | null>(null);
  const isCancelledRef = useRef<boolean>(false);
  const abortControllerRef = useRef<AbortController | null>(null);
  const [expandedLogs, setExpandedLogs] = useState<Record<string, boolean>>({});
  const [messages, setMessages] = useState<ChatMessage[]>([
    {
      id: "welcome",
      role: "assistant",
      introText: "안녕하세요! HOP-Claw 로컬 AI 한글(HWP/HWPX) 협업 어시스턴트입니다.",
      text: "첨부 문서(DOCX, XLSX, PDF, HWP)의 내용을 바탕으로 현재 양식의 표와 본문을 채우거나, 공문서 개조식 변환 및 서식 교정을 요청해보세요.",
    },
  ]);

  if (!sidebarOpen) return null;

  const toggleLogExpand = (msgId: string) => {
    setExpandedLogs((prev) => ({ ...prev, [msgId]: !prev[msgId] }));
  };

  const handleCancel = async () => {
    isCancelledRef.current = true;
    if (abortControllerRef.current) {
      abortControllerRef.current.abort();
    }
    const reqId = currentRequestIdRef.current;
    if (reqId) {
      try {
        await runtimeInvoke("cancel_llm_request", { requestId: reqId });
      } catch (err) {
        console.warn("Cancel request failed", err);
      }
    }
    setLiveSteps((prev) => [...prev, "🛑 사용자에 의해 작업이 중단되었습니다."]);
    setLoading(false);
  };

  const handleSend = async (customPrompt?: string) => {
    const textToSend = customPrompt || inputMessage.trim();
    if (!textToSend || loading) return;

    const userMsg: ChatMessage = {
      id: crypto.randomUUID(),
      role: "user",
      text: textToSend,
    };
    setMessages((prev) => [...prev, userMsg]);
    setInputMessage("");
    setLoading(true);
    setLiveSteps([]);
    isCancelledRef.current = false;
    const requestId = crypto.randomUUID();
    currentRequestIdRef.current = requestId;
    const abortController = new AbortController();
    abortControllerRef.current = abortController;

    try {
      // First sync any direct human typing/formatting in HOP to the staging file
      await syncHopToStaging();
      let latestState = useDocStore.getState();

      if (!latestState.stagingPath) {
        throw new Error(
          "AI가 읽을 수 있는 문서가 연결되어 있지 않습니다. 파일 > 열기로 문서를 다시 열어 주세요."
        );
      }
      if (!latestState.docMarkdown.trim() && latestState.tablesMeta.length === 0) {
        await latestState.refreshDocMarkdown();
        latestState = useDocStore.getState();
      }
      if (!latestState.docMarkdown.trim() && latestState.tablesMeta.length === 0) {
        throw new Error(
          "문서 본문/표를 추출하지 못했습니다(claw-hwp 엔진 실행 실패 가능성). Node.js 설치 및 resources/claw-hwp 폴더 위치를 확인해 주세요."
        );
      }

      const attachmentsContext = latestState.attachments
        .map((a) => `[첨부파일: ${a.fileName}]\n${a.markdownSummary}`)
        .join("\n\n");

      // Build chatHistory excluding welcome message
      const chatHistory = messages
        .filter((m) => m.id !== "welcome")
        .map((m) => ({
          role: m.role,
          content: m.text,
        }));

      const onStep = (event: any) => {
        const stepText = typeof event === "string" ? event : (event?.message || event?.step || "");
        if (stepText) {
          setLiveSteps((prev) => [...prev, stepText]);
        }
      };

      const res: any = await orchestrateAiEdit({
        provider: latestState.provider,
        apiKey: latestState.apiKey,
        model: latestState.model,
        customEndpoint: latestState.customEndpoint,
        loopMode: latestState.agentLoopMode,
        customSupportsTools: latestState.customSupportsTools,
        customSendReasoning: latestState.customSendReasoning,
        customName: latestState.customName,
        docMarkdown: latestState.docMarkdown,
        tablesMeta: latestState.tablesMeta,
        selection: latestState.selection,
        excludedRegions: latestState.excludedRegions,
        autoMaskPii: latestState.autoMaskPii,
        userPrompt: textToSend,
        attachmentsContext,
        // Agentic Track 3 Options
        reasoningEffort: latestState.reasoningEffort,
        stagingPath: latestState.stagingPath,
        isHwpx: latestState.isHwpx,
        fileBytes: latestState.fileBytes,
        chatHistory,
        sessionMasker: latestState.sessionMasker,
        signal: abortController.signal,
        requestId,
        onStep,
      } as any);

      if (isCancelledRef.current) {
        return;
      }

      const diffItems = res.diffItems || [];
      const opsCount = diffItems.reduce((acc: number, item: any) => acc + (item.unifiedOps?.length || 0), 0);
      const pagesReadCount = Math.max(1, latestState.pageCount);

      // 실제로 AI 컨텍스트에 포함된 읽기 단계만 기록합니다.
      const readSteps: string[] = [
        `문서 본문 읽기 (${latestState.docMarkdown.length.toLocaleString()}자)`,
        ...(latestState.tablesMeta.length > 0
          ? [`표 구조 읽기 (표 ${latestState.tablesMeta.length}개, 셀 ${latestState.tablesMeta.reduce((n, t) => n + t.cells.length, 0)}개)`]
          : []),
        ...latestState.attachments.map((a) => `첨부 문서 읽기: ${a.fileName}`),
        ...(latestState.selection ? ["선택 영역 읽기"] : []),
      ];
      const docChecksCount = typeof res.docChecks === "number" ? res.docChecks : readSteps.length;

      const stepDetails: string[] = [
        ...readSteps,
        ...diffItems.map((d: any) => `[${d.title}] → "${(d.afterText || "").slice(0, 45)}${(d.afterText || "").length > 45 ? "..." : ""}"`),
      ];

      const actualSteps =
        res.steps && Array.isArray(res.steps) && res.steps.length > 0
          ? res.steps
          : liveSteps.length > 0
          ? liveSteps
          : stepDetails;

      // Remove fake intro text when no edits are proposed
      let introText: string | undefined = undefined;
      if (res.introText) {
        introText = res.introText;
      } else if (diffItems.length > 0) {
        introText =
          latestState.attachments.length > 0
            ? "첨부 문서와 현재 양식을 비교해, 양식에 맞는 내용만 골라 채워 넣었습니다."
            : "문서 구조와 레이아웃을 분석하여 편집 작업을 제안합니다.";
      }

      const assistantMsg: ChatMessage = {
        id: crypto.randomUUID(),
        role: "assistant",
        introText,
        agentLog: {
          docChecks: docChecksCount,
          pagesRead: pagesReadCount,
          editsCount: opsCount,
          steps: actualSteps,
        },
        text:
          res.summary ||
          (diffItems.length > 0
            ? `${diffItems.length}개 항목(총 ${opsCount}건 편집 연산)을 생성했습니다.`
            : "제안된 수정 사항이 없습니다."),
      };
      setMessages((prev) => [...prev, assistantMsg]);

      if (diffItems.length > 0) {
        await addProposedDiffs(diffItems);
      }
    } catch (err: any) {
      const isAbort =
        isCancelledRef.current ||
        err?.name === "AbortError" ||
        abortController.signal.aborted ||
        (typeof err?.message === "string" && err.message.toLowerCase().includes("aborted"));
      if (isAbort) {
        if (!isCancelledRef.current) {
          isCancelledRef.current = true;
          setLiveSteps((prev) => [...prev, "🛑 사용자에 의해 작업이 중단되었습니다."]);
        }
        return;
      }
      const errorMsg: ChatMessage = {
        id: crypto.randomUUID(),
        role: "assistant",
        text: `오류가 발생했습니다: ${err?.message || err}`,
      };
      setMessages((prev) => [...prev, errorMsg]);
    } finally {
      abortControllerRef.current = null;
      setLoading(false);
    }
  };

  const handleApplyOfficialStandard = async () => {
    await syncHopToStaging();
    const currentMd = useDocStore.getState().docMarkdown;
    if (!currentMd) {
      alert("문서 내용이 비어있습니다.");
      return;
    }
    const lines = currentMd.split("\n").filter((l) => l.trim().length > 0 && !l.startsWith("|"));
    const ops = buildOfficialDocStandardOps(lines);

    await addProposedDiffs([
      {
        id: crypto.randomUUID(),
        status: "pending",
        scope: "document",
        title: "행정안전부 공문서 표준 서식 일괄 교정",
        beforeText: "기존 글꼴 및 들여쓰기 서식",
        afterText: "함초롬바탕 11pt, 줄간격 160%, 개조식 단계별(1.->가.->1)) 들여쓰기",
        unifiedOps: ops,
      },
    ]);
  };

  return (
    <aside
      id="ai-chat-sidebar"
      style={{ width: `${sidebarWidth}px` }}
      className={`h-full bg-white flex flex-col z-20 select-none shrink-0 min-w-[300px] max-w-[72vw] ${
        sidebarDock === "left" ? "border-r border-slate-200" : "border-l border-slate-200"
      }`}
    >
      {/* Top Compact Controls Bar */}
      <div className="flex flex-col gap-1.5 px-3 py-2 border-b border-slate-100 bg-slate-50/70">
        {/* Row 1 (Model & Reasoning Level — always 100% visible) */}
        <div className="flex items-center justify-between gap-2 min-w-0">
          <button
            id="sidebar-model-settings-btn"
            onClick={() => setSettingsModalOpen(true)}
            className="flex-1 min-w-0 flex items-center justify-between gap-1 text-[11px] font-semibold text-slate-700 bg-white hover:bg-slate-100 border border-slate-200 px-2 py-1 rounded-lg shadow-2xs transition-colors"
            title="AI 모델 및 API 설정"
          >
            <div className="flex items-center gap-1 min-w-0">
              <Bot className="w-3.5 h-3.5 text-teal-700 shrink-0" />
              <span className="shrink-0">{provider === "baze" ? "BAZE" : provider === "custom" ? "사용자 지정" : provider.toUpperCase()}</span>
              <span className="text-[10px] text-slate-400 font-mono truncate">({model})</span>
            </div>
            <SlidersHorizontal className="w-2.5 h-2.5 text-slate-400 shrink-0" />
          </button>

          {/* Quick Reasoning Effort Selector */}
          <div className="shrink-0 flex items-center gap-1 bg-indigo-50/70 border border-indigo-200 rounded-lg px-2 py-1">
            <Brain className="w-3 h-3 text-indigo-600 shrink-0" />
            <span className="text-[10px] font-semibold text-indigo-900 shrink-0">추론</span>
            <select
              id="sidebar-reasoning-effort-select"
              data-testid="reasoning-effort-select"
              value={reasoningEffort}
              onChange={(e) => setReasoningEffort(e.target.value as any)}
              className="text-[10px] font-semibold text-slate-700 bg-white border border-slate-200 px-1 py-0.5 rounded shadow-2xs hover:bg-slate-50 focus:outline-none cursor-pointer shrink-0"
              title="추론 수준 (Reasoning Effort)"
            >
              <option value="none">없음 (None)</option>
              <option value="low">낮음 (Low)</option>
              <option value="medium">중간 (Medium)</option>
              <option value="high">높음 (High)</option>
              <option value="xhigh">최대 (Max)</option>
            </select>
          </div>
        </div>

        {/* Row 2 (Apply Mode & Privacy Masking) */}
        <div className="flex items-center justify-between gap-2">
          {/* Diff Review vs Immediate Apply Mode Switch */}
          <div className="flex items-center rounded-lg border border-slate-200 p-0.5 bg-slate-100/80">
            <button
              id="sidebar-mode-diff-btn"
              data-testid="mode-diff-review-btn"
              onClick={() => setAiApplyMode("diff_review")}
              className={`flex items-center gap-1 text-[10px] px-2 py-0.5 rounded-md font-semibold transition-colors ${
                aiApplyMode === "diff_review"
                  ? "bg-teal-50 text-teal-800 border border-teal-200 shadow-2xs"
                  : "text-slate-500 hover:text-slate-800"
              }`}
              title="Smart Diff 개별 검토 모드"
            >
              <GitCompare className="w-3 h-3" />
              <span>Diff 검토</span>
            </button>
            <button
              id="sidebar-mode-immediate-btn"
              data-testid="mode-immediate-btn"
              onClick={() => setAiApplyMode("immediate")}
              className={`flex items-center gap-1 text-[10px] px-2 py-0.5 rounded-md font-semibold transition-colors ${
                aiApplyMode === "immediate"
                  ? "bg-amber-50 text-amber-800 border border-amber-200 shadow-2xs"
                  : "text-slate-500 hover:text-slate-800"
              }`}
              title="즉시 문서 반영 모드"
            >
              <Zap className="w-3 h-3" />
              <span>즉시 반영</span>
            </button>
          </div>

          <button
            id="sidebar-pii-mask-toggle"
            onClick={() => setAutoMaskPii(!autoMaskPii)}
            className={`flex items-center gap-1 text-[10px] px-2 py-1 rounded-lg border font-semibold transition-colors ${
              autoMaskPii
                ? "bg-emerald-50 text-emerald-700 border-emerald-300"
                : "bg-white text-slate-600 border-slate-200 hover:bg-slate-50"
            }`}
            title="주민번호, 전화번호, 이메일 자동 가명화 토글"
          >
            <Shield className="w-3 h-3" />
            <span>가명화 {autoMaskPii ? "ON" : "OFF"}</span>
          </button>
        </div>
      </div>

      {/* Quick Macro Bar */}
      <div className="px-3 py-1.5 bg-slate-50/40 border-b border-slate-100 flex items-center gap-1.5">
        <button
          id="official-doc-macro-btn"
          onClick={handleApplyOfficialStandard}
          className="flex-1 flex items-center justify-center gap-1 text-[11px] font-semibold text-slate-700 bg-white hover:bg-slate-50 border border-slate-200 py-1 rounded-lg shadow-2xs transition-colors"
          title="행정업무운영 편람 표준 규격(함초롬바탕, 줄간격 160%, 1.->가.->1) 들여쓰기) 일괄 교정"
        >
          <FileCheck2 className="w-3.5 h-3.5 text-teal-700" />
          <span>🏛️ 공문서 표준 규격 일괄 교정</span>
        </button>
      </div>

      {/* Center: Attachments + Chat Stream + Diff Summary */}
      <div id="ai-chat-scroll-area" className="flex-1 overflow-y-auto px-4 py-3 flex flex-col gap-4">
        {/* Attached Files Cards (Screenshot Style) */}
        <FileDropZone />

        {/* Pending Diffs Review Card */}
        <DiffReviewSummaryBar />

        {/* Message Stream */}
        <div className="flex flex-col gap-4">
          {messages.map((msg) => {
            if (msg.role === "user") {
              return (
                <div key={msg.id} className="flex justify-end">
                  <div
                    data-testid="user-chat-bubble"
                    className="bg-[#0F4C5C] text-white px-4 py-3 rounded-3xl max-w-[92%] text-[13px] font-medium leading-relaxed whitespace-pre-wrap select-text shadow-xs"
                  >
                    {msg.text}
                  </div>
                </div>
              );
            }

            const isExpanded = Boolean(expandedLogs[msg.id]);

            return (
              <div key={msg.id} className="flex flex-col gap-2.5 text-[13px] text-slate-800 leading-relaxed">
                {msg.introText && (
                  <p className="text-slate-700 leading-relaxed select-text">{msg.introText}</p>
                )}

                {msg.agentLog && (
                  <div className="my-0.5">
                    <button
                      type="button"
                      data-testid="agent-exec-log-summary"
                      onClick={() => toggleLogExpand(msg.id)}
                      className="flex items-center gap-1 text-xs font-medium text-slate-500 hover:text-slate-800 transition-colors py-0.5"
                    >
                      <span>
                        문서 확인 {msg.agentLog.docChecks}회, 페이지 {msg.agentLog.pagesRead}개 읽음, 문서 편집{" "}
                        {msg.agentLog.editsCount}건
                      </span>
                      {isExpanded ? (
                        <ChevronUp className="w-3.5 h-3.5 text-slate-400" />
                      ) : (
                        <ChevronDown className="w-3.5 h-3.5 text-slate-400" />
                      )}
                    </button>

                    {isExpanded && (
                      <div className="mt-1.5 pl-2.5 border-l-2 border-slate-200 flex flex-col gap-1 text-[11px] text-slate-500">
                        {msg.agentLog.steps.map((step, sIdx) => (
                          <div key={sIdx} className="truncate">
                            • {step}
                          </div>
                        ))}
                      </div>
                    )}
                  </div>
                )}

                <p data-testid="assistant-chat-response" className="text-slate-800 leading-relaxed whitespace-pre-wrap select-text">
                  {msg.text}
                </p>
              </div>
            );
          })}

          {loading && (
            <div
              id="ai-loading-indicator"
              data-testid="ai-loading-indicator"
              className="flex flex-col gap-2 text-xs bg-slate-900 text-slate-200 p-3 rounded-xl border border-slate-700 shadow-md animate-in fade-in"
            >
              <div className="flex items-center justify-between border-b border-slate-800 pb-2">
                <div className="flex items-center gap-2">
                  <Loader2 className="w-4 h-4 animate-spin text-teal-400 shrink-0" />
                  <span className="font-semibold text-white text-[12px]">
                    에이전트 추론 및 문서 검증 중...
                  </span>
                </div>
                <button
                  type="button"
                  data-testid="cancel-llm-request-btn"
                  onClick={handleCancel}
                  className="flex items-center gap-1 text-[11px] bg-rose-500/20 hover:bg-rose-500/30 text-rose-300 border border-rose-500/40 px-2 py-0.5 rounded-md transition-colors cursor-pointer"
                  title="요청 중단"
                >
                  <Square className="w-2.5 h-2.5 fill-current" />
                  <span>중단</span>
                </button>
              </div>

              {/* Claude-CLI Style Live Steps Timeline */}
              {liveSteps.length > 0 ? (
                <div className="flex flex-col gap-1 font-mono text-[11px] max-h-40 overflow-y-auto pr-1">
                  {liveSteps.map((step, idx) => {
                    const isLatest = idx === liveSteps.length - 1;
                    return (
                      <div
                        key={idx}
                        className={`flex items-start gap-1.5 transition-opacity ${
                          isLatest ? "text-teal-300 font-semibold" : "text-slate-400"
                        }`}
                      >
                        <span className="shrink-0">{isLatest ? "▶" : "✓"}</span>
                        <span className="break-all">{step}</span>
                      </div>
                    );
                  })}
                </div>
              ) : (
                <div className="text-[11px] text-slate-400 font-mono">
                  🔍 문서 구조 및 레이아웃을 계측·분석하고 있습니다...
                </div>
              )}
            </div>
          )}
        </div>
      </div>

      {/* Bottom Input Area (Screenshot Style: Rounded Box with + button & input) */}
      <div className="p-3 bg-white">
        <form
          onSubmit={(e) => {
            e.preventDefault();
            handleSend();
          }}
          className="border border-slate-200 rounded-2xl p-2.5 bg-white shadow-xs focus-within:border-slate-400 transition-colors flex flex-col gap-1.5"
        >
          <div className="flex items-start gap-2">
            <button
              type="button"
              onClick={() => {
                const el = document.getElementById("sidebar-attachment-file-input") as HTMLInputElement | null;
                el?.click();
              }}
              className="p-1 text-slate-400 hover:text-slate-700 rounded-lg hover:bg-slate-100 mt-0.5 shrink-0"
              title="참고 문서 첨부 (+)"
            >
              <Plus className="w-4 h-4" />
            </button>

            <textarea
              id="ai-chat-input"
              data-testid="ai-chat-input"
              value={inputMessage}
              onChange={(e) => setInputMessage(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && !e.shiftKey) {
                  e.preventDefault();
                  handleSend();
                }
              }}
              placeholder="메시지를 입력하세요."
              rows={2}
              disabled={loading}
              className="flex-1 text-xs py-1 pr-2 focus:outline-none bg-transparent text-slate-800 placeholder:text-slate-400 resize-none leading-relaxed"
            />

            <button
              id="ai-chat-send-btn"
              data-testid="ai-chat-send-btn"
              type="submit"
              disabled={loading || !inputMessage.trim()}
              className="p-1.5 bg-[#0F4C5C] hover:bg-[#0b3946] disabled:opacity-30 text-white rounded-xl transition-colors shrink-0 self-end"
              title="메시지 전송"
            >
              {loading ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : <Send className="w-3.5 h-3.5" />}
            </button>
          </div>
        </form>
      </div>
    </aside>
  );
};
