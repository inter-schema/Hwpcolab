import { callOpenAiCompatibleApi } from "./openaiProvider.ts";

export async function callGeminiApi(
  apiKey: string,
  model: string,
  systemPrompt: string,
  messages: Array<{ role: "user" | "assistant"; content: string }>
) {
  const url = "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions";
  return callOpenAiCompatibleApi(url, apiKey, model || "gemini-2.5-pro", systemPrompt, messages);
}
