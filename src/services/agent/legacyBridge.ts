import {
  callOpenAiAgentTurn,
} from "../providers/openaiProvider.ts";
import type {
  OpenAiAgentTurnResult,
  ReasoningEffortOption,
} from "../providers/openaiProvider.ts";
import { validateCustomEndpoint } from "./endpointPolicy.ts";
import { createProviderAdapter } from "./adapters/index.ts";
import type {
  NeutralMessage,
  NeutralToolSpec,
} from "./types.ts";
import {
  ToolsUnsupportedError,
} from "./types.ts";
import type { AiRequestOptions } from "../aiOrchestrator.ts";

export interface LegacyTurnParams {
  options: AiRequestOptions;
  messages: Array<{
    role: "user" | "assistant" | "tool" | "system";
    content: string | null;
    tool_calls?: any[];
    tool_call_id?: string;
  }>;
  systemPrompt: string;
  tools: any[];
  currentEffort: ReasoningEffortOption;
}

/**
 * Legacy 멀티턴 에이전트 루프에서 공급자 호출을 일반화하는 브리지 함수입니다.
 * - OpenAI/BAZE/Gemini/Ollama/Custom은 OpenAI 형식 메시지와 기존 callOpenAiAgentTurn을 그대로 재사용합니다.
 * - Claude는 OpenAI형 메시지를 중립 메시지로 변환 후 ClaudeAdapter를 호출하여 OpenAI형 응답으로 역변환합니다.
 * - 도구 미지원(ToolsUnsupportedError) 감지 시 호출자(aiOrchestrator)가 single-shot으로 폴백할 수 있도록 에러를 전파합니다.
 */
export async function callLegacyTurn(params: LegacyTurnParams): Promise<OpenAiAgentTurnResult> {
  const { options, messages, systemPrompt, tools, currentEffort } = params;

  if (options.provider === "claude") {
    const adapter = createProviderAdapter({
      provider: "claude",
      apiKey: options.apiKey,
      model: options.model,
      endpoint: options.customEndpoint,
    });

    // OpenAI 형식 메시지 -> 중립 메시지 변환
    const neutralMessages: NeutralMessage[] = messages.map((m) => {
      if (m.role === "system" || m.role === "user") {
        return { role: m.role, content: m.content || "" };
      }
      if (m.role === "assistant") {
        const toolCalls = (m.tool_calls || []).map((tc) => ({
          id: tc.id || "",
          name: tc.function?.name || "",
          argumentsJson: tc.function?.arguments || "{}",
        }));
        return {
          role: "assistant",
          content: m.content,
          toolCalls: toolCalls.length > 0 ? toolCalls : undefined,
        };
      }
      if (m.role === "tool") {
        return {
          role: "tool",
          toolCallId: m.tool_call_id || "",
          content: m.content || "",
        };
      }
      return { role: "user", content: String(m.content || "") };
    });

    const neutralTools: NeutralToolSpec[] = (tools || []).map((t) => ({
      name: t.function?.name || t.name,
      description: t.function?.description || t.description || "",
      parameters: t.function?.parameters || t.parameters || {},
    }));

    const turnResp = await adapter.turn({
      systemPrompt,
      messages: neutralMessages,
      tools: neutralTools,
      toolChoice: "auto",
      reasoningEffort: currentEffort,
      requestId: options.requestId,
      timeoutMs: options.timeoutMs ?? 180_000,
      signal: options.signal,
    });

    const openAiToolCalls = (turnResp.toolCalls || []).map((tc) => ({
      id: tc.id,
      type: "function",
      function: {
        name: tc.name,
        arguments: tc.argumentsJson,
      },
    }));

    return {
      message: {
        role: "assistant",
        content: turnResp.content,
        tool_calls: openAiToolCalls.length > 0 ? openAiToolCalls : undefined,
      },
      toolCalls: openAiToolCalls,
      content: turnResp.content,
      reasoningTokens: turnResp.reasoningTokens || 0,
    };
  }

  // custom 공급자에서 도구 미지원 설정된 경우
  if (options.provider === "custom" && options.customSupportsTools === false) {
    throw new ToolsUnsupportedError("Custom provider has tool support disabled.");
  }

  // openai / baze / gemini / ollama / custom
  let endpointUrl = options.customEndpoint;
  if (options.provider === "baze" && !endpointUrl) {
    endpointUrl = "https://api.baze.co/v1/gateway/chat/completions";
  } else if (options.provider === "openai" && !endpointUrl) {
    endpointUrl = "https://api.openai.com/v1/chat/completions";
  } else if (options.provider === "ollama" && !endpointUrl) {
    endpointUrl = "http://127.0.0.1:11434/v1/chat/completions";
  } else if (options.provider === "gemini" && !endpointUrl) {
    endpointUrl = "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions";
  } else if (options.provider === "custom") {
    if (!endpointUrl) {
      throw new Error("사용자 지정 공급자(custom)의 엔드포인트 URL이 없습니다.");
    }
    const val = validateCustomEndpoint(endpointUrl);
    if (!val.ok) {
      throw new Error(`사용자 지정 공급자 엔드포인트 검증 실패: ${val.reason}`);
    }
    endpointUrl = val.normalized;
  }

  let mappedEffort: ReasoningEffortOption | undefined = currentEffort;
  if (options.provider === "gemini") {
    if (mappedEffort === "xhigh" || mappedEffort === "max") {
      mappedEffort = "high";
    }
  } else if (options.provider === "ollama") {
    mappedEffort = undefined;
  } else if (options.provider === "custom") {
    if (!options.customSendReasoning) {
      mappedEffort = undefined;
    }
  }

  const apiKey = options.provider === "ollama" ? "" : options.apiKey;
  const model =
    options.model ||
    (options.provider === "baze"
      ? "gpt-6.1-terra"
      : options.provider === "gemini"
      ? "gemini-2.5-pro"
      : options.provider === "ollama"
      ? "qwen2.5:14b"
      : options.provider === "custom"
      ? "custom-model"
      : "gpt-4o");

  if (options.signal?.aborted) {
    const err = new Error("Agent loop aborted by user");
    err.name = "AbortError";
    throw err;
  }

  try {
    return await callOpenAiAgentTurn({
      endpointUrl: endpointUrl!,
      apiKey,
      model,
      systemPrompt,
      messages,
      tools,
      toolChoice: "auto",
      reasoningEffort: mappedEffort,
      requestId: options.requestId,
      timeoutMs: options.timeoutMs ?? 180_000,
    });
  } catch (err: any) {
    const message = String(err?.message || err || "");
    if (message.includes("400")) {
      const lower = message.toLowerCase();
      if (
        (lower.includes("tool") || lower.includes("function")) &&
        (lower.includes("not support") ||
          lower.includes("unsupported") ||
          lower.includes("not enabled") ||
          lower.includes("does not support"))
      ) {
        throw new ToolsUnsupportedError(message);
      }
    }
    throw err;
  }
}
