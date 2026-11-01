import React, { useState } from "react";
import { useDocStore } from "../../store/useDocStore";
import { runtimeInvoke as invoke, isTauriEnv } from "../../core/runtimeInvoke";
import {
  FolderOpen,
  Save,
  Undo2,
  Settings,
  Sidebar,
  CheckCheck,
  ChevronLeft,
  Clock,
  MoreHorizontal,
  Scale,
  ArrowLeftRight,
  FilePlus2,
} from "lucide-react";

export const HeaderToolbar: React.FC = () => {
  const {
    fileName,
    isDirty,
    undoHistoryBytes,
    pendingDiffs,
    sidebarOpen,
    sidebarDock,
    openDocument,
    openDocumentBytes,
    createBlankDocument,
    saveToOriginalFile,
    undoLastChange,
    acceptAllDiffs,
    toggleSidebar,
    toggleSidebarDock,
    setSettingsModalOpen,
    setLicenseModalOpen,
  } = useDocStore();

  const [sampleMenuOpen, setSampleMenuOpen] = useState(false);

  const handleOpen = async () => {
    try {
      if (isTauriEnv()) {
        const selected = await invoke<string | null>("pick_local_file", {
          extensions: ["hwp", "hwpx"],
        });
        if (selected) {
          await openDocument(selected);
        }
      } else {
        // Browser fallback: trigger hidden file input
        const fileInput = document.getElementById("header-hwp-file-input") as HTMLInputElement | null;
        fileInput?.click();
      }
    } catch (e: any) {
      alert(`파일 열기 오류: ${e?.message || e}`);
    }
  };

  const handleSave = async () => {
    try {
      await saveToOriginalFile();
    } catch (e: any) {
      alert(`저장 실패: ${e?.message || e}`);
    }
  };

  return (
    <header
      id="top-studio-header"
      className="h-11 bg-white border-b border-slate-200 px-3 flex items-center justify-between z-30 select-none shrink-0"
    >
      {/* Hidden browser file input for non-Tauri open */}
      <input
        id="header-hwp-file-input"
        type="file"
        accept=".hwp,.hwpx"
        className="hidden"
        onChange={async (e) => {
          const f = e.target.files?.[0];
          if (!f) return;
          const bytes = new Uint8Array(await f.arrayBuffer());
          await openDocumentBytes(bytes, f.name);
          e.target.value = "";
        }}
      />

      {/* Left: < HWP 편집 [문서명] ... 🕒 히스토리 + 파일 조작 */}
      <div className="flex items-center gap-2 min-w-0">
        <button
          onClick={toggleSidebar}
          className="p-1 text-slate-600 hover:bg-slate-100 rounded transition-colors"
          title="AI 협업 패널 열기/접기"
        >
          <ChevronLeft className="w-4 h-4" />
        </button>

        <span className="font-extrabold text-slate-900 text-sm tracking-tight whitespace-nowrap">
          HWP 편집
        </span>

        <span
          id="header-doc-filename"
          data-testid="header-doc-name"
          className="text-xs text-slate-600 font-medium max-w-[260px] truncate"
          title={fileName}
        >
          {fileName.replace(/\.(hwp|hwpx)$/i, "")}
        </span>

        {/* Sample / Quick Switcher Menu */}
        <div className="relative">
          <button
            id="header-more-doc-btn"
            onClick={() => setSampleMenuOpen((v) => !v)}
            className="p-1 text-slate-400 hover:text-slate-700 hover:bg-slate-100 rounded"
            title="샘플 문서 전환 및 새 문서 메뉴"
          >
            <MoreHorizontal className="w-4 h-4" />
          </button>

          {sampleMenuOpen && (
            <div
              onMouseLeave={() => setSampleMenuOpen(false)}
              className="absolute left-0 top-full mt-1 bg-white border border-slate-200 rounded-lg shadow-xl py-1.5 min-w-[240px] z-50 text-xs"
            >
              <div className="px-3 py-1 text-[10px] font-bold text-slate-400 uppercase">
                샘플 문서 불러오기
              </div>
              <button
                id="load-sample-lesson-hwp"
                onClick={async () => {
                  setSampleMenuOpen(false);
                  await openDocument("samples/(날짜)(교시)(과목명)교수.학습과정안(이름).hwp");
                }}
                className="w-full px-3 py-1.5 text-left hover:bg-slate-50 text-slate-700 flex items-center justify-between"
              >
                <span>📄 교수·학습 과정안 양식 (.hwp)</span>
              </button>
              <button
                id="load-sample-official-hwp"
                onClick={async () => {
                  setSampleMenuOpen(false);
                  await openDocument("samples/demo_official_report.hwp");
                }}
                className="w-full px-3 py-1.5 text-left hover:bg-slate-50 text-slate-700 flex items-center justify-between"
              >
                <span>🏛️ 행정기관 공문서 샘플 (.hwp)</span>
              </button>
              <button
                id="load-sample-official-hwpx"
                onClick={async () => {
                  setSampleMenuOpen(false);
                  await openDocument("samples/demo_official_report.hwpx");
                }}
                className="w-full px-3 py-1.5 text-left hover:bg-slate-50 text-slate-700 flex items-center justify-between"
              >
                <span>🏛️ 행정기관 공문서 샘플 (.hwpx)</span>
              </button>
              <div className="my-1 border-t border-slate-100" />
              <button
                onClick={async () => {
                  setSampleMenuOpen(false);
                  await createBlankDocument(false);
                }}
                className="w-full px-3 py-1.5 text-left hover:bg-slate-50 text-slate-700 flex items-center gap-1.5"
              >
                <FilePlus2 className="w-3.5 h-3.5 text-indigo-600" />
                <span>새 빈 문서 (.hwp)</span>
              </button>
            </div>
          )}
        </div>

        <div className="flex items-center gap-1 text-xs text-slate-500 ml-1 whitespace-nowrap">
          <Clock className="w-3.5 h-3.5 text-slate-400" />
          <span>{isDirty ? "수정됨 ●" : "저장됨"}</span>
        </div>

        <div className="h-4 w-px bg-slate-200 mx-1.5" />

        {/* File Open / Save / Undo quick buttons */}
        <button
          id="header-open-btn"
          onClick={handleOpen}
          className="flex items-center gap-1 text-xs px-2 py-1 rounded text-slate-700 hover:bg-slate-100 font-medium"
          title="로컬 HWP/HWPX 열기 (Ctrl+O)"
        >
          <FolderOpen className="w-3.5 h-3.5 text-slate-600" />
          <span>열기</span>
        </button>

        <button
          id="header-save-btn"
          onClick={handleSave}
          className={`flex items-center gap-1 text-xs px-2 py-1 rounded font-medium transition-colors ${
            isDirty
              ? "bg-teal-50 text-teal-800 border border-teal-200 hover:bg-teal-100"
              : "text-slate-500 hover:bg-slate-100"
          }`}
          title="스테이징 변경사항을 원본 파일에 저장 (Ctrl+S)"
        >
          <Save className="w-3.5 h-3.5" />
          <span>저장{isDirty ? "*" : ""}</span>
        </button>

        <button
          id="header-undo-btn"
          onClick={undoLastChange}
          disabled={undoHistoryBytes.length === 0}
          className={`p-1 rounded transition-colors ${
            undoHistoryBytes.length > 0
              ? "text-slate-700 hover:bg-slate-100"
              : "text-slate-300 cursor-not-allowed"
          }`}
          title="AI/패치 변경 되돌리기 (Ctrl+Z)"
        >
          <Undo2 className="w-3.5 h-3.5" />
        </button>
      </div>

      {/* Right: Pending Diffs Accept All + Dock Switch + Icons + Profile */}
      <div className="flex items-center gap-2">
        {pendingDiffs.length > 0 && (
          <button
            id="header-accept-all-diffs-btn"
            onClick={acceptAllDiffs}
            className="flex items-center gap-1.5 bg-emerald-600 hover:bg-emerald-700 text-white text-xs px-2.5 py-1 rounded-md font-semibold shadow-xs transition-all"
            title="대기 중인 모든 변경사항 일괄 수락 (Ctrl+Enter)"
          >
            <CheckCheck className="w-3.5 h-3.5" />
            <span>모두 적용 ({pendingDiffs.length})</span>
          </button>
        )}

        <button
          id="header-dock-toggle-btn"
          onClick={toggleSidebarDock}
          className="flex items-center gap-1 text-[11px] px-2 py-1 rounded border border-slate-200 text-slate-600 hover:bg-slate-50"
          title="AI 채팅 패널 위치 전환 (좌측 / 우측)"
        >
          <ArrowLeftRight className="w-3 h-3" />
          <span>AI 패널: {sidebarDock === "left" ? "좌측" : "우측"}</span>
        </button>

        <button
          id="header-sidebar-toggle-btn"
          onClick={toggleSidebar}
          className={`p-1.5 rounded transition-colors ${
            sidebarOpen ? "bg-teal-50 text-teal-800" : "text-slate-500 hover:bg-slate-100"
          }`}
          title="AI 사이드바 표시/숨기기 (Ctrl+B)"
        >
          <Sidebar className="w-4 h-4" />
        </button>

        <button
          id="header-ai-settings-btn"
          onClick={() => setSettingsModalOpen(true)}
          className="p-1.5 text-slate-500 hover:bg-slate-100 rounded"
          title="AI 프로바이더(BAZE / Claude / OpenAI / Gemini / Ollama) 및 프라이버시 설정"
        >
          <Settings className="w-4 h-4" />
        </button>

        <button
          id="header-license-btn"
          data-testid="header-license-btn"
          onClick={() => setLicenseModalOpen(true)}
          className="flex items-center gap-1 text-[11px] font-semibold text-slate-600 hover:text-slate-900 hover:bg-slate-100 border border-slate-200 px-2 py-1 rounded transition-colors"
          title="GNU General Public License v3.0 라이선스 정보"
        >
          <Scale className="w-3.5 h-3.5 text-slate-500" />
          <span>GPL v3.0</span>
        </button>
      </div>
    </header>
  );
};
