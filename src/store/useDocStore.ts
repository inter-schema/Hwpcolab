import { create } from "zustand";
import { runtimeInvoke as invoke } from "../core/runtimeInvoke";
import { hopBridge } from "../core/hopBridge";
import { UnifiedOp, DiffItem } from "../types/operations";
import { SelectionTarget, TableCoordinateMeta, OpenedDocSession, DirectEditingTarget } from "../types/hwp";
import { ExcludedRegion } from "../types/privacy";
import { OpTranslator } from "../core/opTranslator";
import { buildTablesMetaForHwpx, mapInspectedTables } from "../core/hwpxTableIndex";
import { ParsedAttachment } from "../core/multiFileParser";
import { PrivacySessionMasker } from "../core/privacyMasker";
import type { ProviderId, AgentLoopMode } from "../services/agent/types";
import type { MemoryMeta } from "../memory/index.ts";
import { createMemoryStore } from "../memory/index.ts";

export { type DiffItem };

export interface AgentLogStats {
  docChecks: number;
  pagesRead: number;
  editsCount: number;
  details: string[];
}

interface DocState {
  originalPath: string | null;
  stagingPath: string | null;
  fileName: string;
  isHwpx: boolean;
  isDirty: boolean;
  isHopDirectDirty: boolean;
  fileBytes: Uint8Array | null;
  undoHistoryBytes: Uint8Array[];
  tablesMeta: TableCoordinateMeta[];
  selection: SelectionTarget | null;
  editingTarget: DirectEditingTarget | null;
  excludedRegions: ExcludedRegion[];
  autoMaskPii: boolean;
  pendingDiffs: DiffItem[];
  zoom: number;
  currentPage: number;
  pageCount: number;
  docMarkdown: string;
  attachments: ParsedAttachment[];
  sidebarOpen: boolean;
  sidebarDock: "left" | "right";
  sidebarWidth: number;
  licenseModalOpen: boolean;
  aiApplyMode: "diff_review" | "immediate";
  settingsModalOpen: boolean;
  provider: ProviderId;
  apiKey: string;
  model: string;
  customEndpoint: string;
  reasoningEffort: "none" | "low" | "medium" | "high" | "xhigh";
  sessionMasker: PrivacySessionMasker;
  agentLoopMode: AgentLoopMode;
  customName: string;
  customSupportsTools: boolean;
  customSendReasoning: boolean;

  memoryModalOpen: boolean;
  memoryModalInitialTab: "list" | "create_multi" | "create_current";
  memoriesIndex: MemoryMeta[];
  activeMemoryIds: string[];

  // Actions
  setSidebarOpen: (open: boolean) => void;
  toggleSidebar: () => void;
  setSidebarDock: (dock: "left" | "right") => void;
  toggleSidebarDock: () => void;
  setSidebarWidth: (width: number) => void;
  setLicenseModalOpen: (open: boolean) => void;
  setAiApplyMode: (mode: "diff_review" | "immediate") => void;
  setSettingsModalOpen: (open: boolean) => void;
  setReasoningEffort: (effort: "none" | "low" | "medium" | "high" | "xhigh") => void;
  setAgentLoopMode: (mode: AgentLoopMode) => void;
  setProviderSettings: (settings: {
    provider?: ProviderId;
    apiKey?: string;
    model?: string;
    customEndpoint?: string;
    customName?: string;
    customSupportsTools?: boolean;
    customSendReasoning?: boolean;
    agentLoopMode?: AgentLoopMode;
  }) => void;
  setZoom: (zoom: number) => void;
  setCurrentPage: (page: number) => void;
  setPageCount: (count: number) => void;
  setTablesMeta: (meta: TableCoordinateMeta[]) => void;
  setSelection: (sel: SelectionTarget | null) => void;
  setEditingTarget: (target: DirectEditingTarget | null) => void;
  markHopDirectEdited: () => void;
  syncHopToStaging: () => Promise<void>;
  applyHumanCellEdit: (tableIndex: number, row: number, col: number, newText: string) => Promise<void>;
  applyHumanParagraphEdit: (sec: number, para: number, newText: string, oldText: string) => Promise<void>;
  applyHumanInsertParagraph: (text: string) => Promise<void>;
  applyHumanNestedTableInsert: (
    tableIndex: number,
    row: number,
    col: number,
    rows: number,
    cols: number
  ) => Promise<void>;
  setAutoMaskPii: (autoMask: boolean) => void;
  toggleExcludeSelection: () => void;
  addAttachment: (att: ParsedAttachment) => void;
  removeAttachment: (id: string) => void;

