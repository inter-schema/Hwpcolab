import React, { useState, useEffect } from "react";
import { useDocStore } from "../../store/useDocStore";
import { X, Check, ShieldCheck, Key, Cpu, Globe, Brain, FileCheck2, AlertTriangle } from "lucide-react";
import { validateCustomEndpoint } from "../../services/agent/endpointPolicy";
import type { ProviderId, AgentLoopMode } from "../../services/agent/types";

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
    agentLoopMode,
    useAsDefaultHwpApp,
    customName,
    customSupportsTools,
    customSendReasoning,
    setProviderSettings,
    setAutoMaskPii,
    setReasoningEffort,
    setAgentLoopMode,
    setUseAsDefaultHwpApp,
  } = useDocStore();

  const [localProvider, setLocalProvider] = useState(provider);
  const [localApiKey, setLocalApiKey] = useState(apiKey);
  const [localModel, setLocalModel] = useState(model);
  const [localEndpoint, setLocalEndpoint] = useState(customEndpoint);
  const [localMaskPii, setLocalMaskPii] = useState(autoMaskPii);
  const [localReasoningEffort, setLocalReasoningEffort] = useState(reasoningEffort);
  const [localLoopMode, setLocalLoopMode] = useState<AgentLoopMode>(agentLoopMode || "v2");
  const [localDefaultHwpApp, setLocalDefaultHwpApp] = useState<boolean>(Boolean(useAsDefaultHwpApp));
  const [localCustomName, setLocalCustomName] = useState(customName || "사용자 지정");
  const [localCustomSupportsTools, setLocalCustomSupportsTools] = useState(customSupportsTools);
  const [localCustomSendReasoning, setLocalCustomSendReasoning] = useState(customSendReasoning);
  const [endpointError, setEndpointError] = useState<string | null>(null);
  const [savedFeedback, setSavedFeedback] = useState(false);

  // Sync state when modal opens
  useEffect(() => {
    if (settingsModalOpen) {
      // Load saved settings from localStorage if available
      const savedProvider = (localStorage.getItem("hop_provider") as ProviderId) || provider;
      const savedKey = localStorage.getItem(`hop_key_${savedProvider}`) || (savedProvider === provider ? apiKey : "");
      const savedModel = localStorage.getItem(`hop_model_${savedProvider}`) || (savedProvider === provider ? model : "");
      let savedEndpoint = localStorage.getItem(`hop_endpoint_${savedProvider}`) || (savedProvider === provider ? customEndpoint : "");
      if (!savedEndpoint) {
        if (savedProvider === "baze") savedEndpoint = "https://api.baze.co/v1/gateway/chat/completions";
        else if (savedProvider === "ollama") savedEndpoint = "http://127.0.0.1:11434/v1/chat/completions";
        else if (savedProvider === "openai") savedEndpoint = "https://api.openai.com/v1/chat/completions";
        else if (savedProvider === "gemini") savedEndpoint = "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions";
      }
      const savedReasoning = (localStorage.getItem("hop_reasoning_effort") as any) || reasoningEffort;
      const savedLoopMode = (localStorage.getItem("hop_agent_loop_mode") as AgentLoopMode) || agentLoopMode || "v2";
      const savedDefaultHwpApp =
        localStorage.getItem("hop_default_hwp_app") !== null
          ? localStorage.getItem("hop_default_hwp_app") === "true"
          : Boolean(useAsDefaultHwpApp);
      const savedCustomName = localStorage.getItem("hop_custom_name") || customName || "사용자 지정";
      const savedCustomSupportsTools =
        localStorage.getItem("hop_custom_supports_tools") !== null
          ? localStorage.getItem("hop_custom_supports_tools") !== "false"
          : customSupportsTools;
      const savedCustomSendReasoning =
        localStorage.getItem("hop_custom_send_reasoning") !== null
          ? localStorage.getItem("hop_custom_send_reasoning") === "true"
          : customSendReasoning;

      setLocalProvider(savedProvider);
      setLocalApiKey(savedKey);
      setLocalModel(savedModel);
      setLocalEndpoint(savedEndpoint);
      setLocalMaskPii(autoMaskPii);
      setLocalReasoningEffort(savedReasoning);
      setLocalLoopMode(savedLoopMode);
      setLocalDefaultHwpApp(savedDefaultHwpApp);
      setLocalCustomName(savedCustomName);
      setLocalCustomSupportsTools(savedCustomSupportsTools);
      setLocalCustomSendReasoning(savedCustomSendReasoning);
      setEndpointError(null);
      setSavedFeedback(false);
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
    if (p === "custom") defaultModel = "custom-model";
    setLocalModel(localStorage.getItem(`hop_model_${p}`) || defaultModel);

    if (p === "baze") {
      setLocalEndpoint(localStorage.getItem("hop_endpoint_baze") || "https://api.baze.co/v1/gateway/chat/completions");
    } else if (p === "ollama") {
      setLocalEndpoint(localStorage.getItem("hop_endpoint_ollama") || "http://127.0.0.1:11434/v1/chat/completions");
    } else if (p === "custom") {
      setLocalEndpoint(localStorage.getItem("hop_endpoint_custom") || "http://127.0.0.1:8000/v1/chat/completions");
      setLocalCustomName(localStorage.getItem("hop_custom_name") || "사용자 지정");
    }
    setEndpointError(null);
  };

  const handleSave = () => {
    if (savedFeedback) return;

    if (localProvider === "custom") {
      if (!localEndpoint.trim()) {
        setEndpointError("엔드포인트 URL을 입력해주세요.");
        return;
      }
      const val = validateCustomEndpoint(localEndpoint);
      if (!val.ok) {
        setEndpointError(val.reason);
        return;
      }
      if (!localModel.trim()) {
        setEndpointError("모델 ID를 입력해주세요.");
        return;
      }
    }

    localStorage.setItem("hop_provider", localProvider);
    localStorage.setItem(`hop_key_${localProvider}`, localApiKey);
    localStorage.setItem(`hop_model_${localProvider}`, localModel);
    localStorage.setItem("hop_reasoning_effort", localReasoningEffort);
    localStorage.setItem("hop_agent_loop_mode", localLoopMode);
    localStorage.setItem("hop_default_hwp_app", String(localDefaultHwpApp));

    let finalEndpoint = localEndpoint;
    if (localProvider === "ollama") {
      localStorage.setItem("hop_endpoint_ollama", localEndpoint);
    } else if (localProvider === "baze") {
      localStorage.setItem("hop_endpoint_baze", localEndpoint);
    } else if (localProvider === "custom") {
      const val = validateCustomEndpoint(localEndpoint);
      finalEndpoint = val.ok ? val.normalized : localEndpoint;
      localStorage.setItem("hop_endpoint_custom", finalEndpoint);
      localStorage.setItem("hop_custom_name", localCustomName);
      localStorage.setItem("hop_custom_supports_tools", String(localCustomSupportsTools));
      localStorage.setItem("hop_custom_send_reasoning", String(localCustomSendReasoning));
    }

    setProviderSettings({
      provider: localProvider,
      apiKey: localApiKey,
      model: localModel,
      customEndpoint: finalEndpoint,
      customName: localCustomName,
      customSupportsTools: localCustomSupportsTools,
      customSendReasoning: localCustomSendReasoning,
      agentLoopMode: localLoopMode,
    });
    setReasoningEffort(localReasoningEffort);
    setAgentLoopMode(localLoopMode);
    setAutoMaskPii(localMaskPii);
    void setUseAsDefaultHwpApp(localDefaultHwpApp);

    setSavedFeedback(true);
    setTimeout(() => {
      setSavedFeedback(false);
      setSettingsModalOpen(false);
    }, 600);
  };

  return (
    <div className="fixed inset-0 bg-slate-900/50 backdrop-blur-xs flex items-center justify-center z-50 p-4 animate-in fade-in">
      <div className="bg-white rounded-2xl shadow-2xl border border-slate-200 w-full max-w-md max-h-[88vh] overflow-hidden flex flex-col animate-in zoom-in-95 duration-150">
        {/* Header */}
        <div className="px-5 py-4 border-b border-slate-100 flex items-center justify-between bg-slate-50/50 shrink-0">
          <div className="flex items-center gap-2">
            <Cpu className="w-5 h-5 text-indigo-600" />
            <div>
              <h2 className="font-bold text-slate-800 text-sm">AI 모델 및 프라이버시 설정</h2>
              <p className="text-[10px] text-slate-500">BAZE, Claude, OpenAI, Gemini, Ollama 및 OpenAI 호환(사용자 지정) 공급자 지원</p>
            </div>
          </div>
          <button
            onClick={() => setSettingsModalOpen(false)}
            className="text-slate-400 hover:text-slate-700 p-1 rounded-lg"
          >
            <X className="w-4 h-4" />
          </button>
        </div>

        {/* Body */}
        <div className="p-5 flex-1 overflow-y-auto flex flex-col gap-4 text-xs min-h-0">
          {/* Provider Selection */}
          <div className="flex flex-col gap-1.5">
            <label className="font-semibold text-slate-700">AI 프로바이더 선택</label>
            <div className="grid grid-cols-6 gap-1 bg-slate-100 p-1 rounded-xl text-center">
              {(["baze", "claude", "openai", "gemini", "ollama", "custom"] as const).map((p) => (
                <button
                  key={p}
                  type="button"
                  onClick={() => handleProviderChange(p)}
                  className={`py-1.5 px-0.5 rounded-lg font-semibold text-[10px] capitalize transition-all ${
                    localProvider === p
                      ? "bg-white text-indigo-700 shadow-xs"
                      : "text-slate-600 hover:text-slate-900"
                  }`}
                >
                  {p === "baze" ? "BAZE" : p === "ollama" ? "Ollama" : p === "custom" ? "사용자 지정" : p}
                </button>
              ))}
            </div>
          </div>

          {/* Custom Provider specific fields */}
          {localProvider === "custom" ? (
            <div className="flex flex-col gap-3 p-3 bg-slate-50 border border-slate-200 rounded-xl">
              <div className="flex flex-col gap-1">
                <label className="font-semibold text-slate-700">공급자 표시명</label>
                <input
                  type="text"
                  value={localCustomName}
                  onChange={(e) => setLocalCustomName(e.target.value)}
                  placeholder="예: 사내 vLLM 서버"
                  className="w-full px-3 py-1.5 border border-slate-200 rounded-lg focus:outline-none focus:ring-1 focus:ring-indigo-500 text-slate-800 bg-white"
                />
              </div>

              <div className="flex flex-col gap-1">
                <label className="font-semibold text-slate-700 flex items-center gap-1">
                  <Globe className="w-3.5 h-3.5 text-slate-500" />
                  <span>엔드포인트 URL (필수)</span>
                </label>
                <input
                  type="text"
                  value={localEndpoint}
                  onChange={(e) => {
                    setLocalEndpoint(e.target.value);
                    setEndpointError(null);
                  }}
                  placeholder="http://127.0.0.1:8000/v1/chat/completions"
                  className={`w-full px-3 py-1.5 border rounded-lg focus:outline-none text-slate-800 font-mono text-[11px] bg-white ${
                    endpointError
                      ? "border-rose-400 focus:ring-1 focus:ring-rose-500"
                      : "border-slate-200 focus:ring-1 focus:ring-indigo-500"
                  }`}
                />
                {endpointError && (
                  <span className="text-[11px] text-rose-600 font-medium">{endpointError}</span>
                )}
                <span className="text-[10px] text-slate-400">
                  https:// 또는 루프백/사설망(127.0.0.1, ::1, 10.x, 192.168.x 등) http:// 만 허용됩니다. (/chat/completions 자동 보정)
                </span>
              </div>

              <div className="flex flex-col gap-1">
                <label className="font-semibold text-slate-700 flex items-center justify-between">
                  <span className="flex items-center gap-1">
                    <Key className="w-3.5 h-3.5 text-slate-500" />
                    <span>API 키 (선택 사항)</span>
                  </span>
                  <span className="text-[10px] text-slate-400 font-normal">인증 불필요 시 비워두세요</span>
                </label>
                <input
                  type="password"
                  value={localApiKey}
                  onChange={(e) => setLocalApiKey(e.target.value)}
                  placeholder="선택 사항 (Bearer 토큰)"
                  className="w-full px-3 py-1.5 border border-slate-200 rounded-lg focus:outline-none focus:ring-1 focus:ring-indigo-500 text-slate-800 bg-white"
                />
              </div>

              <div className="flex flex-col gap-2 pt-1 border-t border-slate-200">
                <label className="flex items-center gap-2 cursor-pointer">
                  <input
                    type="checkbox"
                    checked={localCustomSupportsTools}
                    onChange={(e) => setLocalCustomSupportsTools(e.target.checked)}
                    className="w-4 h-4 accent-indigo-600 cursor-pointer"
                  />
                  <div className="flex flex-col">
                    <span className="font-semibold text-slate-800 text-[11px]">도구 호출(Tool Calling) 지원</span>
                    <span className="text-[10px] text-slate-500">미지원 시 단일 샷(Single-shot) 모드로 실행됩니다.</span>
                  </div>
                </label>

                <label className="flex items-center gap-2 cursor-pointer">
                  <input
                    type="checkbox"
                    checked={localCustomSendReasoning}
                    onChange={(e) => setLocalCustomSendReasoning(e.target.checked)}
                    className="w-4 h-4 accent-indigo-600 cursor-pointer"
                  />
                  <div className="flex flex-col">
                    <span className="font-semibold text-slate-800 text-[11px]">reasoning_effort 파라미터 전송</span>
                    <span className="text-[10px] text-slate-500">서버가 reasoning_effort 미지원 시 400 방지를 위해 해제하세요.</span>
                  </div>
                </label>
              </div>
            </div>
          ) : (
            <>
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
            </>
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

          {/* Agent Loop Mode Radio (Item 4: 개선 루프 기본화 & 기존 루프 레거시 전환) */}
          <div
            data-testid="agent-loop-mode-section"
            className="flex flex-col gap-1.5 p-3 bg-indigo-50/40 border border-indigo-100 rounded-xl"
          >
            <label className="font-semibold text-slate-800 flex items-center justify-between">
              <span className="flex items-center gap-1.5">
                <Cpu className="w-4 h-4 text-indigo-600" />
                <span>에이전트 루프 방식</span>
              </span>
              <span
                className={`text-[10px] font-bold px-2 py-0.5 rounded-full ${
                  localLoopMode === "v2"
                    ? "bg-emerald-100 text-emerald-800"
                    : "bg-slate-200 text-slate-700"
                }`}
              >
                {localLoopMode === "v2" ? "NEW DEFAULT · 기본 루프" : "LEGACY · 레거시 루프"}
              </span>
            </label>
            <div className="grid grid-cols-2 gap-2 mt-1">
              <label
                data-testid="loop-mode-v2-option"
                className={`flex flex-col gap-1 p-2.5 rounded-lg border cursor-pointer transition-colors ${
                  localLoopMode === "v2"
                    ? "bg-emerald-50/60 border-emerald-500 shadow-xs text-slate-900 font-semibold"
                    : "bg-white/60 border-slate-200 text-slate-600 hover:bg-white"
                }`}
              >
                <div className="flex items-center justify-between gap-1">
                  <div className="flex items-center gap-1.5">
                    <input
                      type="radio"
                      name="agentLoopMode"
                      value="v2"
                      checked={localLoopMode === "v2"}
                      onChange={() => setLocalLoopMode("v2")}
                      className="accent-emerald-600 cursor-pointer"
                    />
                    <span className="text-[11px] font-bold">기본 루프 (개선 루프)</span>
                  </div>
                  <span className="px-1.5 py-0.2 rounded-full bg-emerald-100 text-emerald-800 text-[9px] font-extrabold tracking-tight">
                    NEW DEFAULT
                  </span>
                </div>
                <span className="text-[10px] text-slate-500 pl-5 font-normal">
                  도구 호출 및 다단계 문서 편집 최적화 루프
                </span>
              </label>

              <label
                data-testid="loop-mode-legacy-option"
                className={`flex flex-col gap-1 p-2.5 rounded-lg border cursor-pointer transition-colors ${
                  localLoopMode === "legacy"
                    ? "bg-white border-indigo-500 shadow-xs text-indigo-900 font-semibold"
                    : "bg-white/60 border-slate-200 text-slate-600 hover:bg-white"
                }`}
              >
                <div className="flex items-center justify-between gap-1">
                  <div className="flex items-center gap-1.5">
                    <input
                      type="radio"
                      name="agentLoopMode"
                      value="legacy"
                      checked={localLoopMode === "legacy"}
                      onChange={() => setLocalLoopMode("legacy")}
                      className="accent-indigo-600 cursor-pointer"
                    />
                    <span className="text-[11px]">레거시 루프 (기존 루프)</span>
                  </div>
                  <span className="px-1.5 py-0.2 rounded-full bg-slate-100 text-slate-600 text-[9px] font-bold tracking-tight">
                    LEGACY
                  </span>
                </div>
                <span className="text-[10px] text-slate-500 pl-5 font-normal">
                  이전 버전 호환용 단일 턴 중심 레거시 루프
                </span>
              </label>
            </div>
            <span className="text-[10px] text-slate-500 mt-0.5">
              개선 루프가 기본 루프로 전환되었습니다. 호환성 문제가 있을 때만 레거시 루프로 전환하세요.
            </span>
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

          {/* Item 3: HWP Default App Setting (HWP 기본 앱 설정) */}
          <div
            data-testid="default-hwp-app-section"
            className="flex flex-col gap-2 p-3 bg-amber-50/50 border border-amber-200/80 rounded-xl"
          >
            <div className="flex items-center justify-between gap-2">
              <label
                htmlFor="default-hwp-app-checkbox"
                className="flex items-center gap-1.5 font-semibold text-slate-800 cursor-pointer"
              >
                <FileCheck2 className="w-4 h-4 text-teal-700" />
                <span>HWP 기본 앱 설정 (.hwp / .hwpx)</span>
              </label>
              <input
                id="default-hwp-app-checkbox"
                data-testid="default-hwp-app-checkbox"
                type="checkbox"
                checked={localDefaultHwpApp}
                onChange={(e) => setLocalDefaultHwpApp(e.target.checked)}
                className="w-4 h-4 accent-teal-600 cursor-pointer"
              />
            </div>
            <span className="text-[11px] text-slate-700 leading-snug">
              선택적으로 HWP Colab(HOP-Claw)을 한글 문서(.hwp, .hwpx)의 기본 열기 앱으로 지정합니다.
            </span>
            <div
              data-testid="default-hwp-app-warning"
              className="flex items-start gap-1.5 p-2 rounded-lg bg-amber-100/70 border border-amber-300/70 text-[10px] text-amber-950 leading-relaxed"
            >
              <AlertTriangle className="w-3.5 h-3.5 text-amber-700 shrink-0 mt-0.5" />
              <span>
                <strong>주의:</strong> 이 옵션을 활성화하면 PC에 설치된 기존 한글(한컴오피스 한글) 프로그램과의 파일 연결이 변경됩니다. 기존 한글 프로그램으로 다시 열려면 본 설정을 해제하거나 운영체제 기본 앱 설정에서 변경할 수 있습니다.
              </span>
            </div>
          </div>
        </div>

        {/* Footer */}
        <div className="px-5 py-3 border-t border-slate-100 flex items-center justify-between gap-2 bg-slate-50/80 shrink-0 sticky bottom-0">
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
              id="settings-save-button"
              onClick={handleSave}
              disabled={savedFeedback}
              className={`flex items-center gap-1.5 px-4 py-1.5 rounded-lg text-white font-semibold shadow-sm transition-all ${
                savedFeedback
                  ? "bg-emerald-600 hover:bg-emerald-700"
                  : "bg-indigo-600 hover:bg-indigo-700"
              }`}
            >
              <Check className="w-3.5 h-3.5" />
              <span>{savedFeedback ? "저장 완료!" : "설정 저장"}</span>
            </button>
          </div>
        </div>
      </div>
    </div>
  );
};
