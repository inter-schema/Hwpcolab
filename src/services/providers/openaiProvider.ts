import { runtimeInvoke as invoke } from "../../core/runtimeInvoke.ts";
import { PROPOSE_HWP_EDITS_TOOL } from "../aiOrchestrator.ts";

export type ReasoningEffortOption = "none" | "low" | "medium" | "high" | "xhigh" | "max";

export interface OpenAiAgentTurnParams {
  endpointUrl: string;
  apiKey: string;
  model: string;
  systemPrompt?: string;
  messages: Array<{
    role: "user" | "assistant" | "tool" | "system";
    content: string | null;
    tool_calls?: any[];
    tool_call_id?: string;
  }>;
  tools?: any[];
  toolChoice?: any;
  reasoningEffort?: ReasoningEffortOption;
  requestId?: string;
  timeoutMs?: number;
}

export interface OpenAiAgentTurnResult {
  message: any;
  toolCalls: any[];
  content: string;
  reasoningTokens: number;
}

function normalizeReasoningEffort(effort?: string): string | undefined {
  if (!effort) return undefined;
  if (effort === "max") return "xhigh";
  if (["none", "low", "medium", "high", "xhigh"].includes(effort)) {
    return effort;
  }
  return undefined;
}

/**
 * OpenAI 호환 API의 단일 에이전트 턴(multi-turn & multi-tool 지원)을 실행합니다.
 * BAZE Gateway, OpenAI o-시리즈 및 gpt-6 계열의 reasoning_effort와 400 에러 자동 폴백을 완벽히 지원합니다.
 */
export async function callOpenAiAgentTurn(
  params: OpenAiAgentTurnParams
): Promise<OpenAiAgentTurnResult> {
  let targetUrl = params.endpointUrl || "https://api.baze.co/v1/gateway/chat/completions";
  if (!targetUrl.includes("/chat/completions") && !targetUrl.includes("/responses")) {
    targetUrl = targetUrl.replace(/\/+$/, "") + "/chat/completions";
  }

  const headers: Record<string, string> = {
    "content-type": "application/json",
  };
  if (params.apiKey) {
    headers["Authorization"] = `Bearer ${params.apiKey}`;
    headers["x-api-key"] = params.apiKey;
  }

  const bodyMessages: any[] = [];
  if (params.systemPrompt && !params.messages.some((m) => m.role === "system")) {
    bodyMessages.push({ role: "system", content: params.systemPrompt });
  }
  for (const m of params.messages) {
    const msgObj: any = { role: m.role, content: m.content };
    if (m.tool_calls) msgObj.tool_calls = m.tool_calls;
    if (m.tool_call_id) msgObj.tool_call_id = m.tool_call_id;
    bodyMessages.push(msgObj);
  }

  const bodyJson: any = {
    model: params.model || "gpt-6-luna",
    messages: bodyMessages,
  };

  if (params.tools && params.tools.length > 0) {
    bodyJson.tools = params.tools;
    bodyJson.tool_choice = params.toolChoice || "auto";
  }

  const normalizedEffort = normalizeReasoningEffort(params.reasoningEffort);
  if (normalizedEffort && normalizedEffort !== "none") {
    bodyJson.reasoning_effort = normalizedEffort;
  }

  const timeoutMs = params.timeoutMs ?? 180_000;

  let resp = await invoke<{ status: number; body: any }>("call_llm_http", {
    req: {
      url: targetUrl,
      headers,
      body_json: bodyJson,
      timeout_ms: timeoutMs,
      request_id: params.requestId,
    },
  });

  // reasoning_effort 미지원으로 인한 400 에러 발생 시 자동 폴백 재시도
  if (resp.status === 400 && bodyJson.reasoning_effort) {
    const errText = JSON.stringify(resp.body);
    if (errText.includes("reasoning_effort")) {
      console.warn("[openaiProvider] 400 with reasoning_effort. Retrying without reasoning_effort...");
      delete bodyJson.reasoning_effort;
      resp = await invoke<{ status: number; body: any }>("call_llm_http", {
        req: {
          url: targetUrl,
          headers,
          body_json: bodyJson,
          timeout_ms: timeoutMs,
          request_id: params.requestId,
        },
      });
    }
  }

  if (resp.status < 200 || resp.status >= 300) {
    throw new Error(`LLM API Error (${resp.status}): ${JSON.stringify(resp.body)}`);
  }

  const choice = resp.body.choices?.[0];
  if (!choice) {
    throw new Error(`LLM returned empty choices: ${JSON.stringify(resp.body)}`);
  }

  const message = choice.message || {};
  const toolCalls = Array.isArray(message.tool_calls) ? message.tool_calls : [];
  const content = message.content || "";
  const reasoningTokens = resp.body?.usage?.completion_tokens_details?.reasoning_tokens || 0;

  return {
    message,
    toolCalls,
    content,
    reasoningTokens,
  };
}

export async function callOpenAiCompatibleApi(
  endpointUrl: string,
  apiKey: string,
  model: string,
  systemPrompt: string,
  messages: Array<{ role: "user" | "assistant"; content: string }>,
  reasoningEffort?: ReasoningEffortOption,
  requestId?: string,
  timeoutMs?: number
): Promise<{ summary: string; diffItems: any[] }> {
  const openAiTools = [
    {
      type: "function",
      function: {
        name: PROPOSE_HWP_EDITS_TOOL.name,
        description: PROPOSE_HWP_EDITS_TOOL.description,
        parameters: PROPOSE_HWP_EDITS_TOOL.input_schema,
      },
    },
  ];

  const result = await callOpenAiAgentTurn({
    endpointUrl,
    apiKey,
    model: model || "gpt-4o",
    systemPrompt,
    messages: messages.map((m) => ({ role: m.role, content: m.content })),
    tools: openAiTools,
    toolChoice: "auto",
    reasoningEffort,
    requestId,
    timeoutMs,
  });

  const toolCalls = result.toolCalls;
  if (Array.isArray(toolCalls) && toolCalls.length > 0) {
    const editCall = toolCalls.find((t) => t.function?.name === "propose_hwp_edits") || toolCalls[0];
    if (editCall?.function?.arguments) {
      try {
        return JSON.parse(editCall.function.arguments);
      } catch (e) {
        console.warn("JSON parse error on tool arguments:", e);
      }
    }
  }

  // Fallback: extract JSON from content text (especially useful for local Ollama models)
  const textContent = result.content;
  const jsonBlockMatch = textContent.match(/```json\s*([\s\S]*?)\s*```/) || textContent.match(/\{[\s\S]*"diffItems"[\s\S]*\}/);
  if (jsonBlockMatch) {
    const rawJson = jsonBlockMatch[1] || jsonBlockMatch[0];
    try {
      const parsed = JSON.parse(rawJson);
      if (parsed.diffItems) {
        return parsed;
      }
    } catch {}
  }

  return {
    summary: textContent,
    diffItems: [],
  };
}
