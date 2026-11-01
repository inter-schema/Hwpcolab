import React from "react";
import { useDocStore, DiffItem } from "../../store/useDocStore";
import { Check, X, CheckCheck, Trash2, AlertTriangle, CheckCircle2 } from "lucide-react";
import { diff_match_patch, DIFF_DELETE, DIFF_INSERT } from "diff-match-patch";

const dmp = new diff_match_patch();

function getMetricChips(delta: any): string[] {
  if (!delta) return [];
  if (Array.isArray(delta)) {
    return delta.map((d) => d.deltaDesc || `${d.property || "크기"}: ${d.beforeValue} → ${d.afterValue}`);
  }
  const chips: string[] = [];
  if (delta.widthBeforeMm !== undefined && delta.widthAfterMm !== undefined) {
    chips.push(`너비 ${delta.widthBeforeMm}mm → ${delta.widthAfterMm}mm`);
  }
  if (delta.heightBeforeMm !== undefined && delta.heightAfterMm !== undefined) {
    chips.push(`높이 ${delta.heightBeforeMm}mm → ${delta.heightAfterMm}mm`);
  }
  if (delta.deltaDesc && chips.length === 0) {
    chips.push(delta.deltaDesc);
  }
  return chips;
}

export const DiffReviewSummaryBar: React.FC = () => {
  const { pendingDiffs, acceptAllDiffs, rejectAllDiffs, acceptDiffItem, rejectDiffItem } = useDocStore();

  if (pendingDiffs.length === 0) return null;

  const handleAcceptItem = (item: DiffItem) => {
    const hasWarning =
      (item.warnings && item.warnings.length > 0) ||
      item.verificationStatus === "warning_unresolved" ||
      item.verificationStatus === "warning_new_issues";

    if (hasWarning) {
      const ok = window.confirm(
        "이 수정안은 레이아웃 경고(잘림/오버플로 미해소 가능성)가 있습니다. 그래도 적용하시겠습니까?"
      );
      if (!ok) return;
    }
    acceptDiffItem(item.id);
  };

  const handleAcceptAll = () => {
    const hasWarnings = pendingDiffs.some(
      (d) =>
        (d.warnings && d.warnings.length > 0) ||
        d.verificationStatus === "warning_unresolved" ||
        d.verificationStatus === "warning_new_issues"
    );
    if (hasWarnings) {
      const ok = window.confirm(
        "제안 중 레이아웃 경고(잘림/오버플로 미해소 가능성)가 포함된 항목이 있습니다. 그래도 모두 적용하시겠습니까?"
      );
      if (!ok) return;
    }
    acceptAllDiffs();
  };

  return (
    <div className="flex flex-col gap-2 p-3 bg-emerald-50/60 border border-emerald-200 rounded-xl shadow-xs">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-1.5 text-xs font-bold text-emerald-900">
          <span>제안된 변경사항</span>
          <span className="bg-emerald-200 text-emerald-800 px-1.5 py-0.2 rounded-full text-[10px]">
            {pendingDiffs.length}건
          </span>
        </div>
        <div className="flex items-center gap-1">
          <button
            onClick={handleAcceptAll}
            className="flex items-center gap-1 text-[11px] bg-emerald-600 hover:bg-emerald-700 text-white font-semibold px-2 py-1 rounded-md shadow-xs transition-colors"
            title="모두 적용 (Ctrl+Enter)"
          >
            <CheckCheck className="w-3 h-3" />
            <span>모두 적용</span>
          </button>
          <button
            onClick={rejectAllDiffs}
            className="flex items-center gap-1 text-[11px] bg-white hover:bg-rose-50 text-rose-700 border border-rose-200 font-semibold px-2 py-1 rounded-md transition-colors"
            title="모두 취소"
          >
            <Trash2 className="w-3 h-3" />
            <span>취소</span>
          </button>
        </div>
      </div>

      {/* Pending Diff Cards List */}
      <div className="flex flex-col gap-1.5 max-h-64 overflow-y-auto pr-0.5">
        {pendingDiffs.map((item) => {
          const diffs = dmp.diff_main(item.beforeText || "", item.afterText || "");
          dmp.diff_cleanupSemantic(diffs);
          const metricChips = getMetricChips(item.metricsDelta);
          const isWarning =
            (item.warnings && item.warnings.length > 0) ||
            item.verificationStatus === "warning_unresolved" ||
            item.verificationStatus === "warning_new_issues";

          return (
            <div
              key={item.id}
              className={`bg-white border rounded-lg p-2 text-xs shadow-2xs flex flex-col gap-1.5 transition-colors ${
                isWarning ? "border-amber-300 bg-amber-50/20" : "border-emerald-200/80"
              }`}
            >
              <div className="flex items-center justify-between gap-1">
                <span className="font-semibold text-slate-800 truncate" title={item.title}>
                  {item.title}
                </span>
                <div className="flex items-center gap-1 shrink-0">
                  <button
                    onClick={() => handleAcceptItem(item)}
                    className="p-1 hover:bg-emerald-100 text-emerald-700 rounded transition-colors"
                    title="적용"
                  >
                    <Check className="w-3.5 h-3.5" />
                  </button>
                  <button
                    onClick={() => rejectDiffItem(item.id)}
                    className="p-1 hover:bg-rose-100 text-rose-700 rounded transition-colors"
                    title="되돌리기"
                  >
                    <X className="w-3.5 h-3.5" />
                  </button>
                </div>
              </div>

              {/* Status & Metrics Badges */}
              <div className="flex flex-wrap items-center gap-1">
                {item.verificationStatus === "verified" && (
                  <span className="inline-flex items-center gap-0.5 text-[10px] font-semibold text-emerald-700 bg-emerald-100 px-1.5 py-0.5 rounded">
                    <CheckCircle2 className="w-3 h-3" />
                    <span>✓ 레이아웃 검증 통과</span>
                  </span>
                )}
                {(item.verificationStatus === "warning_unresolved" ||
                  item.verificationStatus === "warning_new_issues") && (
                  <span className="inline-flex items-center gap-0.5 text-[10px] font-semibold text-amber-800 bg-amber-100 px-1.5 py-0.5 rounded">
                    <AlertTriangle className="w-3 h-3" />
                    <span>⚠️ 레이아웃 경고</span>
                  </span>
                )}
                {metricChips.map((chip, cIdx) => (
                  <span
                    key={cIdx}
                    className="inline-block bg-slate-100 text-slate-700 font-mono text-[10px] px-1.5 py-0.5 rounded border border-slate-200"
                  >
                    {chip}
                  </span>
                ))}
              </div>

              {/* Warnings Box */}
              {item.warnings && item.warnings.length > 0 && (
                <div className="bg-amber-50 border border-amber-200 text-amber-800 text-[10px] p-1.5 rounded flex flex-col gap-0.5">
                  {item.warnings.map((w, wIdx) => (
                    <div key={wIdx} className="flex items-start gap-1">
                      <span className="shrink-0">⚠️</span>
                      <span className="leading-tight">{w}</span>
                    </div>
                  ))}
                </div>
              )}

              {/* Text Diff View */}
              <div className="text-[11px] max-h-20 overflow-y-auto leading-relaxed text-slate-600 whitespace-pre-wrap select-text bg-slate-50 p-1.5 rounded">
                {diffs.map(([op, text], idx) => {
                  if (op === DIFF_DELETE) {
                    return (
                      <span key={idx} className="bg-rose-100 text-rose-800 line-through px-0.5">
                        {text}
                      </span>
                    );
                  }
                  if (op === DIFF_INSERT) {
                    return (
                      <span key={idx} className="bg-emerald-100 text-emerald-800 font-medium px-0.5">
                        {text}
                      </span>
                    );
                  }
                  return <span key={idx}>{text}</span>;
                })}
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
};
