import React, { useState } from "react";
import { useDocStore } from "../../store/useDocStore";
import { assertAttachmentFileSize, parseLocalAttachment } from "../../core/multiFileParser";
import { runtimeInvoke as invoke, isTauriEnv } from "../../core/runtimeInvoke";
import { Paperclip, Check, X, Loader2, Plus } from "lucide-react";

export const FileDropZone: React.FC = () => {
  const { attachments, addAttachment, removeAttachment } = useDocStore();
  const [loading, setLoading] = useState(false);
  const [isDragOver, setIsDragOver] = useState(false);

  const processFileByPath = async (filePath: string) => {
    setLoading(true);
    try {
      const bytesArr = await invoke<number[]>("read_attachment_bytes", { filePath });
      const bytes = new Uint8Array(bytesArr);
      const parsed = await parseLocalAttachment(filePath, bytes);
      addAttachment(parsed);
    } catch (e: any) {
      alert(`파일 첨부 오류: ${e?.message || e}`);
    } finally {
      setLoading(false);
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
      onDragOver={(e) => {
        e.preventDefault();
        setIsDragOver(true);
      }}
      onDragLeave={() => setIsDragOver(false)}
      onDrop={async (e) => {
        e.preventDefault();
        setIsDragOver(false);
        const droppedFiles = Array.from(e.dataTransfer?.files || []);
        if (droppedFiles.length === 0) return;
        setLoading(true);
        try {
          for (const file of droppedFiles) {
            assertAttachmentFileSize(file.size);
            const nativePath = (file as any).path as string | undefined;
            if (nativePath && ["hwp", "hwpx"].includes(nativePath.split(".").pop()?.toLowerCase() || "")) {
              await processFileByPath(nativePath);
            } else {
              const buf = await file.arrayBuffer();
              const bytes = new Uint8Array(buf);
              const parsed = await parseLocalAttachment(nativePath || file.name, bytes);
              addAttachment(parsed);
            }
          }
        } catch (err: any) {
          alert(`파일 드롭 처리 오류: ${err?.message || err}`);
        } finally {
          setLoading(false);
        }
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
          if (files.length === 0) return;
          setLoading(true);
          try {
            for (const file of files) {
              assertAttachmentFileSize(file.size);
              const bytes = new Uint8Array(await file.arrayBuffer());
              const parsed = await parseLocalAttachment(file.name, bytes);
              addAttachment(parsed);
            }
          } finally {
            setLoading(false);
            e.target.value = "";
          }
        }}
      />

      {/* Attached Document Cards (Screenshot Style: Rounded Card with DOCX/XLSX Pill + Filename + Green Checkmark) */}
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

      {/* Quick Attach Bar (File Picker + Sample Reference Quick Buttons) */}
      <div
        className={`flex items-center justify-between gap-1.5 text-[11px] px-2 py-1 rounded-lg border border-dashed transition-colors ${
          isDragOver ? "border-teal-600 bg-teal-50/60" : "border-slate-200 bg-slate-50/60"
        }`}
      >
        <button
          id="attach-file-btn"
          type="button"
          onClick={handlePickFile}
          className="flex items-center gap-1 text-slate-600 hover:text-slate-900 font-medium"
        >
          {loading ? (
            <Loader2 className="w-3.5 h-3.5 animate-spin text-teal-700" />
          ) : (
            <Paperclip className="w-3.5 h-3.5 text-slate-500" />
          )}
          <span>파일 첨부 (DOCX·XLSX·PDF·HWP)</span>
        </button>

        <div className="flex items-center gap-1">
          <button
            id="attach-sample-docx-btn"
            type="button"
            onClick={() => processFileByPath("samples/수학_약안.docx")}
            className="px-1.5 py-0.5 rounded bg-white hover:bg-teal-50 text-teal-800 border border-slate-200 text-[10px] font-semibold"
            title="샘플 수학_약안.docx 즉시 첨부"
          >
            + 수학_약안.docx
          </button>
          <button
            id="attach-sample-xlsx-btn"
            type="button"
            onClick={() => processFileByPath("samples/demo_budget_ref.xlsx")}
            className="px-1.5 py-0.5 rounded bg-white hover:bg-emerald-50 text-emerald-800 border border-slate-200 text-[10px] font-semibold"
            title="샘플 예산안 엑셀(demo_budget_ref.xlsx) 즉시 첨부"
          >
            + 예산안.xlsx
          </button>
        </div>
      </div>
    </div>
  );
};
