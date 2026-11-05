import type { ReasoningEffortOption } from "../providers/openaiProvider.ts";

export type ProviderId = "claude" | "openai" | "gemini" | "ollama" | "baze" | "custom";
export type AgentLoopMode = "legacy" | "v2";

export interface NeutralToolCall { id: string; name: string; argumentsJson: string }
export type NeutralMessage =
  | { role: "system" | "user"; content: string }
  | { role: "assistant"; content: string | null; toolCalls?: NeutralToolCall[] }
  | { role: "tool"; toolCallId: string; name?: string; content: string };
export interface NeutralToolSpec { name: string; description: string; parameters: Record<string, unknown> }

export type ToolChoice = "auto" | "none" | { name: string };
export interface AgentTurnRequest {
  systemPrompt: string;
  messages: NeutralMessage[];
  tools: NeutralToolSpec[];
  toolChoice: ToolChoice;
  reasoningEffort?: ReasoningEffortOption;
  requestId?: string;
  timeoutMs?: number;
  signal?: AbortSignal;
}
export interface AgentTurnResponse {
  content: string;
  toolCalls: NeutralToolCall[];
  reasoningTokens?: number;
  stopReason?: string;
}
export class ToolsUnsupportedError extends Error {}
export class TransientProviderError extends Error { constructor(msg: string, public status?: number) { super(msg); } }

export interface ProviderAdapter {
  readonly id: ProviderId;
  readonly supportsTools: boolean;
  turn(req: AgentTurnRequest): Promise<AgentTurnResponse>;
}
export interface ProviderConfig {
  provider: ProviderId; apiKey: string; model: string; endpoint?: string;
  customName?: string; customSupportsTools?: boolean; customSendReasoning?: boolean;
}
