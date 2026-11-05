import React, { useState, useRef, useEffect } from "react";
import { useDocStore } from "../../store/useDocStore";
import {
  BookMarked,
  ChevronDown,
  ArrowUp,
  ArrowDown,
  X,
  Settings,
  Plus,
} from "lucide-react";

export const MemorySelectorBar: React.FC = () => {
  const {
    memoriesIndex,
    activeMemoryIds,
    toggleActiveMemory,
    moveActiveMemoryPriority,
    clearActiveMemories,
    openMemoryModal,
  } = useDocStore();

  const [popoverOpen, setPopoverOpen] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);

  // Close popover when clicking outside
  useEffect(() => {
    const handleOutsideClick = (e: MouseEvent) => {
      if (
        popoverOpen &&
        containerRef.current &&
        !containerRef.current.contains(e.target as Node)
      ) {
        setPopoverOpen(false);
      }
    };
    window.addEventListener("mousedown", handleOutsideClick);
    return () => window.removeEventListener("mousedown", handleOutsideClick);
  }, [popoverOpen]);

  const activeCount = activeMemoryIds.length;
  const labelText =
    activeCount === 0
      ? "작성 선호사항: 꺼짐"
      : `작성 선호사항: ${activeCount}개 적용 중`;

  return (
    <div ref={containerRef} className="relative w-full">
      <div className="flex items-center justify-between gap-1.5 bg-slate-50 border border-slate-200 rounded-lg px-2 py-1 text-xs">
        {/* Toggle / Dropdown trigger button */}
        <button
          type="button"
          id="sidebar-memory-selector-btn"
          data-testid="sidebar-memory-selector-btn"
          onClick={() => setPopoverOpen((prev) => !prev)}
          className={`flex-1 flex items-center gap-1.5 text-[11px] font-semibold truncate transition-colors ${
            activeCount > 0 ? "text-teal-700 font-bold" : "text-slate-600 hover:text-slate-900"
          }`}
          title="작성 선호사항 선택 및 우선순위 조정"
        >
          <BookMarked
            className={`w-3.5 h-3.5 shrink-0 ${
              activeCount > 0 ? "text-teal-600" : "text-slate-400"
            }`}
          />
          <span className="truncate">{labelText}</span>
          <ChevronDown
            className={`w-3 h-3 shrink-0 ml-auto transition-transform ${
              popoverOpen ? "rotate-180 text-teal-600" : "text-slate-400"
            }`}
          />
        </button>

        {/* Direct Open Manager Button */}
        <button
          type="button"
          id="sidebar-open-memory-manager-btn"
          onClick={() => openMemoryModal("list")}
          className="p-1 text-slate-400 hover:text-slate-700 hover:bg-slate-200/50 rounded transition-colors shrink-0"
          title="작성 선호사항 관리자 열기"
        >
          <Settings className="w-3.5 h-3.5" />
        </button>
      </div>

      {/* Popover */}
      {popoverOpen && (
        <div
          id="sidebar-memory-popover"
          className="absolute left-0 right-0 top-full mt-1 z-40 bg-white border border-slate-200 rounded-xl shadow-lg p-2.5 flex flex-col gap-2 max-h-72 overflow-y-auto animate-in fade-in slide-in-from-top-1 duration-100"
        >
          <div className="flex items-center justify-between border-b border-slate-100 pb-1.5">
            <span className="text-[11px] font-bold text-slate-700">
              선호사항 선택 및 우선순위
            </span>
            <button
              type="button"
              onClick={() => {
                setPopoverOpen(false);
                openMemoryModal("create_multi");
              }}
              className="flex items-center gap-0.5 text-[10px] font-semibold text-teal-600 hover:text-teal-800"
            >
              <Plus className="w-3 h-3" />
              <span>새로 만들기</span>
            </button>
          </div>

          {memoriesIndex.length === 0 ? (
            <div className="py-4 text-center text-xs text-slate-400">
              저장된 작성 선호사항이 없습니다.{" "}
              <button
                type="button"
                onClick={() => {
                  setPopoverOpen(false);
                  openMemoryModal("create_multi");
                }}
                className="text-teal-600 hover:underline font-semibold"
              >
                [새로 만들기]
              </button>
            </div>
          ) : (
            <div className="space-y-1.5">
              {memoriesIndex.map((meta) => {
                const priorityIdx = activeMemoryIds.indexOf(meta.id);
                const isActive = priorityIdx !== -1;

                return (
                  <div
                    key={meta.id}
                    className={`flex items-center justify-between gap-1.5 p-1.5 rounded-lg border transition-colors ${
                      isActive
                        ? "bg-teal-50/60 border-teal-200"
                        : "bg-white hover:bg-slate-50 border-slate-150"
                    }`}
                  >
                    <label className="flex items-center gap-1.5 cursor-pointer flex-1 min-w-0">
                      <input
                        type="checkbox"
                        data-testid={`memory-check-${meta.id}`}
                        checked={isActive}
                        onChange={() => toggleActiveMemory(meta.id)}
                        className="rounded text-teal-600 focus:ring-teal-500 w-3.5 h-3.5 shrink-0"
                      />
                      <span className="text-xs text-slate-800 font-medium truncate">
                        {meta.name}
                      </span>
                      <span className="text-[10px] text-slate-400 shrink-0">
                        v{meta.revision}
                      </span>
                    </label>

                    {isActive && (
                      <div className="flex items-center gap-1 shrink-0">
                        <span className="text-[10px] font-bold px-1.5 py-0.2 rounded-full bg-teal-100 text-teal-800">
                          #{priorityIdx + 1}
                        </span>
                        <button
                          type="button"
                          data-testid={`memory-up-${meta.id}`}
                          onClick={() => moveActiveMemoryPriority(meta.id, "up")}
                          disabled={priorityIdx === 0}
                          className="p-0.5 text-slate-500 hover:text-slate-800 disabled:opacity-30 disabled:cursor-not-allowed rounded hover:bg-slate-200/50"
                          title="우선순위 올리기"
                        >
                          <ArrowUp className="w-3 h-3" />
                        </button>
                        <button
                          type="button"
                          data-testid={`memory-down-${meta.id}`}
                          onClick={() => moveActiveMemoryPriority(meta.id, "down")}
                          disabled={priorityIdx === activeMemoryIds.length - 1}
                          className="p-0.5 text-slate-500 hover:text-slate-800 disabled:opacity-30 disabled:cursor-not-allowed rounded hover:bg-slate-200/50"
                          title="우선순위 내리기"
                        >
                          <ArrowDown className="w-3 h-3" />
                        </button>
                      </div>
                    )}
                  </div>
                );
              })}
            </div>
          )}

          {activeCount > 0 && (
            <div className="border-t border-slate-100 pt-1.5 flex justify-end">
              <button
                type="button"
                id="sidebar-memory-clear-btn"
                onClick={clearActiveMemories}
                className="flex items-center gap-1 text-[11px] font-semibold text-rose-600 hover:text-rose-800 py-0.5 px-1.5 rounded hover:bg-rose-50 transition-colors"
              >
                <X className="w-3 h-3" />
                <span>모두 해제</span>
              </button>
            </div>
          )}
        </div>
      )}
    </div>
  );
};
