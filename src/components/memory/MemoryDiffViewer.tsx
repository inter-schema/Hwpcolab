import React, { useState } from "react";
import {
  FIXED_MEMORY_SECTIONS,
  type MemorySectionTitle,
  type SectionDiff,
} from "../../memory/index.ts";
import { Check, X, ChevronDown, ChevronRight, CheckCircle2 } from "lucide-react";

export interface MemoryDiffViewerProps {
  diffs: SectionDiff[];
  acceptedTitles: string[];
  onToggleSection: (title: MemorySectionTitle) => void;
  onAcceptSelected: () => void;
  onCancel: () => void;
  busy?: boolean;
}

export const MemoryDiffViewer: React.FC<MemoryDiffViewerProps> = ({
  diffs,
  acceptedTitles,
  onToggleSection,
  onAcceptSelected,
  onCancel,
  busy = false,
}) => {
  const [collapsedSections, setCollapsedSections] = useState<Record<string, boolean>>(() => {
    const initial: Record<string, boolean> = {};
    for (const d of diffs) {
      if (!d.changed) {
        initial[d.sectionTitle] = true;
      }
    }
    return initial;
  });

  const toggleCollapse = (title: string) => {
    setCollapsedSections((prev) => ({
      ...prev,
      [title]: !prev[title],
    }));
  };

  const changedCount = diffs.filter((d) => d.changed).length;
  const acceptedChangedCount = diffs.filter(
    (d) => d.changed && acceptedTitles.includes(d.sectionTitle)
  ).length;

  return (
    <div className="flex flex-col h-full bg-slate-50 border border-slate-200 rounded-xl overflow-hidden">
      <div className="px-4 py-3 bg-white border-b border-slate-200 flex items-center justify-between">
        <div>
          <h3 className="text-sm font-bold text-slate-800 flex items-center gap-2">
            <span>AI 다듬기 수정안 비교 (Smart Diff)</span>
            <span className="text-xs font-normal text-slate-500">
              (총 {diffs.length}개 섹션 중 {changedCount}개 변경됨, {acceptedChangedCount}개 선택됨)
            </span>
          </h3>
          <p className="text-xs text-slate-500 mt-0.5">
            수락할 섹션을 개별 선택한 뒤 하단 저장 버튼을 눌러주세요.
          </p>
        </div>
      </div>

      <div className="flex-1 overflow-y-auto p-4 space-y-3">
        {FIXED_MEMORY_SECTIONS.map((fixedTitle, idx) => {
          const diff = diffs.find((d) => d.sectionTitle === fixedTitle) || {
            sectionTitle: fixedTitle,
            beforeContent: "",
            afterContent: "",
            changed: false,
            tokens: [],
          };

          const isAccepted = acceptedTitles.includes(diff.sectionTitle);
          const isCollapsed = Boolean(collapsedSections[diff.sectionTitle]);

          return (
            <div
              key={diff.sectionTitle}
              className={`rounded-lg border transition-all ${
                diff.changed
                  ? isAccepted
                    ? "border-teal-300 bg-white shadow-xs"
                    : "border-slate-200 bg-slate-100/60 opacity-80"
                  : "border-slate-200 bg-slate-50"
              }`}
            >
              {/* Section Header */}
              <div className="px-3 py-2 flex items-center justify-between gap-2 border-b border-slate-100">
                <div
                  className="flex items-center gap-2 cursor-pointer select-none"
                  onClick={() => toggleCollapse(diff.sectionTitle)}
                >
                  {isCollapsed ? (
                    <ChevronRight className="w-4 h-4 text-slate-400" />
                  ) : (
                    <ChevronDown className="w-4 h-4 text-slate-400" />
                  )}
                  <span className="text-xs font-semibold text-slate-800">
                    {idx + 1}. {diff.sectionTitle}
                  </span>
                  {diff.changed ? (
                    <span className="text-[10px] font-medium px-1.5 py-0.5 rounded-full bg-teal-100 text-teal-800">
                      변경됨
                    </span>
                  ) : (
                    <span className="text-[10px] font-medium px-1.5 py-0.5 rounded-full bg-slate-200 text-slate-600">
                      변경 없음
                    </span>
                  )}
                </div>

                {/* Section Acceptance Toggle */}
                {diff.changed && (
                  <button
                    type="button"
                    data-testid={`refine-toggle-section-${idx}`}
                    onClick={() => onToggleSection(diff.sectionTitle)}
                    disabled={busy}
                    className={`flex items-center gap-1 text-xs px-2.5 py-1 rounded font-medium transition-colors ${
                      isAccepted
                        ? "bg-teal-600 text-white hover:bg-teal-700 shadow-2xs"
                        : "bg-slate-200 text-slate-700 hover:bg-slate-300"
                    }`}
                  >
                    {isAccepted ? (
                      <>
                        <Check className="w-3.5 h-3.5" />
                        <span>수락됨</span>
                      </>
                    ) : (
                      <>
                        <X className="w-3.5 h-3.5" />
                        <span>거절됨 (유지)</span>
                      </>
                    )}
                  </button>
                )}
              </div>

              {/* Section Body */}
              {!isCollapsed && (
                <div className="p-3 text-xs leading-relaxed font-mono whitespace-pre-wrap break-words">
                  {diff.changed ? (
                    diff.tokens && diff.tokens.length > 0 ? (
                      diff.tokens.map((token, tIdx) => {
                        if (token.op === -1) {
                          return (
                            <span
                              key={tIdx}
                              className="bg-rose-100 text-rose-800 line-through rounded-xs px-0.5"
                            >
                              {token.text}
                            </span>
                          );
                        }
                        if (token.op === 1) {
                          return (
                            <span
                              key={tIdx}
                              className="bg-emerald-100 text-emerald-900 font-medium rounded-xs px-0.5"
                            >
                              {token.text}
                            </span>
                          );
                        }
                        return <span key={tIdx}>{token.text}</span>;
                      })
                    ) : (
                      <div className="text-slate-400 italic">토큰 차이 정보 없음</div>
                    )
                  ) : (
                    <div className="text-slate-500 font-sans">
                      {diff.beforeContent || "- (특별한 규칙 없음)"}
                    </div>
                  )}
                </div>
              )}
            </div>
          );
        })}
      </div>

      {/* Footer Actions */}
      <div className="p-3 bg-white border-t border-slate-200 flex items-center justify-between gap-3">
        <button
          type="button"
          onClick={onCancel}
          disabled={busy}
          className="text-xs px-3 py-1.5 rounded-lg border border-slate-300 text-slate-700 hover:bg-slate-100 transition-colors disabled:opacity-50"
        >
          취소
        </button>

        <button
          type="button"
          id="memory-apply-refine-btn"
          data-testid="memory-apply-refine-btn"
          onClick={onAcceptSelected}
          disabled={busy || acceptedTitles.length === 0}
          className="flex items-center gap-1.5 text-xs font-semibold px-4 py-2 rounded-lg bg-teal-600 hover:bg-teal-700 text-white shadow-xs transition-colors disabled:opacity-50"
        >
          <CheckCircle2 className="w-4 h-4" />
          <span>선택한 섹션만 반영하여 저장</span>
        </button>
      </div>
    </div>
  );
};
