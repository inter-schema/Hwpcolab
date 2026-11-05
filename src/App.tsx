import React, { useEffect, useState, useRef } from "react";
import { HeaderToolbar } from "./components/header/HeaderToolbar";
import { HopEditorHost } from "./components/viewer/HopEditorHost";
import { AiChatSidebar } from "./components/sidebar/AiChatSidebar";
import { ModelPrivacySettings } from "./components/sidebar/ModelPrivacySettings";
import { LicenseModal } from "./components/header/LicenseModal";
import { WritingMemoryModal } from "./components/memory/WritingMemoryModal";
import { useDocStore } from "./store/useDocStore";
import { isTauriEnv, runtimeInvoke } from "./core/runtimeInvoke";

export const App: React.FC = () => {
  const {
    openDocument,
    saveToOriginalFile,
    undoLastChange,
    acceptAllDiffs,
    toggleSidebar,
    pendingDiffs,
    isDirty,
    undoHistoryBytes,
    setProviderSettings,
    sidebarOpen,
    sidebarDock,
    sidebarWidth,
    setSidebarWidth,
    refreshMemoriesIndex,
  } = useDocStore();

  const [isResizing, setIsResizing] = useState(false);
  const [isDragging, setIsDragging] = useState(false);
  const dragStartRef = useRef<{ x: number; active: boolean }>({ x: 0, active: false });

  const pendingDiffsCount = pendingDiffs.filter((d) => d.status === "pending").length;

  useEffect(() => {
    void refreshMemoriesIndex();
    const handleOpenWritingMemory = (e: Event) => {
      const customEvent = e as CustomEvent<{ tab?: "list" | "create_multi" | "create_current" }>;
      const tab = customEvent.detail?.tab;
      useDocStore.getState().openMemoryModal(tab || "create_multi");
    };
    window.addEventListener("hop:open-writing-memory", handleOpenWritingMemory);
    return () => {
      window.removeEventListener("hop:open-writing-memory", handleOpenWritingMemory);
    };
  }, [refreshMemoriesIndex]);

  // Check startup file argument & restore saved LLM settings
  useEffect(() => {
    const savedProvider = localStorage.getItem("hop_provider") as any;
    if (savedProvider) {
      const savedKey = localStorage.getItem(`hop_key_${savedProvider}`) || "";
      const savedModel = localStorage.getItem(`hop_model_${savedProvider}`) || "";
      const savedEndpoint =
        savedProvider === "baze"
          ? localStorage.getItem("hop_endpoint_baze") || "https://api.baze.co/v1/gateway/chat/completions"
          : savedProvider === "custom"
          ? localStorage.getItem("hop_endpoint_custom") || ""
          : savedProvider === "openai"
          ? localStorage.getItem("hop_endpoint_openai") || "https://api.openai.com/v1/chat/completions"
          : savedProvider === "gemini"
          ? localStorage.getItem("hop_endpoint_gemini") || "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions"
          : localStorage.getItem("hop_endpoint_ollama") || "http://127.0.0.1:11434/v1/chat/completions";
      const savedCustomName = localStorage.getItem("hop_custom_name") || "사용자 지정";
      const savedCustomSupportsTools = localStorage.getItem("hop_custom_supports_tools") !== "false";
      const savedCustomSendReasoning = localStorage.getItem("hop_custom_send_reasoning") === "true";
      const savedLoopMode = (localStorage.getItem("hop_agent_loop_mode") as any) || "legacy";

      setProviderSettings({
        provider: savedProvider,
        ...(savedKey ? { apiKey: savedKey } : {}),
        ...(savedModel ? { model: savedModel } : {}),
        customEndpoint: savedEndpoint,
        customName: savedCustomName,
        customSupportsTools: savedCustomSupportsTools,
        customSendReasoning: savedCustomSendReasoning,
        agentLoopMode: savedLoopMode,
      });
    }

    runtimeInvoke<string | null>("get_startup_file")
      .then(async (filePath) => {
        if (filePath) {
          await openDocument(filePath);
        } else {
          // Default to the sample lesson plan document so the HOP editor opens ready to use
          try {
            await openDocument("samples/(날짜)(교시)(과목명)교수.학습과정안(이름).hwp");
          } catch {
            try {
              await openDocument("samples/demo_official_report.hwp");
            } catch (err) {
              console.warn("Could not open default sample document:", err);
            }
          }
        }
      })
      .catch(() => {});
  }, [openDocument, setProviderSettings]);

  // Global Keyboard Shortcuts (Section 7.7)
  useEffect(() => {
    const handleKeyDown = async (e: KeyboardEvent) => {
      if (e.defaultPrevented) return;

      // Ctrl+O: Open document
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "o" && !e.altKey && !e.shiftKey) {
        e.preventDefault();
        try {
          if (isTauriEnv()) {
            const selected = await runtimeInvoke<string | null>("pick_local_file", {
              extensions: ["hwp", "hwpx"],
            });
            if (selected) {
              await openDocument(selected);
            }
          } else {
            const fileInput = document.getElementById("header-hwp-file-input") as HTMLInputElement | null;
            fileInput?.click();
          }
        } catch (err) {
          console.error("Open shortcut error:", err);
        }
      }

      // Ctrl+S: Save document
      if (e.ctrlKey && e.key.toLowerCase() === "s" && !e.shiftKey) {
        e.preventDefault();
        if (isDirty) {
          try {
            await saveToOriginalFile();
          } catch (err) {
            console.error("Save shortcut error:", err);
          }
        }
      }

      // Ctrl+B: Toggle AI Sidebar
      if (e.ctrlKey && e.key.toLowerCase() === "b") {
        // Only intercept if not inside HOP canvas text editing
        const activeTag = document.activeElement?.tagName.toLowerCase();
        if (activeTag !== "textarea" && activeTag !== "input") {
          e.preventDefault();
          toggleSidebar();
        }
      }

      // Ctrl+K: Open or focus Inline Quick Toolbar prompt input
      if (e.ctrlKey && e.key.toLowerCase() === "k") {
        e.preventDefault();
        const inlineInput = document.getElementById("inline-quick-prompt-input") as HTMLInputElement | null;
        if (inlineInput) {
          inlineInput.focus();
          inlineInput.select();
        } else {
          const openBtn = document.querySelector('[data-testid="open-inline-ai-toolbar-btn"]') as HTMLButtonElement | null;
          openBtn?.click();
        }
      }

      // Ctrl+Enter: Accept all pending diffs
      if (e.ctrlKey && e.key === "Enter") {
        if (pendingDiffsCount > 0) {
          e.preventDefault();
          try {
            await acceptAllDiffs();
          } catch (err) {
            console.error("Accept all diffs shortcut error:", err);
          }
        }
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [
    openDocument,
    saveToOriginalFile,
    undoLastChange,
    acceptAllDiffs,
    toggleSidebar,
    pendingDiffsCount,
    isDirty,
    undoHistoryBytes,
  ]);

  // Sidebar drag resizer effect
  useEffect(() => {
    if (!isResizing) return;

    let animFrameId: number | null = null;

    const handleMouseMove = (e: MouseEvent) => {
      if (!dragStartRef.current.active) {
        if (Math.abs(e.clientX - dragStartRef.current.x) > 2) {
          dragStartRef.current.active = true;
          setIsDragging(true);
        } else {
          return;
        }
      }

      if (animFrameId !== null) return;
      animFrameId = window.requestAnimationFrame(() => {
        animFrameId = null;
        const maxW = Math.max(360, Math.floor(window.innerWidth * 0.72));
        const newWidth = sidebarDock === "left" ? e.clientX : window.innerWidth - e.clientX;
        const clamped = Math.min(maxW, Math.max(300, newWidth));
        setSidebarWidth(clamped);
        window.dispatchEvent(new Event("resize"));
      });
    };

    const handleMouseUp = (e: MouseEvent) => {
      setIsResizing(false);
      setIsDragging(false);
      if (animFrameId !== null) {
        window.cancelAnimationFrame(animFrameId);
        animFrameId = null;
      }
      if (dragStartRef.current.active) {
        dragStartRef.current.active = false;
        const maxW = Math.max(360, Math.floor(window.innerWidth * 0.72));
        const newWidth = sidebarDock === "left" ? e.clientX : window.innerWidth - e.clientX;
        const clamped = Math.min(maxW, Math.max(300, newWidth));
        setSidebarWidth(clamped);
        window.dispatchEvent(new Event("resize"));
      }
    };

    window.addEventListener("mousemove", handleMouseMove);
    window.addEventListener("mouseup", handleMouseUp);
    return () => {
      window.removeEventListener("mousemove", handleMouseMove);
      window.removeEventListener("mouseup", handleMouseUp);
      if (animFrameId !== null) {
        window.cancelAnimationFrame(animFrameId);
      }
    };
  }, [isResizing, sidebarDock, setSidebarWidth]);

  const resizeHandle = sidebarOpen && (
    <div
      id="sidebar-resize-handle"
      data-testid="sidebar-resize-handle"
      onMouseDown={(e) => {
        if (e.button !== 0) return;
        dragStartRef.current = { x: e.clientX, active: false };
        setIsResizing(true);
      }}
      onDoubleClick={(e) => {
        e.preventDefault();
        e.stopPropagation();
        setSidebarWidth(400);
        window.dispatchEvent(new Event("resize"));
      }}
      className="w-1.5 cursor-col-resize group relative flex items-center justify-center bg-slate-100 hover:bg-teal-500/30 active:bg-teal-600/50 transition-colors z-30 shrink-0 select-none"
      title="드래그하여 AI 대화창과 편집기 비율 조정 (더블클릭 시 기본값 복원)"
    >
      <div className="w-0.5 h-8 rounded-full bg-slate-300 group-hover:bg-teal-600 transition-colors" />
    </div>
  );

  return (
    <div className="flex flex-col w-screen h-screen overflow-hidden bg-white font-sans">
      {/* 1. Top Header Bar (< HWP 편집 [문서명] ... AI 컨트롤) */}
      <HeaderToolbar />

      {/* 2. Main Studio Body: Dockable AI Chat Panel + Genuine HOP Editor */}
      <div className="flex-1 flex overflow-hidden relative">
        {sidebarDock === "left" && (
          <>
            <AiChatSidebar />
            {resizeHandle}
          </>
        )}
        <HopEditorHost />
        {sidebarDock === "right" && (
          <>
            {resizeHandle}
            <AiChatSidebar />
          </>
        )}
      </div>

      {/* Full-screen transparent overlay during drag to maintain smooth drag over canvas */}
      {isDragging && (
        <div className="fixed inset-0 z-50 cursor-col-resize select-none" />
      )}

      {/* 3. Settings Modal */}
      <ModelPrivacySettings />

      {/* 4. License Modal */}
      <LicenseModal />

      {/* 5. Writing Memory Modal */}
      <WritingMemoryModal />
    </div>
  );
};

export default App;
