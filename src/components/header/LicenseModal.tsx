import React from "react";
import { useDocStore } from "../../store/useDocStore";
import { GPL_V3_FULL_TEXT } from "../../constants/gplv3Text";
import { X, Scale, FileText } from "lucide-react";

export const LicenseModal: React.FC = () => {
  const { licenseModalOpen, setLicenseModalOpen } = useDocStore();

  if (!licenseModalOpen) return null;

  return (
    <div
      id="license-modal-backdrop"
      className="fixed inset-0 bg-slate-900/50 backdrop-blur-xs flex items-center justify-center z-50 p-4 animate-in fade-in"
    >
      <div
        id="license-modal"
        className="bg-white rounded-2xl shadow-2xl border border-slate-200 w-full max-w-2xl max-h-[85vh] flex flex-col overflow-hidden"
      >
        {/* Header */}
        <div className="px-5 py-4 border-b border-slate-100 flex items-center justify-between bg-slate-50/70">
          <div className="flex items-center gap-2">
            <Scale className="w-5 h-5 text-indigo-600 shrink-0" />
            <h2 className="font-bold text-slate-800 text-sm">
              HOP-Claw 오픈소스 라이선스 고지 (GNU GPL v3.0)
            </h2>
          </div>
          <button
            id="license-modal-close-btn"
            onClick={() => setLicenseModalOpen(false)}
            className="text-slate-400 hover:text-slate-700 p-1 rounded-lg transition-colors"
            title="닫기"
          >
            <X className="w-4 h-4" />
          </button>
        </div>

        {/* Content */}
        <div className="p-5 flex flex-col gap-4 overflow-y-auto text-xs leading-relaxed text-slate-700">
          {/* Summary Banner */}
          <div className="bg-indigo-50/70 border border-indigo-200 rounded-xl p-3.5 flex flex-col gap-1.5">
            <div className="flex items-center gap-1.5 font-bold text-indigo-950 text-xs">
              <FileText className="w-4 h-4 text-indigo-600 shrink-0" />
              <span>HOP-Claw 라이선스 요약 및 고지</span>
            </div>
            <p className="text-slate-700 leading-normal">
              HOP-Claw는 자유 소프트웨어(Free Software)이며, Free Software Foundation이 발행한{" "}
              <strong>GNU General Public License v3.0 (또는 그 이후 버전)</strong>에 따라 수정 및 재배포될 수 있습니다.
            </p>
            <div className="mt-1 pt-2 border-t border-indigo-200/60 text-[11px] text-slate-600 flex flex-col gap-1">
              <div>
                <strong>타사 소프트웨어 라이선스 고지:</strong>
              </div>
              <div>• <strong>@rhwp/core</strong> (HWP 5.0 바이너리 파서): MIT License</div>
              <div>• <strong>Lucide React</strong> (아이콘 에셋): ISC License</div>
              <div>• <strong>Tauri / Rust</strong> (데스크톱 런타임): MIT / Apache-2.0 License</div>
            </div>
          </div>

          {/* Full Text */}
          <div className="flex flex-col gap-1.5">
            <span className="font-semibold text-slate-800 text-[11px]">
              GNU GENERAL PUBLIC LICENSE Version 3, 29 June 2007 전문
            </span>
            <pre
              id="gpl-v3-text-box"
              className="bg-slate-900 text-slate-200 p-3.5 rounded-xl font-mono text-[10px] leading-relaxed max-h-72 overflow-y-auto whitespace-pre-wrap select-text border border-slate-800"
            >
              {GPL_V3_FULL_TEXT}
            </pre>
          </div>
        </div>

        {/* Footer */}
        <div className="px-5 py-3 border-t border-slate-100 flex justify-end bg-slate-50/50">
          <button
            onClick={() => setLicenseModalOpen(false)}
            className="px-4 py-1.5 rounded-lg bg-indigo-600 hover:bg-indigo-700 text-white font-semibold text-xs shadow-xs transition-colors"
          >
            확인
          </button>
        </div>
      </div>
    </div>
  );
};
