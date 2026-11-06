import React, { useState } from "react";
import { useDocStore } from "../../store/useDocStore";
import { orchestrateAiEdit, INLINE_QUICK_ACTION_PROMPTS } from "../../services/aiOrchestrator";
import {
  Sparkles,
  FileCheck,
  ListOrdered,
  Calculator,
  Lock,
  ArrowRight,
  Loader2,
  X,
} from "lucide-react";

interface Props {
  position: { x: number; y: number } | null;
  onClose?: () => void;
}

export const InlineQuickToolbar: React.FC<Props> = ({ position, onClose }) => {
  const {
    selection,
    excludedRegions,
    addProposedDiffs,
    toggleExcludeSelection,
    syncHopToStaging,
    setSelection,
  } = useDocStore();

  const [customPrompt, setCustomPrompt] = useState("");
  const [showNestedModal, setShowNestedModal] = useState(false);
  const [nestedRows, setNestedRows] = useState(2);
  const [nestedCols, setNestedCols] = useState(2);
  const [loading, setLoading] = useState(false);

  if (!selection || !position) return null;

  const isTableSelection = selection.kind === "table_cells";

  const handleAction = async (actionKey: string, customText?: string) => {
    if (loading) return;
    setLoading(true);

    try {
      await syncHopToStaging();
      const latest = useDocStore.getState();
      const prompt = customText || INLINE_QUICK_ACTION_PROMPTS[actionKey] || "";
      const attachmentsContext = latest.attachments
        .map((a) => `[${a.fileName}]:\n${a.markdownSummary}`)
        .join("\n\n");

      const res = await orchestrateAiEdit({
        provider: latest.provider,
        apiKey: latest.apiKey,
        model: latest.model,
        customEndpoint: latest.customEndpoint,
        docMarkdown: latest.docMarkdown,
        tablesMeta: latest.tablesMeta,
        selection: latest.selection,
        excludedRegions: latest.excludedRegions,
        autoMaskPii: latest.autoMaskPii,
        userPrompt: prompt,
        attachmentsContext,
      });

      if (res.diffItems.length > 0) {
        await addProposedDiffs(res.diffItems);
      } else {
        alert(res.summary || "제안된 수정 사항이 없습니다.");
      }
    } catch (err: any) {
      alert(`AI 요청 오류: ${err?.message || err}`);
    } finally {
      setLoading(false);
      setCustomPrompt("");
    }
  };

  const isCurrentSelectionExcluded = () => {
    if (selection.kind === "table_cells") {
      return selection.selectedCells.some((c) =>
        excludedRegions.some(
          (r) =>
            r.targetType === "cell" &&
            r.tableIndex === selection.tableIndex &&
            r.row === c.row &&
            r.col === c.col
        )
      );
    }
    return excludedRegions.some(
      (r) => r.targetType === "body_text" && r.rawText === selection.selectedText
    );
  };

  return (
    <div
      id="inline-quick-toolbar"
      data-testid="inline-quick-toolbar"
      onMouseDown={(e) => e.stopPropagation()}
      className="absolute z-40 bg-white/98 rounded-xl shadow-2xl border border-slate-200 p-2 flex flex-col gap-2 min-w-[350px] transform -translate-x-1/2 -translate-y-full transition-all pointer-events-auto"
      style={{
        left: Math.max(250, Math.min(position.x, 680)),
        top: Math.max(220, position.y),
      }}
    >
      {/* Quick Action Buttons */}
      <div className="flex items-center gap-1 flex-wrap justify-between">
        <div className="flex items-center gap-1 flex-wrap">
          <button
            id="quick-btn-official-tone"
            onClick={() => handleAction("official_tone")}
            disabled={loading}
            className="flex items-center gap-1 text-[11px] px-2 py-1 rounded-md bg-slate-50 hover:bg-teal-50 text-slate-700 hover:text-teal-800 font-semibold border border-slate-200 transition-colors"
            title="대한민국 행정기관 공문서 표준 어투(~함, ~기 바람)로 변환"
          >
            <Sparkles className="w-3 h-3 text-teal-700" />
            <span>공문서 어투</span>
          </button>

          <button
            id="quick-btn-proofread"
            onClick={() => handleAction("proofread")}
            disabled={loading}
            className="flex items-center gap-1 text-[11px] px-2 py-1 rounded-md bg-slate-50 hover:bg-emerald-50 text-slate-700 hover:text-emerald-700 font-semibold border border-slate-200 transition-colors"
            title="맞춤법 및 비문 다듬기"
          >
            <FileCheck className="w-3 h-3 text-emerald-600" />
            <span>맞춤법 교정</span>
          </button>

          <button
            id="quick-btn-summarize"
            onClick={() => handleAction("summarize_bullet")}
            disabled={loading}
            className="flex items-center gap-1 text-[11px] px-2 py-1 rounded-md bg-slate-50 hover:bg-amber-50 text-slate-700 hover:text-amber-700 font-semibold border border-slate-200 transition-colors"
            title="핵심 요약 및 개조식 축약"
          >
            <ListOrdered className="w-3 h-3 text-amber-600" />
            <span>개조식 축약</span>
          </button>

          {isTableSelection && (
            <>
            <button
              id="quick-btn-fill-table"
              onClick={() => handleAction("fill_or_calc_table")}
              disabled={loading}
              className="flex items-center gap-1 text-[11px] px-2 py-1 rounded-md bg-teal-50 hover:bg-teal-100 text-teal-800 font-semibold border border-teal-200 transition-colors shadow-2xs"
              title="주변 문맥 및 첨부파일 기반 표 채우기 또는 수치 계산"
            >
              <Calculator className="w-3 h-3 text-teal-700" />
              <span>표 채우기/계산</span>
            </button>

            <button
              id="quick-btn-nested-table"
              data-testid="quick-btn-nested-table"
              onClick={() => {
                if (!useDocStore.getState().isHwpx) {
                  alert("중첩 표는 HWPX 형식에서 지원됩니다. [새 문서 .hwpx]나 HWPX 파일에서 사용해주세요.");
                  return;
                }
                setShowNestedModal(true);
              }}
              disabled={loading}
              className="flex items-center gap-1 text-[11px] px-2 py-1 rounded-md bg-teal-50 hover:bg-teal-100 text-teal-800 font-semibold border border-teal-200 transition-colors shadow-2xs"
              title="선택된 표 셀 내부에 중첩 표 삽입 (HWPX 전용)"
            >
              <span className="font-mono text-xs">⊞</span>
              <span>표 안에 표</span>
            </button>
            </>
          )}

          <button
            id="quick-btn-exclude-ai"
            data-testid="inline-exclude-ai-checkbox"
            onClick={toggleExcludeSelection}
            className={`flex items-center gap-1 text-[11px] px-2 py-1 rounded-md font-semibold border transition-colors ${
              isCurrentSelectionExcluded()
                ? "bg-slate-800 text-white border-slate-900"
                : "bg-slate-50 hover:bg-slate-100 text-slate-700 border-slate-200"
            }`}
            title="이 영역을 AI API 전송에서 제외하고 잠금"
          >
            <Lock className="w-3 h-3" />
            <span>{isCurrentSelectionExcluded() ? "전송 제외됨" : "AI 전송 제외"}</span>
          </button>
        </div>

        <button
          onClick={() => {
            setSelection(null);
            onClose?.();
          }}
          className="p-0.5 text-slate-400 hover:text-slate-700 rounded"
          title="닫기"
        >
          <X className="w-3.5 h-3.5" />
        </button>
      </div>

      {/* Bottom: Custom Single-Line Prompt Input */}
      <form
        onSubmit={(e) => {
          e.preventDefault();
          if (customPrompt.trim()) {
            handleAction("custom", customPrompt.trim());
          }
        }}
        className="flex items-center gap-1.5 border-t border-slate-100 pt-1.5"
      >
        <input
          id="inline-quick-prompt-input"
          type="text"
          value={customPrompt}
          onChange={(e) => setCustomPrompt(e.target.value)}
          placeholder="선택 영역에 대해 한 줄 지시 (예: 공문서 어투로 변환해줘, 빈칸 채워줘)..."
          disabled={loading}
          className="flex-1 text-xs px-2.5 py-1.5 rounded-lg border border-slate-200 bg-slate-50 focus:bg-white focus:outline-none focus:ring-1 focus:ring-teal-600 text-slate-800"
        />
        <button
          id="inline-quick-prompt-submit"
          type="submit"
          disabled={loading || !customPrompt.trim()}
          className="p-1.5 bg-[#0F4C5C] hover:bg-[#0b3946] disabled:opacity-40 text-white rounded-lg transition-colors shadow-2xs"
        >
          {loading ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : <ArrowRight className="w-3.5 h-3.5" />}
        </button>
      </form>

      {/* Nested Table Modal */}
      {showNestedModal && selection.kind === "table_cells" && (
        <div
          id="nested-table-modal"
          data-testid="nested-table-modal"
          className="absolute inset-0 bg-white/98 rounded-xl p-2.5 flex flex-col justify-between z-50 shadow-2xl border border-slate-200"
        >
          <div className="flex items-center justify-between pb-1.5 border-b border-slate-100">
            <span className="text-xs font-bold text-slate-800 flex items-center gap-1">
              <span className="text-teal-700 font-mono text-xs">⊞</span> 표 안에 표 (중첩 표) 삽입
            </span>
            <button
              type="button"
              onClick={() => setShowNestedModal(false)}
              className="text-slate-400 hover:text-slate-600 p-0.5 rounded"
            >
              <X className="w-3.5 h-3.5" />
            </button>
          </div>
          <div className="flex items-center gap-3 py-2 text-xs text-slate-700">
            <label className="flex items-center gap-1.5">
              <span className="font-medium text-slate-600">줄 수:</span>
              <input
                id="nested-table-rows"
                data-testid="nested-table-rows"
                type="number"
                min={1}
                max={8}
                value={nestedRows}
                onChange={(e) => setNestedRows(Math.max(1, Math.min(8, parseInt(e.target.value, 10) || 1)))}
                className="w-14 px-2 py-1 border border-slate-200 rounded text-center font-medium bg-slate-50 focus:bg-white focus:outline-none focus:ring-1 focus:ring-teal-600"
              />
            </label>
            <label className="flex items-center gap-1.5">
              <span className="font-medium text-slate-600">칸 수:</span>
              <input
                id="nested-table-cols"
                data-testid="nested-table-cols"
                type="number"
                min={1}
                max={8}
                value={nestedCols}
                onChange={(e) => setNestedCols(Math.max(1, Math.min(8, parseInt(e.target.value, 10) || 1)))}
                className="w-14 px-2 py-1 border border-slate-200 rounded text-center font-medium bg-slate-50 focus:bg-white focus:outline-none focus:ring-1 focus:ring-teal-600"
              />
            </label>
            <span className="text-[10px] text-slate-400">(1~8줄, 1~8칸)</span>
          </div>
          <div className="flex items-center justify-end gap-1.5 pt-1.5 border-t border-slate-100">
            <button
              type="button"
              onClick={() => setShowNestedModal(false)}
              className="px-2.5 py-1 text-xs text-slate-600 hover:bg-slate-100 rounded transition-colors"
            >
              취소
            </button>
            <button
              id="nested-table-confirm"
              data-testid="nested-table-confirm"
              type="button"
              disabled={loading}
              onClick={async () => {
                if (selection.kind !== "table_cells") return;
                try {
                  setLoading(true);
                  await useDocStore.getState().applyHumanNestedTableInsert(
                    selection.tableIndex,
                    selection.startRow,
                    selection.startCol,
                    nestedRows,
                    nestedCols
                  );
                  setShowNestedModal(false);
                  setSelection(null);
                  onClose?.();
                } catch (err: any) {
                  alert(`중첩 표 생성 실패: ${err?.message || err}`);
                } finally {
                  setLoading(false);
                }
              }}
              className="px-3 py-1 text-xs font-semibold text-white bg-teal-700 hover:bg-teal-800 rounded transition-colors shadow-2xs flex items-center gap-1"
            >
              {loading && <Loader2 className="w-3 h-3 animate-spin" />}
              <span>삽입</span>
            </button>
          </div>
        </div>
      )}
    </div>
  );
};