  openDocument: (filePath: string) => Promise<void>;
  openDocumentBytes: (bytes: Uint8Array, fileName: string, originalPath?: string) => Promise<void>;
  adoptHopDocument: (info: { fileName: string; sourceBytes: Uint8Array | null }) => Promise<void>;
  createBlankDocument: (isHwpx: boolean) => Promise<void>;
  refreshDocMarkdown: () => Promise<void>;
  addProposedDiffs: (items: DiffItem[]) => Promise<void>;
  acceptDiffItem: (diffId: string) => Promise<void>;
  rejectDiffItem: (diffId: string) => void;
  acceptAllDiffs: () => Promise<void>;
  rejectAllDiffs: () => void;
  undoLastChange: () => Promise<void>;
  saveToOriginalFile: () => Promise<void>;

  openMemoryModal: (tab?: "list" | "create_multi" | "create_current") => void;
  closeMemoryModal: () => void;
  refreshMemoriesIndex: () => Promise<void>;
  toggleActiveMemory: (id: string) => void;
  moveActiveMemoryPriority: (id: string, direction: "up" | "down") => void;
  clearActiveMemories: () => void;
}

export const useDocStore = create<DocState>((set, get) => ({
  originalPath: null,
  stagingPath: null,
  fileName: "열린 문서 없음",
  isHwpx: false,
  isDirty: false,
  isHopDirectDirty: false,
  fileBytes: null,
  undoHistoryBytes: [],
  tablesMeta: [],
  selection: null,
  editingTarget: null,
  excludedRegions: [],
  autoMaskPii: false,
  pendingDiffs: [],
  zoom: 1.0,
  currentPage: 1,
  pageCount: 1,
  docMarkdown: "",
  attachments: [],
  sidebarOpen: true,
  sidebarDock: "left",
  sidebarWidth:
    typeof localStorage !== "undefined"
      ? Math.min(900, Math.max(300, Number(localStorage.getItem("hop_sidebar_width")) || 400))
      : 400,
  licenseModalOpen: false,
  aiApplyMode: "diff_review",
  settingsModalOpen: false,
  provider:
    (typeof localStorage !== "undefined" && (localStorage.getItem("hop_provider") as any)) ||
    (import.meta.env?.VITE_DEFAULT_AI_PROVIDER as any) ||
    "baze",
  apiKey:
    (typeof localStorage !== "undefined" &&
      localStorage.getItem(`hop_key_${localStorage.getItem("hop_provider") || "baze"}`)) ||
    (import.meta as any).env?.VITE_BAZE_API_KEY ||
    "",
  model:
    (typeof localStorage !== "undefined" &&
      localStorage.getItem(`hop_model_${localStorage.getItem("hop_provider") || "baze"}`)) ||
    import.meta.env?.VITE_BAZE_DEFAULT_MODEL ||
    "gpt-6-luna",
  customEndpoint:
    (typeof localStorage !== "undefined" &&
      localStorage.getItem(`hop_endpoint_${localStorage.getItem("hop_provider") || "baze"}`)) ||
    import.meta.env?.VITE_BAZE_CHAT_ENDPOINT ||
    "https://api.baze.co/v1/gateway/chat/completions",
  reasoningEffort:
    (typeof localStorage !== "undefined" &&
      (localStorage.getItem("hop_reasoning_effort") as any)) ||
    "high",
  sessionMasker: new PrivacySessionMasker(),
  agentLoopMode:
    (typeof localStorage !== "undefined" && (localStorage.getItem("hop_agent_loop_mode") as AgentLoopMode)) ||
    "legacy",
  customName:
    (typeof localStorage !== "undefined" && localStorage.getItem("hop_custom_name")) ||
    "사용자 지정",
  customSupportsTools:
    typeof localStorage !== "undefined"
      ? localStorage.getItem("hop_custom_supports_tools") !== "false"
      : true,
  customSendReasoning:
    typeof localStorage !== "undefined"
      ? localStorage.getItem("hop_custom_send_reasoning") === "true"
      : false,
  memoryModalOpen: false,
  memoryModalInitialTab: "list",
  memoriesIndex: [],
  activeMemoryIds: [],

  setSidebarOpen: (sidebarOpen) => set({ sidebarOpen }),
  toggleSidebar: () => set((s) => ({ sidebarOpen: !s.sidebarOpen })),
  setSidebarDock: (sidebarDock) => set({ sidebarDock }),
  toggleSidebarDock: () => set((s) => ({ sidebarDock: s.sidebarDock === "left" ? "right" : "left" })),
  setSidebarWidth: (width: number) => {
    const maxW = Math.min(1000, typeof window !== "undefined" ? Math.floor(window.innerWidth * 0.72) : 900);
    const clamped = Math.min(maxW, Math.max(300, width));
    if (typeof localStorage !== "undefined") {
      localStorage.setItem("hop_sidebar_width", String(clamped));
    }
    set({ sidebarWidth: clamped });
  },
  setLicenseModalOpen: (licenseModalOpen) => set({ licenseModalOpen }),
  setAiApplyMode: (aiApplyMode) => set({ aiApplyMode }),
  setSettingsModalOpen: (settingsModalOpen) => set({ settingsModalOpen }),
  setReasoningEffort: (reasoningEffort) => {
    if (typeof localStorage !== "undefined") {
      localStorage.setItem("hop_reasoning_effort", reasoningEffort);
    }
    set({ reasoningEffort });
  },
  setAgentLoopMode: (agentLoopMode) => {
    if (typeof localStorage !== "undefined") {
      localStorage.setItem("hop_agent_loop_mode", agentLoopMode);
    }
    set({ agentLoopMode });
  },
  setProviderSettings: (settings) => set((s) => ({ ...s, ...settings })),
  setZoom: (zoom) => {
    set({ zoom });
    const rt = hopBridge.getRuntime();
    if (rt) {
      rt.canvasView.getViewportManager().setZoom(zoom);
    }
  },
  setCurrentPage: (currentPage) => set({ currentPage }),
  setPageCount: (pageCount) => set({ pageCount }),
  setTablesMeta: (tablesMeta) => set({ tablesMeta }),
  setSelection: (selection) => set({ selection }),
  setEditingTarget: (editingTarget) => set({ editingTarget }),
  setAutoMaskPii: (autoMaskPii) => set({ autoMaskPii }),
  addAttachment: (att) => set((s) => ({ attachments: [...s.attachments, att] })),
  removeAttachment: (id) => set((s) => ({ attachments: s.attachments.filter((a) => a.id !== id) })),

  markHopDirectEdited: () => {
    set({ isDirty: true, isHopDirectDirty: true });
  },

  syncHopToStaging: async () => {
    const { stagingPath, isHwpx, isHopDirectDirty } = get();
    if (!stagingPath || !isHopDirectDirty) return;
    const exported = hopBridge.exportHopBytes(isHwpx);
    if (!exported || exported.byteLength === 0) return;

    await invoke("write_file_bytes", {
      filePath: stagingPath,
      bytes: Array.from(exported),
    });
    set({
      fileBytes: exported,
      isHopDirectDirty: false,
      isDirty: true,
    });
    await get().refreshDocMarkdown();
  },

  openDocument: async (filePath: string) => {
    const session = await invoke<OpenedDocSession>("open_document_session", { filePath });
    const bytes = new Uint8Array(session.bytes);
    set({
      originalPath: session.original_path,
      stagingPath: session.staging_path,
      fileName: session.file_name,
      isHwpx: session.is_hwpx,
      fileBytes: bytes,
      isDirty: false,
      isHopDirectDirty: false,
      undoHistoryBytes: [],
      pendingDiffs: [],
      selection: null,
      excludedRegions: [],
      docMarkdown: "",
      tablesMeta: [],
      sessionMasker: new PrivacySessionMasker(),
    });
    await hopBridge.loadDocumentIntoHop(bytes, session.file_name);
    const rt = hopBridge.getRuntime();
    if (rt) {
      set({ pageCount: Math.max(1, rt.wasm.pageCount) });
    }
    await get().refreshDocMarkdown();
  },

  openDocumentBytes: async (bytes: Uint8Array, fileName: string, originalPath?: string) => {
    const isHwpx = bytes.length >= 4
      && bytes[0] === 0x50
      && bytes[1] === 0x4b
      && bytes[2] === 0x03
      && bytes[3] === 0x04;
    let stagingPath = "";
    try {
      const session = await invoke<OpenedDocSession>("create_blank_session", { isHwpx });
      await invoke("write_file_bytes", {
        filePath: session.staging_path,
        bytes: Array.from(bytes),
      });
      stagingPath = session.staging_path;
    } catch (err) {
      console.warn("[DocStore] 스테이징 세션 생성 생략(정적 브라우저 모드):", err);
    }
    set({
      originalPath: originalPath || null,
      stagingPath: stagingPath || null,
      fileName,
      isHwpx,
      fileBytes: bytes,
      isDirty: false,
      isHopDirectDirty: false,
      undoHistoryBytes: [],
      pendingDiffs: [],
      selection: null,
      excludedRegions: [],
      docMarkdown: "",
      tablesMeta: [],
      sessionMasker: new PrivacySessionMasker(),
    });
    await hopBridge.loadDocumentIntoHop(bytes, fileName);
    const rt = hopBridge.getRuntime();
    if (rt) {
      set({ pageCount: Math.max(1, rt.wasm.pageCount) });
    }
    if (stagingPath) {
      await get().refreshDocMarkdown();
    }
  },

  /**
   * HOP 자체 UI(파일 > 열기 등)로 연 문서를 AI 스토어에 연결합니다.
   * 스테이징 파일을 만들고 문서 본문/표 구조를 추출해 AI 컨텍스트(docMarkdown, tablesMeta)를 채웁니다.
   */
  adoptHopDocument: async ({ fileName, sourceBytes }) => {
    let bytes = sourceBytes;
    if (!bytes) bytes = hopBridge.exportHopBytes(/\.hwpx$/i.test(fileName));
    if (!bytes || bytes.byteLength === 0) return;
    const isHwpx = bytes.length >= 4
      && bytes[0] === 0x50
      && bytes[1] === 0x4b
      && bytes[2] === 0x03
      && bytes[3] === 0x04;
    let stagingPath = "";
    try {
      const session = await invoke<OpenedDocSession>("create_blank_session", { isHwpx });
      await invoke("write_file_bytes", {
        filePath: session.staging_path,
        bytes: Array.from(bytes),
      });
      stagingPath = session.staging_path;
    } catch (err) {
      console.warn("[DocStore] HOP 문서 스테이징 생략(정적 브라우저 모드):", err);
    }
    set({
      originalPath: null,
      stagingPath: stagingPath || null,
      fileName,
      isHwpx,
      fileBytes: bytes,
      isDirty: false,
      isHopDirectDirty: false,
      undoHistoryBytes: [],
      pendingDiffs: [],
      selection: null,
      excludedRegions: [],
      docMarkdown: "",
      tablesMeta: [],
      sessionMasker: new PrivacySessionMasker(),
    });
    const rt = hopBridge.getRuntime();
    if (rt) set({ pageCount: Math.max(1, rt.wasm.pageCount) });
    await get().refreshDocMarkdown();
  },

  createBlankDocument: async (isHwpx: boolean) => {
    const session = await invoke<OpenedDocSession>("create_blank_session", { isHwpx });
    const payload = OpTranslator.toEnginePayload(session.staging_path, isHwpx, [], [], true);
    await invoke("execute_claw_ops", {
      isHwpx: payload.isHwpx,
      payloadJson: payload.stdinJson,
    });
    const bytesArr = await invoke<number[]>("read_file_bytes", { filePath: session.staging_path });
    const bytes = new Uint8Array(bytesArr);
    set({
      originalPath: null,
      stagingPath: session.staging_path,
      fileName: session.file_name,
      isHwpx,
      fileBytes: bytes,
      isDirty: false,
      isHopDirectDirty: false,
      undoHistoryBytes: [],
      pendingDiffs: [],
      selection: null,
      excludedRegions: [],
      docMarkdown: "",
      sessionMasker: new PrivacySessionMasker(),
    });
    await hopBridge.loadDocumentIntoHop(bytes, session.file_name);
    await get().refreshDocMarkdown();
  },

  refreshDocMarkdown: async () => {
    const { stagingPath, isHwpx, fileBytes } = get();
    if (!stagingPath) return;
    try {
      const md = await invoke<string>("extract_markdown", { filePath: stagingPath });
      set({ docMarkdown: md });
    } catch (err) {
      console.warn("[DocStore] Failed to extract markdown:", err);
    }

    const isHwpxDoc = isHwpx ?? /\.hwpx$/i.test(stagingPath);
    if (isHwpxDoc) {
      try {
        let bytes = fileBytes;
        if (!bytes || bytes.byteLength === 0) {
          try {
            const arr = await invoke<number[]>("read_file_bytes", { filePath: stagingPath });
            bytes = new Uint8Array(arr);
            set({ fileBytes: bytes });
          } catch (e) {
            console.warn("[DocStore] Failed to read bytes from stagingPath:", e);
          }
        }
        if (bytes && bytes.byteLength > 0) {
          const existingMeta = get().tablesMeta;
          const parsed = buildTablesMetaForHwpx(bytes, existingMeta);
          const enriched = hopBridge.enrichTablesMetaWithBboxes(parsed);
          set({ tablesMeta: enriched });
        }
      } catch (err) {
        console.warn("[DocStore] Failed to build tablesMeta for HWPX:", err);
      }
    } else {
      try {
        const inspected = await invoke<any>("inspect_document", { filePath: stagingPath });
        if (inspected && Array.isArray(inspected.tables)) {
          const existingMeta = get().tablesMeta;
          const merged = mapInspectedTables(inspected.tables, existingMeta);
          const enriched = hopBridge.enrichTablesMetaWithBboxes(merged);
          set({ tablesMeta: enriched });
        }
      } catch (err) {
        console.warn("[DocStore] Failed to refresh tablesMeta for HWP:", err);
      }
    }
  },

  toggleExcludeSelection: () => {
    const { selection, excludedRegions } = get();
    if (!selection) return;

    if (selection.kind === "table_cells") {
      const next = [...excludedRegions];
      for (const cell of selection.selectedCells) {
        const existsIdx = next.findIndex(
          (r) =>
            r.targetType === "cell" &&
            r.tableIndex === selection.tableIndex &&
            r.row === cell.row &&
            r.col === cell.col
        );
        if (existsIdx >= 0) {
          next.splice(existsIdx, 1);
        } else {
          next.push({
            id: crypto.randomUUID(),
            label: `표 ${selection.tableIndex + 1} (${cell.row + 1}행 ${cell.col + 1}열)`,
            targetType: "cell",
            tableIndex: selection.tableIndex,
            row: cell.row,
            col: cell.col,
            rawText: cell.text,
          });
        }
      }
      set({ excludedRegions: next });
    } else if (selection.kind === "body_range" && selection.selectedText.trim()) {
      const existsIdx = excludedRegions.findIndex(
        (r) => r.targetType === "body_text" && r.rawText === selection.selectedText
      );
      if (existsIdx >= 0) {
        const next = [...excludedRegions];
        next.splice(existsIdx, 1);
        set({ excludedRegions: next });
      } else {
        set({
          excludedRegions: [
            ...excludedRegions,
            {
              id: crypto.randomUUID(),
              label: `본문 문단 ${selection.startPara + 1}`,
              targetType: "body_text",
              rawText: selection.selectedText,
              rects: selection.rects,
            },
          ],
        });
      }
    }
  },

  addProposedDiffs: async (items) => {
    const { aiApplyMode, tablesMeta } = get();
    // Ensure any diff item without a bbox gets one from enriched tablesMeta if targetCoord is present
    const enrichedItems = items.map((item) => {
      if (!item.bbox && item.targetCoord && typeof item.targetCoord.tableIndex === "number") {
        const t = tablesMeta[item.targetCoord.tableIndex];
        const c = t?.cells.find((cell) => cell.row === item.targetCoord?.row && cell.col === item.targetCoord?.col);
        if (c?.bbox) {
          return {
            ...item,
            bbox: {
              pageIndex: c.bbox.pageIndex,
              x: c.bbox.x,
              y: c.bbox.y,
              w: c.bbox.w,
              h: c.bbox.h,
            },
          };
        }
      }
      return item;
    });

    set({ pendingDiffs: enrichedItems });
    if (aiApplyMode === "immediate" && enrichedItems.length > 0) {
      await get().acceptAllDiffs();
    }
  },

  rejectDiffItem: (diffId) =>
    set((state) => ({
      pendingDiffs: state.pendingDiffs.filter((d) => d.id !== diffId),
    })),

  rejectAllDiffs: () => set({ pendingDiffs: [] }),

  acceptDiffItem: async (diffId) => {
    await get().syncHopToStaging();
    const { stagingPath, isHwpx, pendingDiffs, tablesMeta, fileBytes, undoHistoryBytes, fileName } = get();
    const target = pendingDiffs.find((d) => d.id === diffId);
    if (!target || !stagingPath) return;

    const payload = OpTranslator.toEnginePayload(
      stagingPath,
      isHwpx,
      target.unifiedOps,
      tablesMeta,
      false
    );

    const res = await invoke<{ ok: boolean; stdout_json: any; stderr: string }>("execute_claw_ops", {
      isHwpx: payload.isHwpx,
      payloadJson: payload.stdinJson,
    });

    if (!res.ok) {
      throw new Error(`패치 적용 실패: ${JSON.stringify(res.stdout_json)} ${res.stderr}`);
    }

    const updatedBytesArr = await invoke<number[]>("read_file_bytes", { filePath: stagingPath });
    const updatedBytes = new Uint8Array(updatedBytesArr);

    set({
      fileBytes: updatedBytes,
      isDirty: true,
      isHopDirectDirty: false,
      undoHistoryBytes: fileBytes ? [...undoHistoryBytes, fileBytes] : undoHistoryBytes,
      pendingDiffs: pendingDiffs.filter((d) => d.id !== diffId),
    });

    const scrollContainer = typeof document !== "undefined" ? document.getElementById("scroll-container") : null;
    const savedScrollTop = scrollContainer?.scrollTop ?? 0;
    const savedScrollLeft = scrollContainer?.scrollLeft ?? 0;

    await hopBridge.loadDocumentIntoHop(updatedBytes, fileName);
    if (scrollContainer) {
      scrollContainer.scrollTop = savedScrollTop;
      scrollContainer.scrollLeft = savedScrollLeft;
    }
    await get().refreshDocMarkdown();
  },

  acceptAllDiffs: async () => {
    await get().syncHopToStaging();
    const { stagingPath, isHwpx, pendingDiffs, tablesMeta, fileBytes, undoHistoryBytes, fileName } = get();
    if (!stagingPath || pendingDiffs.length === 0) return;

    const batchedOps = pendingDiffs.flatMap((d) => d.unifiedOps);
    const payload = OpTranslator.toEnginePayload(stagingPath, isHwpx, batchedOps, tablesMeta, false);

    const res = await invoke<{ ok: boolean; stdout_json: any; stderr: string }>("execute_claw_ops", {
      isHwpx: payload.isHwpx,
      payloadJson: payload.stdinJson,
    });

    if (!res.ok) {
      throw new Error(`일괄 패치 적용 실패: ${JSON.stringify(res.stdout_json)} ${res.stderr}`);
    }

    const updatedBytesArr = await invoke<number[]>("read_file_bytes", { filePath: stagingPath });
    const updatedBytes = new Uint8Array(updatedBytesArr);

    set({
      fileBytes: updatedBytes,
      isDirty: true,
      isHopDirectDirty: false,
      undoHistoryBytes: fileBytes ? [...undoHistoryBytes, fileBytes] : undoHistoryBytes,
      pendingDiffs: [],
    });

    const scrollContainer = typeof document !== "undefined" ? document.getElementById("scroll-container") : null;
    const savedScrollTop = scrollContainer?.scrollTop ?? 0;
    const savedScrollLeft = scrollContainer?.scrollLeft ?? 0;

    await hopBridge.loadDocumentIntoHop(updatedBytes, fileName);
    if (scrollContainer) {
      scrollContainer.scrollTop = savedScrollTop;
      scrollContainer.scrollLeft = savedScrollLeft;
    }
    await get().refreshDocMarkdown();
  },

  undoLastChange: async () => {
    const { stagingPath, undoHistoryBytes, fileName } = get();
    if (!stagingPath || undoHistoryBytes.length === 0) return;
    const prevBytes = undoHistoryBytes[undoHistoryBytes.length - 1];
    await invoke("write_file_bytes", { filePath: stagingPath, bytes: Array.from(prevBytes) });
    set({
      fileBytes: prevBytes,
      isHopDirectDirty: false,
      undoHistoryBytes: undoHistoryBytes.slice(0, -1),
    });
    await hopBridge.loadDocumentIntoHop(prevBytes, fileName);
    await get().refreshDocMarkdown();
  },

  applyHumanCellEdit: async (tableIndex: number, row: number, col: number, newText: string) => {
    await get().syncHopToStaging();
    const { stagingPath, isHwpx, tablesMeta, fileBytes, undoHistoryBytes, fileName } = get();
    if (!stagingPath) return;

    const op: UnifiedOp = {
      type: "set_cell_text",
      tableIndex,
      row,
      col,
      text: newText,
    };

    const payload = OpTranslator.toEnginePayload(stagingPath, isHwpx, [op], tablesMeta, false);
    const res = await invoke<{ ok: boolean; stdout_json: any; stderr: string }>("execute_claw_ops", {
      isHwpx: payload.isHwpx,
      payloadJson: payload.stdinJson,
    });

    if (!res.ok) {
      throw new Error(`셀 수정 실패: ${JSON.stringify(res.stdout_json)} ${res.stderr}`);
    }

    const updatedBytesArr = await invoke<number[]>("read_file_bytes", { filePath: stagingPath });
    const updatedBytes = new Uint8Array(updatedBytesArr);

    set({
      fileBytes: updatedBytes,
      isDirty: true,
      isHopDirectDirty: false,
      undoHistoryBytes: fileBytes ? [...undoHistoryBytes, fileBytes] : undoHistoryBytes,
      editingTarget: null,
      selection: null,
    });
    await hopBridge.loadDocumentIntoHop(updatedBytes, fileName);
    await get().refreshDocMarkdown();
  },

  applyHumanNestedTableInsert: async (tableIndex: number, row: number, col: number, rows: number, cols: number) => {
    await get().syncHopToStaging();
    const { stagingPath, isHwpx, tablesMeta, fileBytes, undoHistoryBytes, fileName } = get();
    if (!stagingPath) return;

    if (!isHwpx) {
      throw new Error("중첩 표는 국가 표준 HWPX 문서에서만 지원됩니다. HWPX로 저장 후 시도해 주세요.");
    }

    const op: UnifiedOp = {
      type: "insert_nested_table",
      tableIndex,
      row,
      col,
      rows,
      cols,
      widthMode: "fit_cell",
    };

    const payload = OpTranslator.toEnginePayload(stagingPath, isHwpx, [op], tablesMeta, false);
    const res = await invoke<{ ok: boolean; stdout_json: any; stderr: string }>("execute_claw_ops", {
      isHwpx: payload.isHwpx,
      payloadJson: payload.stdinJson,
    });

    if (!res.ok) {
      throw new Error(`중첩 표 삽입 실패: ${JSON.stringify(res.stdout_json)} ${res.stderr}`);
    }

    const updatedBytesArr = await invoke<number[]>("read_file_bytes", { filePath: stagingPath });
    const updatedBytes = new Uint8Array(updatedBytesArr);

    set({
      fileBytes: updatedBytes,
      isDirty: true,
      isHopDirectDirty: false,
      undoHistoryBytes: fileBytes ? [...undoHistoryBytes, fileBytes] : undoHistoryBytes,
      editingTarget: null,
      selection: null,
    });
    await hopBridge.loadDocumentIntoHop(updatedBytes, fileName);
    await get().refreshDocMarkdown();
  },

  applyHumanParagraphEdit: async (sec: number, para: number, newText: string, oldText: string) => {
    await get().syncHopToStaging();
    const { stagingPath, isHwpx, tablesMeta, fileBytes, undoHistoryBytes, fileName } = get();
    if (!stagingPath) return;

    const op: UnifiedOp = isHwpx
      ? { type: "set_paragraph_text", paraIndex: para, text: newText }
      : oldText && oldText.trim()
      ? { type: "replace_body_text", find: oldText.trim(), replace: newText.trim() }
      : { type: "set_paragraph_text", paraIndex: para, text: newText, oldTextForHwp: oldText };

    const payload = OpTranslator.toEnginePayload(stagingPath, isHwpx, [op], tablesMeta, false);
    const res = await invoke<{ ok: boolean; stdout_json: any; stderr: string }>("execute_claw_ops", {
      isHwpx: payload.isHwpx,
      payloadJson: payload.stdinJson,
    });

    if (!res.ok) {
      throw new Error(`문단 수정 실패: ${JSON.stringify(res.stdout_json)} ${res.stderr}`);
    }

    const updatedBytesArr = await invoke<number[]>("read_file_bytes", { filePath: stagingPath });
    const updatedBytes = new Uint8Array(updatedBytesArr);

    set({
      fileBytes: updatedBytes,
      isDirty: true,
      isHopDirectDirty: false,
      undoHistoryBytes: fileBytes ? [...undoHistoryBytes, fileBytes] : undoHistoryBytes,
      editingTarget: null,
      selection: null,
    });
    await hopBridge.loadDocumentIntoHop(updatedBytes, fileName);
    await get().refreshDocMarkdown();
  },

  applyHumanInsertParagraph: async (text: string) => {
    await get().syncHopToStaging();
    const { stagingPath, isHwpx, tablesMeta, fileBytes, undoHistoryBytes, fileName } = get();
    if (!stagingPath) return;

    const op: UnifiedOp = { type: "append_paragraph", text };
    const payload = OpTranslator.toEnginePayload(stagingPath, isHwpx, [op], tablesMeta, false);
    const res = await invoke<{ ok: boolean; stdout_json: any; stderr: string }>("execute_claw_ops", {
      isHwpx: payload.isHwpx,
      payloadJson: payload.stdinJson,
    });

    if (!res.ok) {
      throw new Error(`문단 추가 실패: ${JSON.stringify(res.stdout_json)} ${res.stderr}`);
    }

    const updatedBytesArr = await invoke<number[]>("read_file_bytes", { filePath: stagingPath });
    const updatedBytes = new Uint8Array(updatedBytesArr);

    set({
      fileBytes: updatedBytes,
      isDirty: true,
      isHopDirectDirty: false,
      undoHistoryBytes: fileBytes ? [...undoHistoryBytes, fileBytes] : undoHistoryBytes,
    });
    await hopBridge.loadDocumentIntoHop(updatedBytes, fileName);
    await get().refreshDocMarkdown();
  },

  saveToOriginalFile: async () => {
    await get().syncHopToStaging();
    const { originalPath, stagingPath } = get();
    if (!stagingPath) return;
    const finalPath = await invoke<string>("commit_staging_to_original", {
      stagingPath,
      originalPath: originalPath || "",
    });
    set({ originalPath: finalPath, isDirty: false, isHopDirectDirty: false });
  },

  openMemoryModal: (tab) => {
    set({ memoryModalOpen: true, memoryModalInitialTab: tab ?? "list" });
    void get().refreshMemoriesIndex();
  },
  closeMemoryModal: () => set({ memoryModalOpen: false }),
  refreshMemoriesIndex: async () => {
    const store = createMemoryStore();
    const list = await store.list();
    const currentActive = get().activeMemoryIds;
    const validIds = currentActive.filter((id) => list.some((m) => m.id === id));
    set({ memoriesIndex: list, activeMemoryIds: validIds });
  },
  toggleActiveMemory: (id) => {
    const { activeMemoryIds } = get();
    if (activeMemoryIds.includes(id)) {
      set({ activeMemoryIds: activeMemoryIds.filter((x) => x !== id) });
    } else {
      set({ activeMemoryIds: [...activeMemoryIds, id] });
    }
  },
  moveActiveMemoryPriority: (id, direction) => {
    const { activeMemoryIds } = get();
    const idx = activeMemoryIds.indexOf(id);
    if (idx === -1) return;
    const targetIdx = direction === "up" ? idx - 1 : idx + 1;
    if (targetIdx < 0 || targetIdx >= activeMemoryIds.length) return;
    const next = [...activeMemoryIds];
    const [item] = next.splice(idx, 1);
    next.splice(targetIdx, 0, item);
    set({ activeMemoryIds: next });
  },
  clearActiveMemories: () => set({ activeMemoryIds: [] }),
}));

if (typeof window !== 'undefined') {
  (window as any).__docStore = useDocStore;
}
