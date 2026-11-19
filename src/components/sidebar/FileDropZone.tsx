import React, { useRef, useState } from "react";
import { useDocStore } from "../../store/useDocStore";
import { assertAttachmentFileSize, parseLocalAttachment } from "../../core/multiFileParser";
import { runtimeInvoke as invoke, isTauriEnv } from "../../core/runtimeInvoke";
import { Paperclip, Check, X, Loader2, UploadCloud } from "lucide-react";

export const FileDropZone: React.FC = () => {
  const { attachments, addAttachment, removeAttachment } = useDocStore();
  const [loading, setLoading] = useState(false);
  const [loadingFileName, setLoadingFileName] = useState<string | null>(null);
  const [isDragOver, setIsDragOver] = useState(false);
  const dragCounterRef = useRef(0);

  const processFileByPath = async (filePath: string) => {
    const baseName = filePath.split(/[/\\]/).pop() || filePath;
    setLoading(true);
    setLoadingFileName(baseName);
    try {
      const bytesArr = await invoke<number[]>("read_attachment_bytes", { filePath });
      const bytes = new Uint8Array(bytesArr);
      const parsed = await parseLocalAttachment(filePath, bytes);
      addAttachment(parsed);
    } catch (e: any) {
      alert(`파일 첨부 오류: ${e?.message || e}`);
    } finally {
      setLoading(false);
      setLoadingFileName(null);
    }
  };

  const processDroppedOrPickedFiles = async (files: File[]) => {
    if (files.length === 0) return;
    setLoading(true);
    try {
      for (const file of files) {
        setLoadingFileName(file.name);
        assertAttachmentFileSize(file.size);
        const nativePath = (file as any).path as string | undefined;
        if (nativePath && ["hwp", "hwpx"].includes(nativePath.split(".").pop()?.toLowerCase() || "")) {
          const bytesArr = await invoke<number[]>("read_attachment_bytes", { filePath: nativePath });
          const bytes = new Uint8Array(bytesArr);
          const parsed = await parseLocalAttachment(nativePath, bytes);
          addAttachment(parsed);
        } else {
          const buf = await file.arrayBuffer();
          const bytes = new Uint8Array(buf);
          const parsed = await parseLocalAttachment(nativePath || file.name, bytes);
          addAttachment(parsed);
        }
      }
    } catch (err: any) {
      alert(`파일 첨부 처리 오류: ${err?.message || err}`);
    } finally {
      setLoading(false);
      setLoadingFileName(null);
    }
  };

  const handlePickFile = async () => {
    try {
      if (isTauriEnv()) {
        const picked = await invoke<string | null>("pick_local_file", {
          extensions: ["docx", "xlsx", "xls", "csv", "pdf", "hwp", "hwpx", "png", "jpg", "txt"],
        });
        if (picked) {
          await processFileByPath(picked);
        }
      } else {
        const el = document.getElementById("sidebar-attachment-file-input") as HTMLInputElement | null;
        el?.click();
      }
    } catch (e: any) {
      alert(`파일 선택 실패: ${e?.message || e}`);
    }
  };

  const getExtBadge = (fileName: string, kind: string) => {
    const ext = fileName.split(".").pop()?.toUpperCase() || kind.toUpperCase();
    return ext;
  };

  return (
    <div
      data-testid="attachment-drop-zone"
      onDragEnter={(e) => {
        e.preventDefault();
        e.stopPropagation();
        dragCounterRef.current += 1;
        setIsDragOver(true);
      }}
      onDragOver={(e) => {
        e.preventDefault();
        e.stopPropagation();
        if (e.dataTransfer) {
          e.dataTransfer.dropEffect = "copy";
        }
        if (!isDragOver) {
          setIsDragOver(true);
        }
      }}
      onDragLeave={(e) => {
        e.preventDefault();
        e.stopPropagation();
        dragCounterRef.current = Math.max(0, dragCounterRef.current - 1);
        if (dragCounterRef.current === 0) {
          setIsDragOver(false);
        }
      }}
      onDrop={async (e) => {
        e.preventDefault();
        e.stopPropagation();
        dragCounterRef.current = 0;
        setIsDragOver(false);
        const droppedFiles = Array.from(e.dataTransfer?.files || []);
        await processDroppedOrPickedFiles(droppedFiles);
      }}
      className="flex flex-col gap-2"
    >
      <input
        id="sidebar-attachment-file-input"
        data-testid="attachment-file-input"
        type="file"
        multiple
        accept=".docx,.xlsx,.xls,.csv,.pdf,.hwp,.hwpx,.png,.jpg,.jpeg,.txt,.md"
        className="hidden"
        onChange={async (e) => {
          const files = Array.from(e.target.files || []);
          await processDroppedOrPickedFiles(files);
          e.target.value = "";
        }}
      />

      {/* Attached Document Cards */}
      {attachments.length > 0 && (
        <div id="attached-files-container" className="flex flex-wrap justify-end gap-2 py-1">
          {attachments.map((att) => (
            <div
              key={att.id}
              data-testid="attachment-card"
              className="relative group w-[108px] bg-white border border-slate-200 rounded-2xl p-3 shadow-xs flex flex-col justify-between gap-2 select-none"
            >
              <button
                onClick={() => removeAttachment(att.id)}
                className="absolute -top-1.5 -right-1.5 w-4 h-4 rounded-full bg-slate-200 hover:bg-slate-300 text-slate-600 flex items-center justify-center opacity-0 group-hover:opacity-100 transition-opacity"
                title="첨부 제거"
              >
                <X className="w-2.5 h-2.5" />
              </button>

              <span className="inline-block w-fit px-2 py-0.5 rounded-full bg-slate-100 text-[10px] font-bold text-slate-600 tracking-wide">
                {getExtBadge(att.fileName, att.kind)}
              </span>

              <div className="text-xs font-medium text-slate-800 leading-snug line-clamp-2 break-all">
                {att.fileName}
              </div>

              <div className="flex justify-end">
                <Check className="w-3.5 h-3.5 text-emerald-600 stroke-[2.5]" />
              </div>
            </div>
          ))}
        </div>
      )}

      {/* Drag & Drop + File Picker Area */}
      <div
        className={`flex items-center justify-between gap-1.5 text-[11px] px-2.5 py-2 rounded-lg border border-dashed transition-all ${
          isDragOver
            ? "border-teal-600 bg-teal-50/80 ring-2 ring-teal-500/20"
            : loading
            ? "border-teal-300 bg-teal-50/40"
            : "border-slate-200 bg-slate-50/60 hover:border-slate-300"
        }`}
      >
        <button
          id="attach-file-btn"
          type="button"
          onClick={handlePickFile}
          disabled={loading}
          className="flex items-center gap-1.5 text-slate-600 hover:text-slate-900 font-medium w-full justify-center disabled:cursor-wait"
        >
          {loading ? (
            <>
              <Loader2
                data-testid="attachment-loading-spinner"
                className="w-3.5 h-3.5 animate-spin text-teal-700 shrink-0"
              />
              <span className="text-teal-800 truncate">
                {loadingFileName ? `${loadingFileName} 로드 중...` : "파일 로드 중..."}
              </span>
            </>
          ) : isDragOver ? (
            <>
              <UploadCloud className="w-3.5 h-3.5 text-teal-700 shrink-0" />
              <span className="text-teal-800 font-semibold">파일을 여기에 놓아 첨부하세요</span>
            </>
          ) : (
            <>
              <Paperclip className="w-3.5 h-3.5 text-slate-500 shrink-0" />
              <span>파일 첨부 또는 끌어다 놓기 (DOCX·XLSX·PDF·HWP)</span>
            </>
          )}
        </button>
      </div>
    </div>
  );
};
