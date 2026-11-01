import React, { useState, useEffect } from "react";
import { useDocStore } from "../../store/useDocStore";
import { X, Check, ShieldCheck, Key, Cpu, Globe, Brain } from "lucide-react";

export const ModelPrivacySettings: React.FC = () => {
  const {
    settingsModalOpen,
    setSettingsModalOpen,
    setLicenseModalOpen,
    provider,
    apiKey,
    model,
    customEndpoint,
    autoMaskPii,
    reasoningEffort,
    setProviderSettings,
    setAutoMaskPii,
    setReasoningEffort,
  } = useDocStore();

  const [localProvider, setLocalProvider] = useState(provider);
  const [localApiKey, setLocalApiKey] = useState(apiKey);
  const [localModel, setLocalModel] = useState(model);
  const [localEndpoint, setLocalEndpoint] = useState(customEndpoint);
  const [localMaskPii, setLocalMaskPii] = useState(autoMaskPii);
  const [localReasoningEffort, setLocalReasoningEffort] = useState(reasoningEffort);

  // Sync state when modal opens
  useEffect(() => {
    if (settingsModalOpen) {
      // Load saved settings from localStorage if available
      const savedProvider = (localStorage.getItem("hop_provider") as any) || provider;
      const savedKey = localStorage.getItem(`hop_key_${savedProvider}`) || apiKey;
      const savedModel = localStorage.getItem(`hop_model_${savedProvider}`) || model;
      const savedEndpoint =
        savedProvider === "baze"
          ? localStorage.getItem("hop_endpoint_baze") || customEndpoint || "https://api.baze.co/v1/gateway/chat/completions"
          : localStorage.getItem("hop_endpoint_ollama") || customEndpoint;
      const savedReasoning = (localStorage.getItem("hop_reasoning_effort") as any) || reasoningEffort;

      setLocalProvider(savedProvider);
      setLocalApiKey(savedKey);
      setLocalModel(savedModel);
      setLocalEndpoint(savedEndpoint);
      setLocalMaskPii(autoMaskPii);
      setLocalReasoningEffort(savedReasoning);
    }
  }, [settingsModalOpen]);

  if (!settingsModalOpen) return null;

  const handleProviderChange = (p: typeof provider) => {
    setLocalProvider(p);
    const key =
      localStorage.getItem(`hop_key_${p}`) ||
      (p === "baze" ? ((import.meta as any).env?.VITE_BAZE_API_KEY || "") : "");
    setLocalApiKey(key);

    let defaultModel = "claude-sonnet-4-20250514";
    if (p === "baze") defaultModel = "gpt-6-luna";
    if (p === "openai") defaultModel = "gpt-4o";
    if (p === "gemini") defaultModel = "gemini-2.5-pro";
    if (p === "ollama") defaultModel = "qwen2.5:14b";
    setLocalModel(localStorage.getItem(`hop_model_${p}`) || defaultModel);

    if (p === "baze") {
      setLocalEndpoint(localStorage.getItem("hop_endpoint_baze") || "https://api.baze.co/v1/gateway/chat/completions");
    } else if (p === "ollama") {
      setLocalEndpoint(localStorage.getItem("hop_endpoint_ollama") || "http://127.0.0.1:11434/v1/chat/completions");
    }
  };

  const handleSave = () => {
    localStorage.setItem("hop_provider", localProvider);
    localStorage.setItem(`hop_key_${localProvider}`, localApiKey);
    localStorage.setItem(`hop_model_${localProvider}`, localModel);
    localStorage.setItem("hop_reasoning_effort", localReasoningEffort);
    if (localProvider === "ollama") {
      localStorage.setItem("hop_endpoint_ollama", localEndpoint);
    } else if (localProvider === "baze") {
      localStorage.setItem("hop_endpoint_baze", localEndpoint);
    }

    setProviderSettings({
      provider: localProvider,
      apiKey: localApiKey,
      model: localModel,
      customEndpoint: localEndpoint,
    });
    setReasoningEffort(localReasoningEffort);
    setAutoMaskPii(localMaskPii);
    setSettingsModalOpen(false);
  };

  return (
    <div className="fixed inset-0 bg-slate-900/50 backdrop-blur-xs flex items-center justify-center z-50 p-4 animate-in fade-in">
      <div className="bg-white rounded-2xl shadow-2xl border border-slate-200 w-full max-w-md overflow-hidden flex flex-col">
        {/* Header */}
        <div className="px-5 py-4 border-b border-slate-100 flex items-center justify-between bg-slate-50/50">
          <div className="flex items-center gap-2">
            <Cpu className="w-5 h-5 text-indigo-600" />
            <h2 className="font-bold text-slate-800 text-sm">AI 모델 및 프라이버시 설정</h2>
          </div>
          <button
            onClick={() => setSettingsModalOpen(false)}
            className="text-slate-400 hover:text-slate-700 p-1 rounded-lg"
          >
            <X className="w-4 h-4" />
          </button>
        </div>

        {/* Body */}
        <div className="p-5 flex flex-col gap-4 text-xs">
          {/* Provider Selection */}
          <div className="flex flex-col gap-1.5">
            <label className="font-semibold text-slate-700">AI 프로바이더 선택</label>
            <div className="grid grid-cols-5 gap-1 bg-slate-100 p-1 rounded-xl text-center">
              {(["baze", "claude", "openai", "gemini", "ollama"] as const).map((p) => (
                <button
                  key={p}
                  onClick={() => handleProviderChange(p)}
                  className={`py-1.5 px-0.5 rounded-lg font-semibold text-[11px] capitalize transition-all ${
                    localProvider === p
                      ? "bg-white text-indigo-700 shadow-xs"
                      : "text-slate-600 hover:text-slate-900"
                  }`}
                >
                  {p === "baze" ? "BAZE" : p === "ollama" ? "Ollama" : p}
                </button>
              ))}
            </div>
          </div>

          {/* API Key Input (except for Ollama) */}
          {localProvider !== "ollama" ? (
            <div className="flex flex-col gap-1.5">
              <label className="font-semibold text-slate-700 flex items-center justify-between">
                <span className="flex items-center gap-1">
                  <Key className="w-3.5 h-3.5 text-slate-500" />
                  <span>{localProvider === "baze" ? "BAZE Gateway" : localProvider.toUpperCase()} API 키</span>
                </span>
                {localProvider === "baze" && (
                  <span className="text-[10px] text-indigo-600 font-normal">마인드로직 FactChat/BAZE 전용</span>
                )}
              </label>
              <input
                type="password"
                value={localApiKey}
                onChange={(e) => setLocalApiKey(e.target.value)}
                placeholder={localProvider === "baze" ? "baze_..." : "sk-..."}
                className="w-full px-3 py-2 border border-slate-200 rounded-lg focus:outline-none focus:ring-1 focus:ring-indigo-500 text-slate-800"
              />
              <span className="text-[10px] text-slate-400">
                API 키는 로컬 PC에만 안전하게 저장되며 외부로 유출되지 않습니다.
              </span>
            </div>
          ) : (
            <div className="flex flex-col gap-1.5">
              <label className="font-semibold text-slate-700 flex items-center gap-1">
                <Globe className="w-3.5 h-3.5 text-slate-500" />
                <span>Ollama 엔드포인트 URL</span>
              </label>
              <input
                type="text"
                value={localEndpoint}
                onChange={(e) => setLocalEndpoint(e.target.value)}
                placeholder="http://127.0.0.1:11434/v1/chat/completions"
                className="w-full px-3 py-2 border border-slate-200 rounded-lg focus:outline-none focus:ring-1 focus:ring-indigo-500 text-slate-800"
              />
            </div>
          )}

          {/* BAZE Custom Endpoint */}
          {localProvider === "baze" && (
            <div className="flex flex-col gap-1.5">
              <label className="font-semibold text-slate-700 flex items-center gap-1">
                <Globe className="w-3.5 h-3.5 text-slate-500" />
                <span>게이트웨이 엔드포인트 URL</span>
              </label>
              <input
                type="text"
                value={localEndpoint}
                onChange={(e) => setLocalEndpoint(e.target.value)}
                placeholder="https://api.baze.co/v1/gateway/chat/completions"
                className="w-full px-3 py-2 border border-slate-200 rounded-lg focus:outline-none focus:ring-1 focus:ring-indigo-500 text-slate-800 font-mono text-[11px]"
              />
            </div>
          )}

          {/* Model Name */}
          <div className="flex flex-col gap-1.5">
            <div className="flex items-center justify-between">
              <label className="font-semibold text-slate-700">모델 ID</label>
              {localProvider === "baze" && (
                <div className="flex items-center gap-1">
                  {["gpt-6-luna", "gpt-6-sol", "gpt-6.1-terra", "gpt-5.6-terra"].map((m) => (
                    <button
                      key={m}
                      type="button"
                      onClick={() => setLocalModel(m)}
                      className={`text-[10px] px-1.5 py-0.5 rounded border transition-colors ${
                        localModel === m
                          ? "bg-indigo-50 border-indigo-300 text-indigo-700 font-bold"
                          : "bg-slate-50 border-slate-200 text-slate-500 hover:bg-slate-100"
                      }`}
                    >
                      {m}
                    </button>
                  ))}
                </div>
              )}
            </div>
            <input
              type="text"
              value={localModel}
              onChange={(e) => setLocalModel(e.target.value)}
              placeholder="모델 식별자"
              className="w-full px-3 py-2 border border-slate-200 rounded-lg focus:outline-none focus:ring-1 focus:ring-indigo-500 text-slate-800"
            />
          </div>

          {/* Reasoning Effort Selector */}
          <div className="flex flex-col gap-1.5">
            <label className="font-semibold text-slate-700 flex items-center justify-between">
              <span className="flex items-center gap-1">
                <Brain className="w-3.5 h-3.5 text-indigo-600" />
                <span>추론 수준 (Reasoning Effort)</span>
              </span>
              <span className="text-[10px] text-slate-400 font-normal">
                {localReasoningEffort === "high" || localReasoningEffort === "xhigh"
                  ? "정밀 표·레이아웃 계측"
                  : "빠른 응답"}
              </span>
            </label>
            <select
              value={localReasoningEffort}
              onChange={(e) => setLocalReasoningEffort(e.target.value as any)}
              className="w-full px-3 py-2 border border-slate-200 rounded-lg focus:outline-none focus:ring-1 focus:ring-indigo-500 text-slate-800 bg-white font-medium cursor-pointer"
            >
              <option value="none">없음 (None - 즉시 응답)</option>
              <option value="low">낮음 (Low - 빠른 추론)</option>
              <option value="medium">중간 (Medium - 표준 균형)</option>
              <option value="high">높음 (High - 정밀 레이아웃 추론, 권장)</option>
              <option value="xhigh">최대 (XHigh / Max - 심층 에이전틱 추론)</option>
            </select>
            <span className="text-[10px] text-slate-400">
              높음(High) 이상 선택 시 표 셀 잘림 측정, 가상 복사본 시험 적용 및 레이아웃 검증이 수행됩니다.
            </span>
          </div>

          {/* Privacy Toggle */}
          <div className="flex items-center justify-between p-3 bg-slate-50 border border-slate-200 rounded-xl">
            <div className="flex flex-col gap-0.5">
              <div className="flex items-center gap-1.5 font-semibold text-slate-800">
                <ShieldCheck className="w-4 h-4 text-emerald-600" />
                <span>개인정보(PII) 자동 가명화 전송</span>
              </div>
              <span className="text-[10px] text-slate-500">
                주민번호, 휴대전화, 이메일을 임시 토큰으로 치환하여 전송
              </span>
            </div>
            <input
              type="checkbox"
              checked={localMaskPii}
              onChange={(e) => setLocalMaskPii(e.target.checked)}
              className="w-4 h-4 accent-indigo-600 cursor-pointer"
            />
          </div>
        </div>

        {/* Footer */}
        <div className="px-5 py-3 border-t border-slate-100 flex items-center justify-between gap-2 bg-slate-50/50">
          <button
            type="button"
            onClick={() => {
              setSettingsModalOpen(false);
              setLicenseModalOpen(true);
            }}
            className="text-[11px] text-slate-500 hover:text-indigo-600 underline text-left transition-colors"
          >
            본 소프트웨어는 GNU General Public License v3.0 라이선스 하에 배포됩니다.
          </button>
          <div className="flex items-center gap-2 shrink-0">
            <button
              onClick={() => setSettingsModalOpen(false)}
              className="px-3 py-1.5 rounded-lg border border-slate-200 text-slate-600 hover:bg-slate-100 font-semibold"
            >
              취소
            </button>
            <button
              onClick={handleSave}
              className="flex items-center gap-1 px-4 py-1.5 rounded-lg bg-indigo-600 hover:bg-indigo-700 text-white font-semibold shadow-sm"
            >
              <Check className="w-3.5 h-3.5" />
              <span>설정 저장</span>
            </button>
          </div>
        </div>
      </div>
    </div>
  );
};
