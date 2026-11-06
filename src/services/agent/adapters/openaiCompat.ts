import { runtimeInvoke as invoke } from "../../../core/runtimeInvoke.ts";
import { callOpenAiAgentTurn } from "../../providers/openaiProvider.ts";
import { validateCustomEndpoint } from "../endpointPolicy.ts";
import type {
  AgentTurnRequest,
  AgentTurnResponse,
  NeutralMessage,
  NeutralToolCall,
  ProviderAdapter,
  ProviderConfig,
  ProviderId,
} from "../types.ts";
import {
  ToolsUnsupportedError,
  TransientProviderError,
} from "../types.ts";

export class OpenAiCompatAdapter implements ProviderAdapter {
  readonly id: ProviderId;
  readonly supportsTools: boolean;
  private cfg: ProviderConfig;

  constructor(cfg: ProviderConfig, id?: ProviderId) {
    this.cfg = cfg;
    this.id = id || cfg.provider;
    this.supportsTools = this.id === "custom" ? (cfg.customSupportsTools ?? true) : true;
  }

  async turn(req: AgentTurnRequest): Promise<AgentTurnResponse> {
    if (req.tools && req.tools.length > 0 && !this.supportsTools) {
      throw new ToolsUnsupportedError(`Provider ${this.id} does not support tools.`);
    }

    let endpointUrl = this.cfg.endpoint;
    if (this.id === "baze") {
      endpointUrl = endpointUrl || "https://api.baze.co/v1/gateway/chat/completions";
    } else if (this.id === "openai") {
      endpointUrl = endpointUrl || "https://api.openai.com/v1/chat/completions";
    } else if (this.id === "ollama") {
      endpointUrl = endpointUrl || "http://127.0.0.1:11434/v1/chat/completions";
    } else if (this.id === "gemini") {
      endpointUrl = endpointUrl || "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions";
    } else if (this.id === "custom") {
      if (!endpointUrl) {
        throw new Error("사용자 지정 공급자(custom)의 엔드포인트 URL이 지정되지 않았습니다.");
      }
      const val = validateCustomEndpoint(endpointUrl);
      if (!val.ok) {
        throw new Error(`유효하지 않은 사용자 지정 엔드포인트: ${val.reason}`);
      }
      endpointUrl = val.normalized;
    }

    const apiKey = this.id === "ollama" ? "" : (this.cfg.apiKey || "");

    // 공급자별 reasoning effort 매핑
    let mappedEffort = req.reasoningEffort;
    if (this.id === "gemini") {
      if (mappedEffort === "xhigh" || mappedEffort === "max") {
        mappedEffort = "high";
      }
    } else if (this.id === "ollama") {
      mappedEffort = undefined;
    } else if (this.id === "custom") {
      if (!this.cfg.customSendReasoning) {
        mappedEffort = undefined;
      }
    }

    // 중립 메시지 -> OpenAI 형식 변환
    const openAiMessages = req.messages.map((m: NeutralMessage) => {
      if (m.role === "system" || m.role === "user") {
        return { role: m.role, content: m.content };
      }
      if (m.role === "assistant") {
        const msg: any = { role: "assistant", content: m.content };
        if (m.toolCalls && m.toolCalls.length > 0) {
          msg.tool_calls = m.toolCalls.map((tc) => ({
            id: tc.id,
            type: "function",
            function: {
              name: tc.name,
              arguments: tc.argumentsJson,
            },
          }));
        }
        return msg;
      }
      if (m.role === "tool") {
        const msg: any = {
          role: "tool",
          tool_call_id: m.toolCallId,
          content: m.content,
        };
        if (m.name) msg.name = m.name;
        return msg;
      }
      return m as any;
    });

    // 도구 스펙 변환
    let tools: any[] | undefined = undefined;
    if (this.supportsTools && req.tools && req.tools.length > 0) {
      tools = req.tools.map((t) => ({
        type: "function",
        function: {
          name: t.name,
          description: t.description,
          parameters: t.parameters,
        },
      }));
    }

    // toolChoice 매핑: {name} -> {type:'function', function:{name}}, 'none' -> 'none', 'auto' -> 'auto'
    let toolChoice: any = undefined;
    if (tools && tools.length > 0) {
      if (req.toolChoice === "none") {
        toolChoice = "none";
      } else if (req.toolChoice === "auto") {
        toolChoice = "auto";
      } else if (typeof req.toolChoice === "object" && req.toolChoice && "name" in req.toolChoice) {
        toolChoice = {
          type: "function",
          function: { name: req.toolChoice.name },
        };
      } else {
        toolChoice = "auto";
      }
    }

    const requestId = req.requestId || `req_${Date.now()}_${Math.random().toString(36).slice(2, 9)}`;

    // AbortSignal -> cancel_llm_request 매핑
    let abortListener: (() => void) | undefined;
    if (req.signal) {
      if (req.signal.aborted) {
        void invoke("cancel_llm_request", { requestId, request_id: requestId }).catch(() => {});
        throw new DOMException("The operation was aborted.", "AbortError");
      }
      abortListener = () => {
        void invoke("cancel_llm_request", { requestId, request_id: requestId }).catch(() => {});
      };
      req.signal.addEventListener("abort", abortListener, { once: true });
    }

    try {
      const turnResult = await callOpenAiAgentTurn({
        endpointUrl: endpointUrl || "https://api.openai.com/v1/chat/completions",
        apiKey,
        model: this.cfg.model,
        systemPrompt: req.systemPrompt,
        messages: openAiMessages,
        tools,
        toolChoice,
        reasoningEffort: mappedEffort,
        requestId,
        timeoutMs: req.timeoutMs,
      });

      const neutralToolCalls: NeutralToolCall[] = (turnResult.toolCalls || []).map((tc: any) => ({
        id: tc.id || "",
        name: tc.function?.name || "",
        argumentsJson: tc.function?.arguments || "{}",
      }));

      return {
        content: turnResult.content || "",
        toolCalls: neutralToolCalls,
        reasoningTokens: turnResult.reasoningTokens,
        stopReason: turnResult.message?.finish_reason,
      };
    } catch (err: any) {
      if (err instanceof ToolsUnsupportedError || err instanceof TransientProviderError) {
        throw err;
      }
      const message = String(err?.message || err || "");
      const statusMatch = message.match(/LLM API Error \((\d+)\)/);
      const status = statusMatch ? parseInt(statusMatch[1], 10) : undefined;

      // 429 및 5xx는 일시적 오류로 재시도 가능
      if (status === 429 || (status !== undefined && status >= 500 && status < 600)) {
        throw new TransientProviderError(message, status);
      }
      if (
        !status &&
        (message.includes("fetch failed") ||
          message.includes("timeout") ||
          message.includes("Network") ||
          message.includes("ECONNREFUSED") ||
          message.includes("Local bridge error"))
      ) {
        throw new TransientProviderError(message);
      }

      // 도구 미지원 400 에러 감지
      if (status === 400) {
        const lower = message.toLowerCase();
        const mentionsTool = lower.includes("tool") || lower.includes("function");
        const mentionsUnsupported =
          lower.includes("not support") ||
          lower.includes("unsupported") ||
          lower.includes("not enabled") ||
          lower.includes("does not support");
        if (mentionsTool && mentionsUnsupported) {
          throw new ToolsUnsupportedError(message);
        }
      }

      throw err;
    } finally {
      if (req.signal && abortListener) {
        req.signal.removeEventListener("abort", abortListener);
      }
    }
  }
}
