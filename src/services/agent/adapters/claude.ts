import { runtimeInvoke as invoke } from "../../../core/runtimeInvoke.ts";
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

export class ClaudeAdapter implements ProviderAdapter {
  readonly id: ProviderId = "claude";
  readonly supportsTools: boolean = true;
  private cfg: ProviderConfig;

  constructor(cfg: ProviderConfig) {
    this.cfg = cfg;
  }

  async turn(req: AgentTurnRequest): Promise<AgentTurnResponse> {
    const url = this.cfg.endpoint || "https://api.anthropic.com/v1/messages";
    const headers: Record<string, string> = {
      "anthropic-version": "2023-06-01",
      "content-type": "application/json",
    };
    if (this.cfg.apiKey) {
      headers["x-api-key"] = this.cfg.apiKey;
    }

    // 시스템 프롬프트 수집
    let systemPrompt = req.systemPrompt || "";
    const systemMessages = req.messages.filter((m) => m.role === "system");
    if (systemMessages.length > 0) {
      const extraSystem = systemMessages.map((m) => (m as any).content).join("\n\n");
      systemPrompt = systemPrompt ? `${systemPrompt}\n\n${extraSystem}` : extraSystem;
    }

    // 중립 메시지 -> Claude 형식(role 교대, tool_result 단일 user 메시지 병합)
    type ClaudeContentBlock =
      | { type: "text"; text: string }
      | { type: "tool_use"; id: string; name: string; input: Record<string, unknown> }
      | { type: "tool_result"; tool_use_id: string; content: string };

    interface IntermediateMessage {
      role: "user" | "assistant";
      blocks: ClaudeContentBlock[];
    }

    const intermediate: IntermediateMessage[] = [];

    for (const m of req.messages) {
      if (m.role === "system") {
        continue;
      }
      if (m.role === "user") {
        intermediate.push({
          role: "user",
          blocks: [{ type: "text", text: m.content }],
        });
      } else if (m.role === "assistant") {
        const blocks: ClaudeContentBlock[] = [];
        if (m.content) {
          blocks.push({ type: "text", text: m.content });
        }
        if (m.toolCalls && m.toolCalls.length > 0) {
          for (const tc of m.toolCalls) {
            let input: Record<string, unknown> = {};
            try {
              input = JSON.parse(tc.argumentsJson);
            } catch {
              input = {};
            }
            blocks.push({
              type: "tool_use",
              id: tc.id,
              name: tc.name,
              input,
            });
          }
        }
        if (blocks.length === 0) {
          blocks.push({ type: "text", text: "" });
        }
        intermediate.push({ role: "assistant", blocks });
      } else if (m.role === "tool") {
        intermediate.push({
          role: "user",
          blocks: [
            {
              type: "tool_result",
              tool_use_id: m.toolCallId,
              content: m.content,
            },
          ],
        });
      }
    }

    // 동일 role 연속 메시지 병합 (다중 tool_result가 단일 user 메시지로 병합됨)
    const mergedMessages: Array<{ role: "user" | "assistant"; content: ClaudeContentBlock[] }> = [];
    for (const msg of intermediate) {
      if (mergedMessages.length > 0 && mergedMessages[mergedMessages.length - 1].role === msg.role) {
        mergedMessages[mergedMessages.length - 1].content.push(...msg.blocks);
      } else {
        mergedMessages.push({
          role: msg.role,
          content: [...msg.blocks],
        });
      }
    }

    // 첫 메시지는 반드시 user여야 함
    if (mergedMessages.length === 0) {
      mergedMessages.push({ role: "user", content: [{ type: "text", text: "시작" }] });
    } else if (mergedMessages[0].role !== "user") {
      mergedMessages.unshift({ role: "user", content: [{ type: "text", text: "진행" }] });
    }

    // 도구 스펙 변환
    const claudeTools = (req.tools || []).map((t) => ({
      name: t.name,
      description: t.description,
      input_schema: t.parameters,
    }));

    const bodyJson: any = {
      model: this.cfg.model || "claude-sonnet-4-20250514",
      max_tokens: 8192,
      messages: mergedMessages,
    };

    if (systemPrompt) {
      bodyJson.system = systemPrompt;
    }

    // tool_choice 매핑:
    // auto/none -> tools 제거 또는 {type:'none'} 대신 tools 생략 / {type:'tool', name}
    if (req.toolChoice !== "none" && claudeTools.length > 0) {
      bodyJson.tools = claudeTools;
      if (req.toolChoice === "auto") {
        bodyJson.tool_choice = { type: "auto" };
      } else if (typeof req.toolChoice === "object" && req.toolChoice && "name" in req.toolChoice) {
        bodyJson.tool_choice = { type: "tool", name: req.toolChoice.name };
      } else {
        bodyJson.tool_choice = { type: "auto" };
      }
    }

    const requestId = req.requestId || `req_${Date.now()}_${Math.random().toString(36).slice(2, 9)}`;
    const timeoutMs = req.timeoutMs ?? 180_000;

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
      const resp = await invoke<{ status: number; body: any }>("call_llm_http", {
        req: {
          url,
          headers,
          body_json: bodyJson,
          timeout_ms: timeoutMs,
          request_id: requestId,
        },
      });

      if (resp.status < 200 || resp.status >= 300) {
        const errText = typeof resp.body === "string" ? resp.body : JSON.stringify(resp.body || {});
        if (resp.status === 429 || resp.status >= 500) {
          throw new TransientProviderError(`Claude API Error (${resp.status}): ${errText}`, resp.status);
        }
        if (resp.status === 400) {
          const lower = errText.toLowerCase();
          if (
            (lower.includes("tool") || lower.includes("function")) &&
            (lower.includes("not support") ||
              lower.includes("unsupported") ||
              lower.includes("not enabled") ||
              lower.includes("does not support"))
          ) {
            throw new ToolsUnsupportedError(`Claude API Error (${resp.status}): ${errText}`);
          }
        }
        throw new Error(`Claude API Error (${resp.status}): ${errText}`);
      }

      const contentBlocks = Array.isArray(resp.body?.content) ? resp.body.content : [];
      let textContent = "";
      const neutralToolCalls: NeutralToolCall[] = [];

      for (const block of contentBlocks) {
        if (block.type === "text" && block.text) {
          textContent += (textContent ? "\n" : "") + block.text;
        } else if (block.type === "tool_use") {
          neutralToolCalls.push({
            id: block.id || "",
            name: block.name || "",
            argumentsJson:
              typeof block.input === "string"
                ? block.input
                : JSON.stringify(block.input || {}),
          });
        }
      }

      return {
        content: textContent,
        toolCalls: neutralToolCalls,
        reasoningTokens: 0,
        stopReason: resp.body?.stop_reason,
      };
    } catch (err: any) {
      if (err instanceof ToolsUnsupportedError || err instanceof TransientProviderError) {
        throw err;
      }
      const message = String(err?.message || err || "");
      if (
        message.includes("fetch failed") ||
        message.includes("timeout") ||
        message.includes("Network") ||
        message.includes("ECONNREFUSED") ||
        message.includes("Local bridge error")
      ) {
        throw new TransientProviderError(message);
      }
      throw err;
    } finally {
      if (req.signal && abortListener) {
        req.signal.removeEventListener("abort", abortListener);
      }
    }
  }
}
