import React, { useEffect, useState } from "react";
import { useDocStore, DiffItem } from "../../store/useDocStore";
import { hopBridge } from "../../core/hopBridge";
import { diff_match_patch, DIFF_DELETE, DIFF_INSERT } from "diff-match-patch";
import { Check, X, AlertTriangle, CheckCircle2 } from "lucide-react";

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

export const SmartDiffOverlay: React.FC = () => {
  const { pendingDiffs, acceptDiffItem, rejectDiffItem, zoom } = useDocStore();
  const [, setScrollTick] = useState(0);

  useEffect(() => {
    const scrollContainer = document.getElementById("scroll-container");
    if (!scrollContainer) return;
    const onScroll = () => setScrollTick((t) => t + 1);
    scrollContainer.addEventListener("scroll", onScroll, { passive: true });
    window.addEventListener("resize", onScroll);
    return () => {
      scrollContainer.removeEventListener("scroll", onScroll);
      window.removeEventListener("resize", onScroll);
    };
  }, [pendingDiffs.length, zoom]);

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

  // Prevent cards from overlapping vertically on the canvas
  const usedPositions: Array<{ x: number; y: number }> = [];

  return (
    <div
      id="smart-diff-canvas-overlay"
      className="absolute inset-0 pointer-events-none z-30 overflow-hidden"
    >
      {pendingDiffs.slice(0, 8).map((item, idx) => {
        let pos: { x: number; y: number } | null = null;
        if (item.bbox) {
          pos = hopBridge.pageBoxToContainerCoords(item.bbox.pageIndex, item.bbox.x, item.bbox.y);
        }
        let left = pos ? Math.max(80, Math.min(pos.x, 560)) : 120 + (idx % 2) * 280;
        let top = pos ? Math.max(165, pos.y) : 175 + idx * 82;

        // Avoid vertical overlap with earlier cards
        for (const used of usedPositions) {
          if (Math.abs(used.x - left) < 220 && Math.abs(used.y - top) < 68) {
            top = used.y + 72;
          }
        }
        usedPositions.push({ x: left, y: top });

        const diffs = dmp.diff_main(item.beforeText || "", item.afterText || "");
        dmp.diff_cleanupSemantic(diffs);
        const metricChips = getMetricChips(item.metricsDelta);
        const isWarning =
          (item.warnings && item.warnings.length > 0) ||
          item.verificationStatus === "warning_unresolved" ||
          item.verificationStatus === "warning_new_issues";

        return (
          <div
            key={`diff-${item.id}`}
            data-testid="smart-diff-card"
            className={`absolute pointer-events-auto bg-white/98 border-2 rounded-lg shadow-xl p-2 flex flex-col gap-1 min-w-[210px] max-w-[340px] transition-all ${
              isWarning ? "border-amber-500" : "border-emerald-500"
            }`}
            style={{
              left,
              top,
              zIndex: 30 + idx,
            }}
          >
            {/* Header: Title & Individual Accept/Reject */}
            <div className="flex items-center justify-between gap-2 border-b border-slate-100 pb-1 text-[11px]">
              <span className="font-bold text-slate-800 truncate" title={item.title}>
                {item.title}
              </span>
              <div className="flex items-center gap-1 shrink-0">
                <button
                  data-testid="diff-accept-btn"
                  onClick={() => handleAcceptItem(item)}
                  className={`flex items-center gap-0.5 text-white font-bold px-2 py-0.5 rounded text-[10px] shadow-xs transition-colors ${
                    isWarning
                      ? "bg-amber-600 hover:bg-amber-700"
                      : "bg-emerald-600 hover:bg-emerald-700"
                  }`}
                  title={isWarning ? "경고 확인 후 적용" : "이 변경 적용 (✔)"}
                >
                  <Check className="w-3 h-3" />
                  <span>적용</span>
                </button>
                <button
                  data-testid="diff-reject-btn"
                  onClick={() => rejectDiffItem(item.id)}
                  className="flex items-center gap-0.5 bg-rose-50 hover:bg-rose-100 text-rose-700 font-semibold px-2 py-0.5 rounded text-[10px] border border-rose-200 transition-colors"
                  title="되돌리기 (✖)"
                >
                  <X className="w-3 h-3" />
                  <span>취소</span>
                </button>
              </div>
            </div>

            {/* Verification Status & Layout Metrics Chips */}
            <div className="flex flex-wrap items-center gap-1">
              {item.verificationStatus === "verified" && (
                <span className="inline-flex items-center gap-0.5 text-[9px] font-semibold text-emerald-700 bg-emerald-100 px-1 py-0.2 rounded">
                  <CheckCircle2 className="w-2.5 h-2.5" />
                  <span>✓ 레이아웃 검증 통과</span>
                </span>
              )}
              {(item.verificationStatus === "warning_unresolved" ||
                item.verificationStatus === "warning_new_issues") && (
                <span className="inline-flex items-center gap-0.5 text-[9px] font-semibold text-amber-800 bg-amber-100 px-1 py-0.2 rounded">
                  <AlertTriangle className="w-2.5 h-2.5" />
                  <span>⚠️ 레이아웃 경고</span>
                </span>
              )}
              {metricChips.map((chip, cIdx) => (
                <span
                  key={cIdx}
                  className="inline-block bg-slate-100 text-slate-700 font-mono text-[9px] px-1 py-0.2 rounded border border-slate-200"
                >
                  {chip}
                </span>
              ))}
            </div>

            {/* Warnings Box */}
            {item.warnings && item.warnings.length > 0 && (
              <div className="bg-amber-50 border border-amber-200 text-amber-800 text-[10px] p-1 rounded flex flex-col gap-0.5">
                {item.warnings.map((w, wIdx) => (
                  <div key={wIdx} className="flex items-start gap-1">
                    <span className="shrink-0">⚠️</span>
                    <span className="leading-tight">{w}</span>
                  </div>
                ))}
              </div>
            )}

            {/* Visual Text Diff */}
            <div className="text-xs font-sans leading-relaxed max-h-24 overflow-y-auto whitespace-pre-wrap select-text">
              {diffs.map(([op, text], dIdx) => {
                if (op === DIFF_DELETE) {
                  return (
                    <span
                      key={dIdx}
                      className="bg-rose-100 text-rose-800 line-through px-0.5 rounded-xs mr-0.5"
                    >
                      {text}
                    </span>
                  );
                }
                if (op === DIFF_INSERT) {
                  return (
                    <span
                      key={dIdx}
                      className="bg-emerald-100 text-emerald-800 font-semibold px-0.5 rounded-xs"
                    >
                      {text}
                    </span>
                  );
                }
                return (
                  <span key={dIdx} className="text-slate-600">
                    {text}
                  </span>
                );
              })}
            </div>
          </div>
        );
      })}
    </div>
  );
};
