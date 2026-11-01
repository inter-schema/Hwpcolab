import { runtimeInvoke as invoke } from "../../core/runtimeInvoke.ts";
import { PROPOSE_HWP_EDITS_TOOL } from "../aiOrchestrator.ts";

export async function callClaudeApi(
  apiKey: string,
  model: string,
  systemPrompt: string,
  messages: Array<{ role: "user" | "assistant"; content: string }>
): Promise<{ summary: string; diffItems: any[] }> {
  const url = "https://api.anthropic.com/v1/messages";
  const headers = {
    "x-api-key": apiKey,
    "anthropic-version": "2023-06-01",
    "content-type": "application/json",
  };

  const claudeTools = [
    {
      name: PROPOSE_HWP_EDITS_TOOL.name,
      description: PROPOSE_HWP_EDITS_TOOL.description,
      input_schema: PROPOSE_HWP_EDITS_TOOL.input_schema,
    },
  ];

  const bodyJson = {
    model: model || "claude-sonnet-4-20250514",
    max_tokens: 4096,
    system: systemPrompt,
    messages: messages.map((m) => ({ role: m.role, content: m.content })),
    tools: claudeTools,
    tool_choice: { type: "auto" },
  };

  const resp = await invoke<{ status: number; body: any }>("call_llm_http", {
    req: {
      url,
      headers,
      body_json: bodyJson,
    },
  });

  if (resp.status < 200 || resp.status >= 300) {
    throw new Error(`Claude API Error (${resp.status}): ${JSON.stringify(resp.body)}`);
  }

  const content = resp.body.content || [];
  for (const block of content) {
    if (block.type === "tool_use" && block.name === "propose_hwp_edits") {
      return block.input;
    }
  }

  // Fallback if returned in text
  for (const block of content) {
    if (block.type === "text" && block.text) {
      const match = block.text.match(/\{[\s\S]*"diffItems"[\s\S]*\}/);
      if (match) {
        try {
          return JSON.parse(match[0]);
        } catch {}
      }
    }
  }

  return {
    summary: content.map((c: any) => c.text || "").join("\n"),
    diffItems: [],
  };
}
