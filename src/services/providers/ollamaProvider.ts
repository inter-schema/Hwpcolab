import { callOpenAiCompatibleApi } from "./openaiProvider.ts";

export async function callOllamaApi(
  endpointUrl: string,
  model: string,
  systemPrompt: string,
  messages: Array<{ role: "user" | "assistant"; content: string }>
) {
  const url = endpointUrl || "http://127.0.0.1:11434/v1/chat/completions";
  return callOpenAiCompatibleApi(url, "", model || "qwen2.5:14b", systemPrompt, messages);
}
