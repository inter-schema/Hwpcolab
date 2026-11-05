import React, { useState, useEffect, useRef } from "react";
import { useDocStore } from "../../store/useDocStore";
import { parseLocalAttachment } from "../../core/multiFileParser";
import {
  FIXED_MEMORY_SECTIONS,
  type MemorySectionTitle,
  type MemoryDoc,
  type MemoryMeta,
  type SectionDiff,
  type GenerateMemoryInputDoc,
  type GenerateMemoryResult,
  createMemoryStore,
  generateMemory,
  proposeRefinement,
  applySectionDecisions,
  scanAndRedactPii,
  normalizeMemoryBody,
  parseMemorySections,
  serializeMemorySections,
} from "../../memory/index.ts";
import type { ProviderConfig } from "../../services/agent/types";
import { MemoryDiffViewer } from "./MemoryDiffViewer";
import {
  X,
  BookMarked,
  FileText,
  Files,
  Upload,
  Download,
  Copy,
  Trash2,
  Save,
  Wand2,
  History,
  FilePlus2,
  AlertTriangle,
  CheckCircle2,
  Loader2,
  RotateCcw,
  Eye,
} from "lucide-react";

async function extractTextFromFile(file: File): Promise<string> {
  const bytes = new Uint8Array(await file.arrayBuffer());
  const nameLower = file.name.toLowerCase();
  if (
    nameLower.endsWith(".md") ||
    nameLower.endsWith(".markdown") ||
    nameLower.endsWith(".txt")
  ) {
    return new TextDecoder("utf-8").decode(bytes);
  }
  const parsed = await parseLocalAttachment(file.name, bytes);
  return (parsed as any).extractedMarkdown || parsed.markdownSummary || "";
}

function getProviderConfig(): ProviderConfig {
  const s = useDocStore.getState();
  return {
    provider: s.provider,
    apiKey: s.apiKey,
    model: s.model,
    endpoint: s.customEndpoint || undefined,
    customName: s.customName,
    customSupportsTools: s.customSupportsTools,
    customSendReasoning: s.customSendReasoning,
  };
}

export const WritingMemoryModal: React.FC = () => {
  const {
    memoryModalOpen,
    memoryModalInitialTab,
    closeMemoryModal,
    memoriesIndex,
    refreshMemoriesIndex,
    fileName,
    docMarkdown,
  } = useDocStore();

  const [activeTab, setActiveTab] = useState<"list" | "create_multi" | "create_current">("list");
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [selectedDoc, setSelectedDoc] = useState<MemoryDoc | null>(null);
  const [activeSubview, setActiveSubview] = useState<"edit" | "refine" | "revisions">("edit");
  const [isDeleting, setIsDeleting] = useState(false);
  const [statusMessage, setStatusMessage] = useState<string | null>(null);
  const [warningsList, setWarningsList] = useState<string[]>([]);
  const [isBusy, setIsBusy] = useState(false);

  // Tab 1 Subview A: Direct Edit State
  const [editName, setEditName] = useState("");
  const [editDesc, setEditDesc] = useState("");
  const [editedSections, setEditedSections] = useState<Record<MemorySectionTitle, string>>({
    "문서 유형·양식": "",
    "어투·문체": "",
    "용어·표기": "",
    "서식·표 규칙": "",
    "구조(제목 체계)": "",
    "금지·지양": "",
    "예시 문구(일반화됨)": "",
  });

  // Tab 1 Subview B: AI Refine State
  const [refineInstruction, setRefineInstruction] = useState("");
  const [refineFiles, setRefineFiles] = useState<File[]>([]);
  const [refineDiffs, setRefineDiffs] = useState<SectionDiff[] | null>(null);
  const [acceptedRefineTitles, setAcceptedRefineTitles] = useState<string[]>([]);
  const [refineExtraDocsRef, setRefineExtraDocsRef] = useState<any[]>([]);

  // Tab 1 Subview C: Revisions State
  const [revisionsList, setRevisionsList] = useState<number[]>([]);
  const [previewRevisionDoc, setPreviewRevisionDoc] = useState<{ rev: number; doc: MemoryDoc } | null>(null);

  // Tab 2: Create Multi State
  const [createMultiName, setCreateMultiName] = useState("");
  const [createMultiDesc, setCreateMultiDesc] = useState("");
  const [createMultiFiles, setCreateMultiFiles] = useState<File[]>([]);

  // Tab 3: Create Current State
  const [createCurrentName, setCreateCurrentName] = useState("");
  const [createCurrentDesc, setCreateCurrentDesc] = useState("");

  // Common Preview Panel State (for Tab 2 & Tab 3)
  const [previewResult, setPreviewResult] = useState<GenerateMemoryResult | null>(null);
  const [previewBody, setPreviewBody] = useState("");
  const [previewOriginTab, setPreviewOriginTab] = useState<"create_multi" | "create_current">("create_multi");

  const importFileInputRef = useRef<HTMLInputElement>(null);
  const createMultiFileInputRef = useRef<HTMLInputElement>(null);
  const refineFileInputRef = useRef<HTMLInputElement>(null);

  // Synchronize initial tab and index on modal open
  useEffect(() => {
    if (memoryModalOpen) {
      setActiveTab(memoryModalInitialTab);
      setIsDeleting(false);
      setStatusMessage(null);
      setWarningsList([]);
      setPreviewResult(null);
      void refreshMemoriesIndex();
    }
  }, [memoryModalOpen, memoryModalInitialTab, refreshMemoriesIndex]);

  // Set default current document name when opening tab 3
  useEffect(() => {
    if (activeTab === "create_current") {
      const baseName = fileName ? fileName.replace(/\.[^/.]+$/, "") : "현재 문서";
      setCreateCurrentName(baseName.trim() ? `${baseName} 양식` : "새 작성 선호사항");
      setCreateCurrentDesc("");
    }
  }, [activeTab, fileName]);

  // Auto-select first memory if available and none selected
  useEffect(() => {
    if (activeTab === "list" && memoriesIndex.length > 0 && !selectedId) {
      setSelectedId(memoriesIndex[0].id);
    }
  }, [activeTab, memoriesIndex, selectedId]);

  // Load selected doc when selectedId changes
  useEffect(() => {
    let cancelled = false;
    async function load() {
      if (!selectedId) {
        setSelectedDoc(null);
        return;
      }
      const store = createMemoryStore();
      const doc = await store.get(selectedId);
      if (cancelled) return;
      if (doc) {
        setSelectedDoc(doc);
        setEditName(doc.meta.name);
        setEditDesc(doc.meta.description || "");
        const parsed = parseMemorySections(doc.body);
        setEditedSections(parsed);
        const revs = await store.listRevisions(selectedId);
        if (!cancelled) setRevisionsList(revs);
      } else {
        setSelectedDoc(null);
      }
      setIsDeleting(false);
      setRefineDiffs(null);
      setPreviewRevisionDoc(null);
    }
    void load();
    return () => {
      cancelled = true;
    };
  }, [selectedId]);

  if (!memoryModalOpen) {
    return null;
  }

  // Handle Tab 1 Direct Save
  const handleSaveDirect = async () => {
    if (!selectedDoc) return;
    setIsBusy(true);
    setStatusMessage(null);
    try {
      const store = createMemoryStore();
      const serialized = serializeMemorySections(editedSections);
      const sanitizedBody = scanAndRedactPii(serialized).text;
      const sanitizedName = scanAndRedactPii(editName).text;
      const sanitizedDesc = scanAndRedactPii(editDesc).text;

      const updatedDoc: MemoryDoc = {
        meta: {
          ...selectedDoc.meta,
          name: sanitizedName,
          description: sanitizedDesc,
          updatedAt: new Date().toISOString(),
        },
        body: sanitizedBody,
      };

      await store.save(updatedDoc, { recordRevision: true });
      await refreshMemoriesIndex();
      const reloaded = await store.get(selectedDoc.meta.id);
      setSelectedDoc(reloaded);
      const revs = await store.listRevisions(selectedDoc.meta.id);
      setRevisionsList(revs);
      setStatusMessage("직접 편집 내용이 새 리비전으로 저장되었습니다.");
    } catch (e: any) {
      setStatusMessage(`저장 실패: ${e?.message || e}`);
    } finally {
      setIsBusy(false);
    }
  };

  // Handle Tab 1 Propose Refine
  const handleProposeRefine = async () => {
    if (!selectedDoc) return;
    setIsBusy(true);
    setStatusMessage(null);
    try {
      const extraDocs: GenerateMemoryInputDoc[] = [];
      for (const file of refineFiles.slice(0, 10)) {
        const text = await extractTextFromFile(file);
        if (text.trim()) {
          extraDocs.push({ name: file.name, text });
        }
      }

      const res = await proposeRefinement({
        doc: selectedDoc,
        instruction: refineInstruction,
        extraDocs,
        provider: getProviderConfig(),
      });

      setRefineDiffs(res.sectionDiffs);
      setAcceptedRefineTitles(
        res.sectionDiffs.filter((d) => d.changed).map((d) => d.sectionTitle)
      );
      setRefineExtraDocsRef(res.extraSourceDocs);
      if (res.warnings.length > 0) {
        setWarningsList(res.warnings);
      }
    } catch (e: any) {
      setStatusMessage(`수정안 생성 실패: ${e?.message || e}`);
    } finally {
      setIsBusy(false);
    }
  };

  // Handle Tab 1 Apply Refine Decisions
  const handleApplyRefine = async () => {
    if (!selectedDoc || !refineDiffs) return;
    setIsBusy(true);
    setStatusMessage(null);
    try {
      const store = createMemoryStore();
      const nextDoc = applySectionDecisions(
        selectedDoc,
        refineDiffs,
        acceptedRefineTitles,
        refineExtraDocsRef
      );

      await store.save(nextDoc, { recordRevision: true });
      await refreshMemoriesIndex();
      const reloaded = await store.get(selectedDoc.meta.id);
      setSelectedDoc(reloaded);
      if (reloaded) {
        setEditedSections(parseMemorySections(reloaded.body));
      }
      const revs = await store.listRevisions(selectedDoc.meta.id);
      setRevisionsList(revs);
      setRefineDiffs(null);
      setRefineInstruction("");
      setRefineFiles([]);
      setStatusMessage("선택한 섹션이 성공적으로 반영되었습니다.");
    } catch (e: any) {
      setStatusMessage(`반영 실패: ${e?.message || e}`);
    } finally {
      setIsBusy(false);
    }
  };

  // Handle Tab 1 Rollback
  const handleRollback = async (rev: number) => {
    if (!selectedDoc) return;
    setIsBusy(true);
    setStatusMessage(null);
    try {
      const store = createMemoryStore();
      const rolled = await store.rollbackToRevision(selectedDoc.meta.id, rev);
      await refreshMemoriesIndex();
      setSelectedDoc(rolled);
      setEditName(rolled.meta.name);
      setEditDesc(rolled.meta.description || "");
      setEditedSections(parseMemorySections(rolled.body));
      const revs = await store.listRevisions(selectedDoc.meta.id);
      setRevisionsList(revs);
      setPreviewRevisionDoc(null);
      setStatusMessage(`리비전 ${rev}으로 롤백 완료되었습니다 (새 리비전으로 기록됨).`);
    } catch (e: any) {
      setStatusMessage(`롤백 실패: ${e?.message || e}`);
    } finally {
      setIsBusy(false);
    }
  };

  // Handle Tab 1 Duplicate
  const handleDuplicate = async () => {
    if (!selectedDoc) return;
    setIsBusy(true);
    setStatusMessage(null);
    try {
      const store = createMemoryStore();
      const newId = "mem_" + crypto.randomUUID();
      const duplicated: MemoryDoc = {
        meta: {
          ...selectedDoc.meta,
          id: newId,
          name: `${selectedDoc.meta.name} (복제본)`,
          revision: 1,
          createdAt: new Date().toISOString(),
          updatedAt: new Date().toISOString(),
        },
        body: selectedDoc.body,
      };
      await store.save(duplicated, { recordRevision: false });
      await refreshMemoriesIndex();
      setSelectedId(newId);
      setStatusMessage("작성 선호사항이 복제되었습니다.");
    } catch (e: any) {
      setStatusMessage(`복제 실패: ${e?.message || e}`);
    } finally {
      setIsBusy(false);
    }
  };

  // Handle Tab 1 Export
  const handleExport = async () => {
    if (!selectedDoc) return;
    try {
      const store = createMemoryStore();
      const rawMd = await store.exportMarkdown(selectedDoc.meta.id);
      const blob = new Blob([rawMd], { type: "text/markdown;charset=utf-8" });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = `${selectedDoc.meta.name}.md`;
      a.click();
      URL.revokeObjectURL(url);
    } catch (e: any) {
      setStatusMessage(`내보내기 실패: ${e?.message || e}`);
    }
  };

  // Handle Tab 1 Delete
  const handleDelete = async () => {
    if (!selectedDoc) return;
    setIsBusy(true);
    setStatusMessage(null);
    try {
      const store = createMemoryStore();
      await store.delete(selectedDoc.meta.id);
      await refreshMemoriesIndex();
      setSelectedId(null);
      setSelectedDoc(null);
      setIsDeleting(false);
      setStatusMessage("작성 선호사항이 삭제되었습니다.");
    } catch (e: any) {
      setStatusMessage(`삭제 실패: ${e?.message || e}`);
    } finally {
      setIsBusy(false);
    }
  };

  // Handle Tab 1 Import Markdown
  const handleImportMarkdown = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;
    setIsBusy(true);
    setStatusMessage(null);
    try {
      const rawMd = await file.text();
      const store = createMemoryStore();
      const { doc, warnings } = await store.importMarkdown(rawMd);
      await refreshMemoriesIndex();
      setSelectedId(doc.meta.id);
      if (warnings.length > 0) {
        setWarningsList(warnings);
      }
      setStatusMessage(`"${doc.meta.name}" 마크다운을 가져왔습니다.`);
    } catch (err: any) {
      setStatusMessage(`가져오기 실패: ${err?.message || err}`);
    } finally {
      setIsBusy(false);
      if (importFileInputRef.current) importFileInputRef.current.value = "";
    }
  };

  // Handle Tab 2 Run Generate
  const handleRunCreateMulti = async () => {
    if (!createMultiName.trim()) {
      setStatusMessage("작성 선호사항 이름을 입력해 주세요.");
      return;
    }
    if (createMultiFiles.length === 0) {
      setStatusMessage("학습할 문서를 1개 이상 선택해 주세요.");
      return;
    }
    setIsBusy(true);
    setStatusMessage(null);
    setWarningsList([]);
    try {
      const docs: GenerateMemoryInputDoc[] = [];
      for (const file of createMultiFiles.slice(0, 10)) {
        const text = await extractTextFromFile(file);
        if (text.trim()) {
          docs.push({ name: file.name, text });
        }
      }
      if (docs.length === 0) {
        throw new Error("분석할 유효 텍스트가 추출되지 않았습니다.");
      }

      const res = await generateMemory({
        name: createMultiName.trim(),
        description: createMultiDesc.trim(),
        docs,
        provider: getProviderConfig(),
      });

      setPreviewResult(res);
      setPreviewBody(res.doc.body);
      setPreviewOriginTab("create_multi");
      if (res.warnings.length > 0) {
        setWarningsList(res.warnings);
      }
    } catch (err: any) {
      setStatusMessage(`생성 실패: ${err?.message || err}`);
    } finally {
      setIsBusy(false);
    }
  };

  // Handle Tab 3 Run Current Doc Generate
  const handleRunCreateCurrent = async () => {
    if (!createCurrentName.trim()) {
      setStatusMessage("작성 선호사항 이름을 입력해 주세요.");
      return;
    }
    if (!docMarkdown || !docMarkdown.trim()) {
      setStatusMessage("현재 열린 문서에 내용이 없습니다.");
      return;
    }
    setIsBusy(true);
    setStatusMessage(null);
    setWarningsList([]);
    try {
      const res = await generateMemory({
        name: createCurrentName.trim(),
        description: createCurrentDesc.trim(),
        docs: [{ name: fileName || "현재 문서", text: docMarkdown }],
        provider: getProviderConfig(),
      });

      setPreviewResult(res);
      setPreviewBody(res.doc.body);
      setPreviewOriginTab("create_current");
      if (res.warnings.length > 0) {
        setWarningsList(res.warnings);
      }
    } catch (err: any) {
      setStatusMessage(`생성 실패: ${err?.message || err}`);
    } finally {
      setIsBusy(false);
    }
  };

  // Confirm and Save from Preview Panel (for Tab 2 & Tab 3)
  const handleConfirmSavePreview = async () => {
    if (!previewResult) return;
    setIsBusy(true);
    setStatusMessage(null);
    try {
      const store = createMemoryStore();
      const sanitizedBody = scanAndRedactPii(normalizeMemoryBody(previewBody)).text;
      const finalDoc: MemoryDoc = {
        ...previewResult.doc,
        body: sanitizedBody,
      };

      await store.save(finalDoc, { recordRevision: false });
      await refreshMemoriesIndex();
      setSelectedId(finalDoc.meta.id);
      setActiveTab("list");
      setPreviewResult(null);
      setPreviewBody("");
      if (previewOriginTab === "create_multi") {
        setCreateMultiName("");
        setCreateMultiDesc("");
        setCreateMultiFiles([]);
      }
      setStatusMessage(`"${finalDoc.meta.name}" 작성 선호사항이 저장되었습니다.`);
    } catch (err: any) {
      setStatusMessage(`저장 확정 실패: ${err?.message || err}`);
    } finally {
      setIsBusy(false);
    }
  };

  const handleCancelPreview = () => {
    setPreviewResult(null);
    setPreviewBody("");
  };

  return (
    <div
      id="writing-memory-modal"
      data-testid="writing-memory-modal"
      className="fixed inset-0 z-50 bg-slate-900/50 flex items-center justify-center p-4 backdrop-blur-xs"
    >
      <div className="bg-white rounded-2xl shadow-2xl border border-slate-200 w-full max-w-5xl h-[88vh] flex flex-col overflow-hidden animate-in fade-in zoom-in-95 duration-150">
        {/* Top Header */}
        <div className="px-6 py-4 border-b border-slate-200 flex items-center justify-between bg-slate-50/80">
          <div className="flex items-center gap-2.5">
            <div className="p-2 rounded-xl bg-teal-600 text-white shadow-xs">
              <BookMarked className="w-5 h-5" />
            </div>
            <div>
              <h2 className="text-base font-bold text-slate-800 flex items-center gap-2">
                <span>작성 선호사항 (Writing Memory)</span>
                <span className="text-[11px] font-normal px-2 py-0.5 rounded-full bg-teal-100 text-teal-800">
                  단방향 영구 비식별화
                </span>
              </h2>
              <p className="text-xs text-slate-500">
                자주 사용하는 보고서·공문서 양식 및 문체 규칙을 추출하고 보관합니다.
              </p>
            </div>
          </div>

          <button
            onClick={closeMemoryModal}
            className="p-2 text-slate-400 hover:text-slate-600 hover:bg-slate-200/60 rounded-xl transition-colors"
            title="닫기"
          >
            <X className="w-5 h-5" />
          </button>
        </div>

        {/* Tab Navigation */}
        <div className="px-6 border-b border-slate-200 flex items-center gap-2 bg-white">
          <button
            type="button"
            id="memory-tab-list"
            onClick={() => {
              setActiveTab("list");
              setPreviewResult(null);
            }}
            className={`flex items-center gap-2 py-3 px-3 text-xs font-semibold border-b-2 transition-colors ${
              activeTab === "list"
                ? "border-teal-600 text-teal-700"
                : "border-transparent text-slate-500 hover:text-slate-800"
            }`}
          >
            <BookMarked className="w-4 h-4" />
            <span>메모리 관리 ({memoriesIndex.length})</span>
          </button>

          <button
            type="button"
            id="memory-tab-create-multi"
            onClick={() => {
              setActiveTab("create_multi");
              setPreviewResult(null);
            }}
            className={`flex items-center gap-2 py-3 px-3 text-xs font-semibold border-b-2 transition-colors ${
              activeTab === "create_multi"
                ? "border-teal-600 text-teal-700"
                : "border-transparent text-slate-500 hover:text-slate-800"
            }`}
          >
            <Files className="w-4 h-4" />
            <span>여러 문서로 새로 만들기</span>
          </button>

          <button
            type="button"
            id="memory-tab-create-current"
            onClick={() => {
              setActiveTab("create_current");
              setPreviewResult(null);
            }}
            className={`flex items-center gap-2 py-3 px-3 text-xs font-semibold border-b-2 transition-colors ${
              activeTab === "create_current"
                ? "border-teal-600 text-teal-700"
                : "border-transparent text-slate-500 hover:text-slate-800"
            }`}
          >
            <FileText className="w-4 h-4" />
            <span>현재 열린 문서로 만들기</span>
          </button>
        </div>

        {/* Status / Warnings Banner */}
        {statusMessage && (
          <div className="px-6 py-2 bg-teal-50 border-b border-teal-200 text-teal-900 text-xs flex items-center justify-between">
            <span>{statusMessage}</span>
            <button
              onClick={() => setStatusMessage(null)}
              className="text-teal-600 hover:text-teal-800 font-bold ml-2"
            >
              ✕
            </button>
          </div>
        )}

        {/* Modal Main Content */}
        <div className="flex-1 overflow-hidden flex flex-col bg-slate-50/50">
          {/* TAB 1: 메모리 관리 */}
          {activeTab === "list" && (
            <div className="flex-1 flex overflow-hidden">
              {/* Left Column: Memories List */}
              <div className="w-72 border-r border-slate-200 bg-white flex flex-col shrink-0">
                <div className="p-3 border-b border-slate-100 flex items-center justify-between">
                  <span className="text-xs font-bold text-slate-700">목록</span>
                  <div className="flex items-center gap-1">
                    <button
                      type="button"
                      onClick={() => importFileInputRef.current?.click()}
                      className="flex items-center gap-1 text-[11px] font-semibold text-teal-700 hover:bg-teal-50 px-2 py-1 rounded border border-teal-200 transition-colors"
                      title="마크다운(.md) 스타일 가이드 파일 가져오기"
                    >
                      <Upload className="w-3 h-3" />
                      <span>.md 가져오기</span>
                    </button>
                    <input
                      ref={importFileInputRef}
                      id="memory-import-md-input"
                      data-testid="memory-import-md-input"
                      type="file"
                      accept=".md,.markdown,text/markdown"
                      className="hidden"
                      onChange={handleImportMarkdown}
                    />
                  </div>
                </div>

                <div className="flex-1 overflow-y-auto p-2 space-y-1">
                  {memoriesIndex.length === 0 ? (
                    <div className="p-6 text-center text-slate-400 text-xs">
                      <BookMarked className="w-8 h-8 mx-auto mb-2 text-slate-300 stroke-1" />
                      저장된 메모리가 없습니다.
                      <div className="mt-2">
                        <button
                          type="button"
                          onClick={() => setActiveTab("create_multi")}
                          className="text-teal-600 hover:underline font-semibold"
                        >
                          새로 만들기
                        </button>
                      </div>
                    </div>
                  ) : (
                    memoriesIndex.map((m) => {
                      const isSelected = selectedId === m.id;
                      return (
                        <div
                          key={m.id}
                          data-testid={`memory-item-${m.id}`}
                          onClick={() => setSelectedId(m.id)}
                          className={`p-2.5 rounded-xl cursor-pointer text-left border transition-all ${
                            isSelected
                              ? "bg-teal-50/80 border-teal-300 shadow-xs"
                              : "bg-white hover:bg-slate-50 border-slate-150"
                          }`}
                        >
                          <div className="flex items-center justify-between gap-1">
                            <span className="text-xs font-bold text-slate-800 truncate">
                              {m.name}
                            </span>
                            <span className="text-[10px] font-medium px-1.5 py-0.2 rounded bg-slate-100 text-slate-600 shrink-0">
                              v{m.revision}
                            </span>
                          </div>
                          {m.description && (
                            <p className="text-[11px] text-slate-500 truncate mt-0.5">
                              {m.description}
                            </p>
                          )}
                          <div className="flex items-center justify-between text-[10px] text-slate-400 mt-1">
                            <span>문서 {m.sourceDocs?.length ?? 0}개 기반</span>
                            <span>{new Date(m.updatedAt).toLocaleDateString()}</span>
                          </div>
                        </div>
                      );
                    })
                  )}
                </div>
              </div>

              {/* Right Column: Detail View */}
              <div className="flex-1 flex flex-col bg-slate-50/40 overflow-hidden">
                {!selectedDoc ? (
                  <div className="flex-1 flex flex-col items-center justify-center p-8 text-center text-slate-400">
                    <BookMarked className="w-12 h-12 stroke-1 text-slate-300 mb-2" />
                    <p className="text-xs">좌측 목록에서 편집할 메모리를 선택하세요.</p>
                  </div>
                ) : (
                  <div className="flex-1 flex flex-col overflow-hidden">
                    {/* Top Meta Bar */}
                    <div className="p-4 bg-white border-b border-slate-200 flex flex-col gap-3">
                      <div className="flex items-center justify-between gap-3">
                        <div className="flex-1 flex items-center gap-2">
                          <input
                            id="memory-edit-name-input"
                            type="text"
                            value={editName}
                            onChange={(e) => setEditName(e.target.value)}
                            placeholder="메모리 이름"
                            className="flex-1 text-sm font-bold text-slate-800 border border-slate-200 px-3 py-1.5 rounded-lg focus:outline-none focus:ring-1 focus:ring-teal-500"
                          />
                          <span className="text-xs font-medium px-2 py-1 rounded bg-slate-100 text-slate-600 shrink-0">
                            리비전 {selectedDoc.meta.revision}
                          </span>
                        </div>

                        {/* Top Actions: Duplicate, Export, Delete */}
                        <div className="flex items-center gap-1.5">
                          <button
                            type="button"
                            id="memory-duplicate-btn"
                            onClick={handleDuplicate}
                            disabled={isBusy}
                            className="flex items-center gap-1 text-xs px-2.5 py-1.5 rounded-lg border border-slate-200 text-slate-700 hover:bg-slate-50 transition-colors"
                            title="복제하여 새 메모리로 생성"
                          >
                            <Copy className="w-3.5 h-3.5" />
                            <span>복제</span>
                          </button>

                          <button
                            type="button"
                            id="memory-export-btn"
                            onClick={handleExport}
                            disabled={isBusy}
                            className="flex items-center gap-1 text-xs px-2.5 py-1.5 rounded-lg border border-slate-200 text-slate-700 hover:bg-slate-50 transition-colors"
                            title="마크다운(.md) 파일로 다운로드"
                          >
                            <Download className="w-3.5 h-3.5" />
                            <span>.md 내보내기</span>
                          </button>

                          {!isDeleting ? (
                            <button
                              type="button"
                              id="memory-delete-btn"
                              onClick={() => setIsDeleting(true)}
                              disabled={isBusy}
                              className="flex items-center gap-1 text-xs px-2.5 py-1.5 rounded-lg border border-rose-200 text-rose-700 hover:bg-rose-50 transition-colors"
                              title="메모리 영구 삭제"
                            >
                              <Trash2 className="w-3.5 h-3.5" />
                              <span>삭제</span>
                            </button>
                          ) : (
                            <div className="flex items-center gap-1 bg-rose-50 border border-rose-200 rounded-lg p-1">
                              <span className="text-[11px] text-rose-800 font-semibold px-1">
                                삭제할까요?
                              </span>
                              <button
                                type="button"
                                id="memory-confirm-delete-btn"
                                onClick={handleDelete}
                                disabled={isBusy}
                                className="text-xs bg-rose-600 text-white font-bold px-2 py-0.5 rounded hover:bg-rose-700"
                              >
                                정말 삭제
                              </button>
                              <button
                                type="button"
                                onClick={() => setIsDeleting(false)}
                                className="text-xs text-slate-600 px-1 hover:underline"
                              >
                                취소
                              </button>
                            </div>
                          )}
                        </div>
                      </div>

                      <input
                        id="memory-edit-desc-input"
                        type="text"
                        value={editDesc}
                        onChange={(e) => setEditDesc(e.target.value)}
                        placeholder="설명 (선택사항, 예: 기획재정부 보고서 표준 양식)"
                        className="text-xs text-slate-600 border border-slate-200 px-3 py-1.5 rounded-lg focus:outline-none focus:ring-1 focus:ring-teal-500"
                      />

                      {/* Subview Nav Tabs */}
                      <div className="flex items-center gap-2 pt-1 border-t border-slate-100">
                        <button
                          type="button"
                          id="memory-subview-edit-btn"
                          onClick={() => {
                            setActiveSubview("edit");
                            setRefineDiffs(null);
                          }}
                          className={`flex items-center gap-1.5 text-xs px-3 py-1.5 rounded-lg font-semibold transition-colors ${
                            activeSubview === "edit"
                              ? "bg-teal-600 text-white"
                              : "bg-slate-100 text-slate-600 hover:bg-slate-200"
                          }`}
                        >
                          <Save className="w-3.5 h-3.5" />
                          <span>직접 편집</span>
                        </button>

                        <button
                          type="button"
                          id="memory-subview-refine-btn"
                          onClick={() => setActiveSubview("refine")}
                          className={`flex items-center gap-1.5 text-xs px-3 py-1.5 rounded-lg font-semibold transition-colors ${
                            activeSubview === "refine"
                              ? "bg-teal-600 text-white"
                              : "bg-slate-100 text-slate-600 hover:bg-slate-200"
                          }`}
                        >
                          <Wand2 className="w-3.5 h-3.5" />
                          <span>AI로 다듬기 / 추가 학습</span>
                        </button>

                        <button
                          type="button"
                          id="memory-subview-revisions-btn"
                          onClick={() => {
                            setActiveSubview("revisions");
                            setRefineDiffs(null);
                          }}
                          className={`flex items-center gap-1.5 text-xs px-3 py-1.5 rounded-lg font-semibold transition-colors ${
                            activeSubview === "revisions"
                              ? "bg-teal-600 text-white"
                              : "bg-slate-100 text-slate-600 hover:bg-slate-200"
                          }`}
                        >
                          <History className="w-3.5 h-3.5" />
                          <span>리비전 기록 ({revisionsList.length})</span>
                        </button>
                      </div>
                    </div>

                    {/* Subview Body Container */}
                    <div className="flex-1 overflow-y-auto p-4">
                      {/* Subview A: 직접 편집 */}
                      {activeSubview === "edit" && (
                        <div className="space-y-4">
                          <div className="flex items-center justify-between">
                            <span className="text-xs text-slate-500">
                              7개 고정 섹션을 직접 수정할 수 있습니다 (저장 시 개인정보 패턴 자동 검사 및 새 리비전 생성).
                            </span>
                            <button
                              type="button"
                              id="memory-save-direct-btn"
                              onClick={handleSaveDirect}
                              disabled={isBusy}
                              className="flex items-center gap-1.5 text-xs font-semibold px-4 py-2 rounded-lg bg-teal-600 hover:bg-teal-700 text-white shadow-xs transition-colors disabled:opacity-50"
                            >
                              {isBusy ? <Loader2 className="w-4 h-4 animate-spin" /> : <Save className="w-4 h-4" />}
                              <span>변경사항 저장 (v{selectedDoc.meta.revision + 1})</span>
                            </button>
                          </div>

                          <div className="space-y-3">
                            {FIXED_MEMORY_SECTIONS.map((sectionTitle, idx) => (
                              <div
                                key={sectionTitle}
                                className="bg-white p-3.5 rounded-xl border border-slate-200 shadow-2xs space-y-1.5"
                              >
                                <label className="text-xs font-bold text-slate-800 flex items-center gap-1.5">
                                  <span className="w-4 h-4 rounded-full bg-teal-100 text-teal-800 flex items-center justify-center text-[10px]">
                                    {idx + 1}
                                  </span>
                                  <span>{sectionTitle}</span>
                                </label>
                                <textarea
                                  data-testid={`memory-section-textarea-${idx}`}
                                  value={editedSections[sectionTitle] || ""}
                                  onChange={(e) =>
                                    setEditedSections((prev) => ({
                                      ...prev,
                                      [sectionTitle]: e.target.value,
                                    }))
                                  }
                                  rows={3}
                                  className="w-full text-xs font-mono text-slate-700 border border-slate-200 rounded-lg p-2.5 focus:outline-none focus:ring-1 focus:ring-teal-500 resize-y"
                                  placeholder={`- (${sectionTitle} 규칙 없음)`}
                                />
                              </div>
                            ))}
                          </div>
                        </div>
                      )}

                      {/* Subview B: AI로 다듬기 */}
                      {activeSubview === "refine" && (
                        <div className="h-full flex flex-col space-y-4">
                          {!refineDiffs ? (
                            <div className="bg-white p-5 rounded-xl border border-slate-200 shadow-2xs space-y-4">
                              <div>
                                <h3 className="text-xs font-bold text-slate-800 mb-1">
                                  AI 다듬기 지시사항
                                </h3>
                                <input
                                  type="text"
                                  id="memory-refine-instruction-input"
                                  value={refineInstruction}
                                  onChange={(e) => setRefineInstruction(e.target.value)}
                                  placeholder="예: 어투를 더 정중한 격식체로 다듬고, 표 제목 서식 규칙을 강화해줘"
                                  className="w-full text-xs border border-slate-200 rounded-lg px-3 py-2 focus:outline-none focus:ring-1 focus:ring-teal-500"
                                />
                              </div>

                              <div>
                                <h3 className="text-xs font-bold text-slate-800 mb-1">
                                  추가 학습 문서 (선택사항, 최대 10개)
                                </h3>
                                <div className="flex items-center gap-2">
                                  <input
                                    id="memory-refine-files-input"
                                    ref={refineFileInputRef}
                                    type="file"
                                    multiple
                                    accept=".hwp,.hwpx,.docx,.xlsx,.xls,.csv,.pdf,.md,.txt"
                                    onChange={(e) => {
                                      const files = Array.from(e.target.files || []);
                                      setRefineFiles((prev) => [...prev, ...files].slice(0, 10));
                                    }}
                                    className="text-xs file:mr-2 file:py-1 file:px-2.5 file:rounded-lg file:border-0 file:text-xs file:font-semibold file:bg-teal-50 file:text-teal-700 hover:file:bg-teal-100 cursor-pointer"
                                  />
                                </div>

                                {refineFiles.length > 0 && (
                                  <div className="flex flex-wrap gap-1.5 mt-2">
                                    {refineFiles.map((f, i) => (
                                      <span
                                        key={i}
                                        className="inline-flex items-center gap-1 px-2 py-0.5 rounded-md bg-slate-100 text-slate-700 text-[11px]"
                                      >
                                        <FileText className="w-3 h-3 text-slate-500" />
                                        <span>{f.name}</span>
                                        <button
                                          type="button"
                                          onClick={() =>
                                            setRefineFiles((prev) => prev.filter((_, idx) => idx !== i))
                                          }
                                          className="text-slate-400 hover:text-rose-600 font-bold ml-1"
                                        >
                                          ✕
                                        </button>
                                      </span>
                                    ))}
                                  </div>
                                )}
                              </div>

                              <div className="pt-2">
                                <button
                                  type="button"
                                  id="memory-propose-refine-btn"
                                  onClick={handleProposeRefine}
                                  disabled={
                                    isBusy ||
                                    (!refineInstruction.trim() && refineFiles.length === 0)
                                  }
                                  className="flex items-center gap-1.5 text-xs font-semibold px-4 py-2 rounded-lg bg-teal-600 hover:bg-teal-700 text-white shadow-xs transition-colors disabled:opacity-50"
                                >
                                  {isBusy ? (
                                    <Loader2 className="w-4 h-4 animate-spin" />
                                  ) : (
                                    <Wand2 className="w-4 h-4" />
                                  )}
                                  <span>수정안 생성 (Smart Diff 미리보기)</span>
                                </button>
                              </div>
                            </div>
                          ) : (
                            <MemoryDiffViewer
                              diffs={refineDiffs}
                              acceptedTitles={acceptedRefineTitles}
                              onToggleSection={(title) => {
                                setAcceptedRefineTitles((prev) =>
                                  prev.includes(title)
                                    ? prev.filter((t) => t !== title)
                                    : [...prev, title]
                                );
                              }}
                              onAcceptSelected={handleApplyRefine}
                              onCancel={() => setRefineDiffs(null)}
                              busy={isBusy}
                            />
                          )}
                        </div>
                      )}

                      {/* Subview C: 리비전 기록 */}
                      {activeSubview === "revisions" && (
                        <div className="space-y-4">
                          <p className="text-xs text-slate-500">
                            최근 최대 5개의 수정 이력이 보관됩니다. 이전 버전으로 롤백하면 새 리비전으로 복원됩니다.
                          </p>

                          {revisionsList.length === 0 ? (
                            <div className="p-8 text-center text-slate-400 text-xs bg-white rounded-xl border border-slate-200">
                              이전 리비전 기록이 없습니다. (현재 버전이 최초 생성 버전입니다)
                            </div>
                          ) : (
                            <div className="space-y-3">
                              {revisionsList.map((rev) => (
                                <div
                                  key={rev}
                                  className="bg-white p-3.5 rounded-xl border border-slate-200 shadow-2xs flex flex-col gap-2"
                                >
                                  <div className="flex items-center justify-between">
                                    <div className="flex items-center gap-2">
                                      <span className="text-xs font-bold text-slate-800">
                                        리비전 {rev}
                                      </span>
                                      <span className="text-[10px] text-slate-400">
                                        (현재: v{selectedDoc.meta.revision})
                                      </span>
                                    </div>

                                    <div className="flex items-center gap-2">
                                      <button
                                        type="button"
                                        onClick={async () => {
                                          if (previewRevisionDoc?.rev === rev) {
                                            setPreviewRevisionDoc(null);
                                          } else {
                                            const store = createMemoryStore();
                                            const doc = await store.getRevision(
                                              selectedDoc.meta.id,
                                              rev
                                            );
                                            if (doc) setPreviewRevisionDoc({ rev, doc });
                                          }
                                        }}
                                        className="flex items-center gap-1 text-xs px-2.5 py-1 rounded border border-slate-200 text-slate-600 hover:bg-slate-50"
                                      >
                                        <Eye className="w-3.5 h-3.5" />
                                        <span>
                                          {previewRevisionDoc?.rev === rev ? "미리보기 닫기" : "미리보기"}
                                        </span>
                                      </button>

                                      <button
                                        type="button"
                                        data-testid={`memory-rollback-rev-${rev}`}
                                        onClick={() => handleRollback(rev)}
                                        disabled={isBusy}
                                        className="flex items-center gap-1 text-xs px-2.5 py-1 rounded bg-amber-500 hover:bg-amber-600 text-white font-medium shadow-2xs transition-colors"
                                      >
                                        <RotateCcw className="w-3.5 h-3.5" />
                                        <span>이 버전으로 롤백</span>
                                      </button>
                                    </div>
                                  </div>

                                  {previewRevisionDoc?.rev === rev && (
                                    <div className="p-3 bg-slate-50 rounded-lg text-xs font-mono text-slate-700 whitespace-pre-wrap max-h-60 overflow-y-auto border border-slate-200">
                                      {previewRevisionDoc.doc.body}
                                    </div>
                                  )}
                                </div>
                              ))}
                            </div>
                          )}
                        </div>
                      )}
                    </div>
                  </div>
                )}
              </div>
            </div>
          )}

          {/* TAB 2: 여러 문서로 새로 만들기 */}
          {activeTab === "create_multi" && (
            <div className="flex-1 p-6 overflow-y-auto">
              {!previewResult ? (
                <div className="max-w-2xl mx-auto bg-white p-6 rounded-2xl border border-slate-200 shadow-sm space-y-5">
                  <div>
                    <h3 className="text-sm font-bold text-slate-800 mb-1 flex items-center gap-2">
                      <Files className="w-4 h-4 text-teal-600" />
                      <span>여러 문서에서 작성 스타일 추출</span>
                    </h3>
                    <p className="text-xs text-slate-500">
                      공문서나 보고서 예시(최대 10개)를 첨부하면 공통 양식과 문체 규칙을 추출합니다.
                    </p>
                  </div>

                  <div className="space-y-3">
                    <div>
                      <label className="text-xs font-bold text-slate-700 block mb-1">
                        메모리 이름 <span className="text-rose-500">*</span>
                      </label>
                      <input
                        id="memory-create-name-input"
                        type="text"
                        value={createMultiName}
                        onChange={(e) => setCreateMultiName(e.target.value)}
                        placeholder="예: 내부 결재 기안 양식"
                        className="w-full text-xs border border-slate-200 rounded-lg px-3 py-2 focus:outline-none focus:ring-1 focus:ring-teal-500"
                      />
                    </div>

                    <div>
                      <label className="text-xs font-bold text-slate-700 block mb-1">
                        설명 (선택)
                      </label>
                      <input
                        id="memory-create-desc-input"
                        type="text"
                        value={createMultiDesc}
                        onChange={(e) => setCreateMultiDesc(e.target.value)}
                        placeholder="예: 부서 공통 내부 보고서 서식 가이드"
                        className="w-full text-xs border border-slate-200 rounded-lg px-3 py-2 focus:outline-none focus:ring-1 focus:ring-teal-500"
                      />
                    </div>

                    <div>
                      <label className="text-xs font-bold text-slate-700 block mb-1">
                        학습할 문서 파일 (최대 10개, HWP, HWPX, DOCX, XLSX, PDF, MD, TXT)
                      </label>
                      <input
                        id="memory-create-files-input"
                        data-testid="memory-create-files-input"
                        ref={createMultiFileInputRef}
                        type="file"
                        multiple
                        accept=".hwp,.hwpx,.docx,.xlsx,.xls,.csv,.pdf,.md,.txt"
                        onChange={(e) => {
                          const files = Array.from(e.target.files || []);
                          setCreateMultiFiles((prev) => [...prev, ...files].slice(0, 10));
                        }}
                        className="text-xs file:mr-2 file:py-1.5 file:px-3 file:rounded-lg file:border-0 file:text-xs file:font-semibold file:bg-teal-50 file:text-teal-700 hover:file:bg-teal-100 cursor-pointer"
                      />

                      {createMultiFiles.length > 0 && (
                        <div className="flex flex-wrap gap-2 mt-3">
                          {createMultiFiles.map((file, i) => (
                            <span
                              key={i}
                              className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-lg bg-slate-100 border border-slate-200 text-slate-700 text-xs"
                            >
                              <FileText className="w-3.5 h-3.5 text-slate-500" />
                              <span>{file.name}</span>
                              <button
                                type="button"
                                onClick={() =>
                                  setCreateMultiFiles((prev) => prev.filter((_, idx) => idx !== i))
                                }
                                className="text-slate-400 hover:text-rose-600 font-bold ml-1"
                              >
                                ✕
                              </button>
                            </span>
                          ))}
                        </div>
                      )}
                    </div>
                  </div>

                  <div className="pt-2 flex justify-end">
                    <button
                      type="button"
                      id="memory-run-generate-btn"
                      onClick={handleRunCreateMulti}
                      disabled={isBusy || !createMultiName.trim() || createMultiFiles.length === 0}
                      className="flex items-center gap-1.5 text-xs font-semibold px-5 py-2.5 rounded-xl bg-teal-600 hover:bg-teal-700 text-white shadow-sm transition-colors disabled:opacity-50"
                    >
                      {isBusy ? <Loader2 className="w-4 h-4 animate-spin" /> : <Wand2 className="w-4 h-4" />}
                      <span>선호사항 초안 생성</span>
                    </button>
                  </div>
                </div>
              ) : (
                <div id="memory-preview-panel" className="max-w-4xl mx-auto space-y-4">
                  {/* PII Banner */}
                  <div
                    id="memory-preview-pii-banner"
                    className="p-4 rounded-xl bg-emerald-50 border border-emerald-200 text-emerald-900 text-xs space-y-1.5"
                  >
                    <div className="font-bold flex items-center gap-1.5">
                      <CheckCircle2 className="w-4 h-4 text-emerald-700" />
                      <span>
                        자동 탐지 {previewResult.inputRedactionCount + previewResult.outputRedactionCount}건(입력 {previewResult.inputRedactionCount}건, 출력 {previewResult.outputRedactionCount}건) 비식별화 치환 완료 — 저장 전 남은 개인정보가 없는지 확인하세요.
                      </span>
                    </div>
                    {warningsList.length > 0 && (
                      <ul className="list-disc pl-5 text-[11px] text-emerald-800 space-y-0.5">
                        {warningsList.map((w, i) => (
                          <li key={i}>{w}</li>
                        ))}
                      </ul>
                    )}
                  </div>

                  {/* Body Textarea */}
                  <div className="bg-white p-4 rounded-xl border border-slate-200 shadow-xs space-y-2">
                    <label className="text-xs font-bold text-slate-800">
                      생성된 작성 선호사항 본문 미리보기 및 수정
                    </label>
                    <textarea
                      id="memory-preview-body-textarea"
                      value={previewBody}
                      onChange={(e) => setPreviewBody(e.target.value)}
                      rows={16}
                      className="w-full text-xs font-mono text-slate-800 border border-slate-200 rounded-lg p-3 focus:outline-none focus:ring-1 focus:ring-teal-500"
                    />
                  </div>

                  {/* Preview Footer Actions */}
                  <div className="flex items-center justify-between pt-2">
                    <button
                      type="button"
                      id="memory-cancel-preview-btn"
                      onClick={handleCancelPreview}
                      disabled={isBusy}
                      className="text-xs px-4 py-2 rounded-lg border border-slate-300 text-slate-700 hover:bg-slate-100"
                    >
                      취소하고 다시 설정
                    </button>

                    <button
                      type="button"
                      id="memory-confirm-save-btn"
                      onClick={handleConfirmSavePreview}
                      disabled={isBusy}
                      className="flex items-center gap-1.5 text-xs font-semibold px-5 py-2.5 rounded-xl bg-teal-600 hover:bg-teal-700 text-white shadow-sm transition-colors disabled:opacity-50"
                    >
                      {isBusy ? <Loader2 className="w-4 h-4 animate-spin" /> : <Save className="w-4 h-4" />}
                      <span>확인 및 저장</span>
                    </button>
                  </div>
                </div>
              )}
            </div>
          )}

          {/* TAB 3: 현재 열린 문서로 만들기 */}
          {activeTab === "create_current" && (
            <div className="flex-1 p-6 overflow-y-auto">
              {!previewResult ? (
                <div className="max-w-2xl mx-auto bg-white p-6 rounded-2xl border border-slate-200 shadow-sm space-y-5">
                  <div>
                    <h3 className="text-sm font-bold text-slate-800 mb-1 flex items-center gap-2">
                      <FilePlus2 className="w-4 h-4 text-teal-600" />
                      <span>현재 열린 문서에서 양식 추출</span>
                    </h3>
                    <p className="text-xs text-slate-500">
                      현재 편집기에서 열려 있는 문서의 문체와 서식을 분석하여 새 메모리로 생성합니다.
                    </p>
                  </div>

                  <div className="p-3.5 rounded-xl bg-slate-50 border border-slate-200 flex items-center justify-between">
                    <div>
                      <span className="text-xs font-bold text-slate-800 block">
                        {fileName || "문서 없음"}
                      </span>
                      <span className="text-[11px] text-slate-500">
                        본문 길이: {(docMarkdown || "").length.toLocaleString()}자
                      </span>
                    </div>
                    {(!docMarkdown || !docMarkdown.trim()) && (
                      <span className="text-xs font-medium text-rose-600 bg-rose-50 px-2 py-1 rounded border border-rose-200">
                        문서 내용이 비어 있습니다
                      </span>
                    )}
                  </div>

                  <div className="space-y-3">
                    <div>
                      <label className="text-xs font-bold text-slate-700 block mb-1">
                        메모리 이름 <span className="text-rose-500">*</span>
                      </label>
                      <input
                        id="memory-current-name-input"
                        type="text"
                        value={createCurrentName}
                        onChange={(e) => setCreateCurrentName(e.target.value)}
                        placeholder="예: 표준 기안서 양식"
                        className="w-full text-xs border border-slate-200 rounded-lg px-3 py-2 focus:outline-none focus:ring-1 focus:ring-teal-500"
                      />
                    </div>

                    <div>
                      <label className="text-xs font-bold text-slate-700 block mb-1">
                        설명 (선택)
                      </label>
                      <input
                        id="memory-current-desc-input"
                        type="text"
                        value={createCurrentDesc}
                        onChange={(e) => setCreateCurrentDesc(e.target.value)}
                        placeholder="예: 보고서 결재 전용 서식"
                        className="w-full text-xs border border-slate-200 rounded-lg px-3 py-2 focus:outline-none focus:ring-1 focus:ring-teal-500"
                      />
                    </div>
                  </div>

                  <div className="pt-2 flex justify-end">
                    <button
                      type="button"
                      id="memory-run-current-generate-btn"
                      onClick={handleRunCreateCurrent}
                      disabled={isBusy || !docMarkdown || !docMarkdown.trim() || !createCurrentName.trim()}
                      className="flex items-center gap-1.5 text-xs font-semibold px-5 py-2.5 rounded-xl bg-teal-600 hover:bg-teal-700 text-white shadow-sm transition-colors disabled:opacity-50"
                    >
                      {isBusy ? <Loader2 className="w-4 h-4 animate-spin" /> : <Wand2 className="w-4 h-4" />}
                      <span>이 문서로 초안 생성</span>
                    </button>
                  </div>
                </div>
              ) : (
                <div id="memory-preview-panel" className="max-w-4xl mx-auto space-y-4">
                  {/* PII Banner */}
                  <div
                    id="memory-preview-pii-banner"
                    className="p-4 rounded-xl bg-emerald-50 border border-emerald-200 text-emerald-900 text-xs space-y-1.5"
                  >
                    <div className="font-bold flex items-center gap-1.5">
                      <CheckCircle2 className="w-4 h-4 text-emerald-700" />
                      <span>
                        자동 탐지 {previewResult.inputRedactionCount + previewResult.outputRedactionCount}건(입력 {previewResult.inputRedactionCount}건, 출력 {previewResult.outputRedactionCount}건) 비식별화 치환 완료 — 저장 전 남은 개인정보가 없는지 확인하세요.
                      </span>
                    </div>
                    {warningsList.length > 0 && (
                      <ul className="list-disc pl-5 text-[11px] text-emerald-800 space-y-0.5">
                        {warningsList.map((w, i) => (
                          <li key={i}>{w}</li>
                        ))}
                      </ul>
                    )}
                  </div>

                  {/* Body Textarea */}
                  <div className="bg-white p-4 rounded-xl border border-slate-200 shadow-xs space-y-2">
                    <label className="text-xs font-bold text-slate-800">
                      생성된 작성 선호사항 본문 미리보기 및 수정
                    </label>
                    <textarea
                      id="memory-preview-body-textarea"
                      value={previewBody}
                      onChange={(e) => setPreviewBody(e.target.value)}
                      rows={16}
                      className="w-full text-xs font-mono text-slate-800 border border-slate-200 rounded-lg p-3 focus:outline-none focus:ring-1 focus:ring-teal-500"
                    />
                  </div>

                  {/* Preview Footer Actions */}
                  <div className="flex items-center justify-between pt-2">
                    <button
                      type="button"
                      id="memory-cancel-preview-btn"
                      onClick={handleCancelPreview}
                      disabled={isBusy}
                      className="text-xs px-4 py-2 rounded-lg border border-slate-300 text-slate-700 hover:bg-slate-100"
                    >
                      취소하고 다시 설정
                    </button>

                    <button
                      type="button"
                      id="memory-confirm-save-btn"
                      onClick={handleConfirmSavePreview}
                      disabled={isBusy}
                      className="flex items-center gap-1.5 text-xs font-semibold px-5 py-2.5 rounded-xl bg-teal-600 hover:bg-teal-700 text-white shadow-sm transition-colors disabled:opacity-50"
                    >
                      {isBusy ? <Loader2 className="w-4 h-4 animate-spin" /> : <Save className="w-4 h-4" />}
                      <span>확인 및 저장</span>
                    </button>
                  </div>
                </div>
              )}
            </div>
          )}
        </div>
      </div>
    </div>
  );
};
