import { describe, it, expect, vi, beforeEach } from "vitest";
import { OpenAiCompatAdapter } from "./openaiCompat.ts";
import { createProviderAdapter } from "./index.ts";
import { ToolsUnsupportedError, TransientProviderError } from "../types.ts";
import * as runtimeModule from "../../../core/runtimeInvoke.ts";

vi.mock("../../../core/runtimeInvoke.ts", () => ({
  runtimeInvoke: vi.fn(),
  isTauriEnv: () => false,
}));

describe("OpenAiCompatAdapter", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("converts neutral messages and tool calls to OpenAI format and parses response", async () => {
    const mockInvoke = vi.mocked(runtimeModule.runtimeInvoke);
    mockInvoke.mockResolvedValueOnce({
      status: 200,
      body: {
        choices: [
          {
            message: {
              role: "assistant",
              content: "Here is your response",
              tool_calls: [
                {
                  id: "call_123",
                  type: "function",
                  function: {
                    name: "propose_hwp_edits",
                    arguments: '{"diffItems":[]}',
                  },
                },
              ],
            },
          },
        ],
        usage: {
          completion_tokens_details: {
            reasoning_tokens: 42,
          },
        },
      },
    });

    const adapter = new OpenAiCompatAdapter({
      provider: "openai",
      apiKey: "test-key",
      model: "gpt-4o",
    });

    const res = await adapter.turn({
      systemPrompt: "System instruction",
      messages: [
        { role: "user", content: "Hello" },
        {
          role: "assistant",
          content: null,
          toolCalls: [{ id: "call_000", name: "inspect_layout", argumentsJson: "{}" }],
        },
        { role: "tool", toolCallId: "call_000", content: '{"status":"ok"}' },
      ],
      tools: [
        {
          name: "propose_hwp_edits",
          description: "Propose edits",
          parameters: { type: "object", properties: {} },
        },
      ],
      toolChoice: { name: "propose_hwp_edits" },
      requestId: "test-req-1",
    });

    expect(mockInvoke).toHaveBeenCalledTimes(1);
    const invokeArgs = mockInvoke.mock.calls[0][1];
    expect(invokeArgs.req.url).toBe("https://api.openai.com/v1/chat/completions");
    expect(invokeArgs.req.headers["Authorization"]).toBe("Bearer test-key");
    expect(invokeArgs.req.body_json.tool_choice).toEqual({
      type: "function",
      function: { name: "propose_hwp_edits" },
    });
    expect(invokeArgs.req.body_json.messages).toEqual([
      { role: "system", content: "System instruction" },
      { role: "user", content: "Hello" },
      {
        role: "assistant",
        content: null,
        tool_calls: [
          {
            id: "call_000",
            type: "function",
            function: { name: "inspect_layout", arguments: "{}" },
          },
        ],
      },
      { role: "tool", tool_call_id: "call_000", content: '{"status":"ok"}' },
    ]);

    expect(res.content).toBe("Here is your response");
    expect(res.toolCalls).toEqual([
      {
        id: "call_123",
        name: "propose_hwp_edits",
        argumentsJson: '{"diffItems":[]}',
      },
    ]);
    expect(res.reasoningTokens).toBe(42);
  });

  it("handles toolChoice 'none'", async () => {
    const mockInvoke = vi.mocked(runtimeModule.runtimeInvoke);
    mockInvoke.mockResolvedValueOnce({
      status: 200,
      body: {
        choices: [{ message: { role: "assistant", content: "No tools used" } }],
      },
    });

    const adapter = new OpenAiCompatAdapter({
      provider: "baze",
      apiKey: "baze-key",
      model: "gpt-6-luna",
    });

    await adapter.turn({
      systemPrompt: "sys",
      messages: [{ role: "user", content: "hi" }],
      tools: [{ name: "test_tool", description: "d", parameters: {} }],
      toolChoice: "none",
    });

    const invokeArgs = mockInvoke.mock.calls[0][1];
    expect(invokeArgs.req.body_json.tool_choice).toBe("none");
  });

  it("maps Gemini reasoning effort: xhigh/max -> high", async () => {
    const mockInvoke = vi.mocked(runtimeModule.runtimeInvoke);
    mockInvoke.mockResolvedValueOnce({
      status: 200,
      body: { choices: [{ message: { content: "ok" } }] },
    });

    const adapter = createProviderAdapter({
      provider: "gemini",
      apiKey: "gem-key",
      model: "gemini-2.5-pro",
    });

    await adapter.turn({
      systemPrompt: "sys",
      messages: [{ role: "user", content: "hi" }],
      tools: [],
      toolChoice: "auto",
      reasoningEffort: "xhigh",
    });

    const invokeArgs = mockInvoke.mock.calls[0][1];
    expect(invokeArgs.req.body_json.reasoning_effort).toBe("high");
  });

  it("handles Ollama: no apiKey sent, no reasoning effort", async () => {
    const mockInvoke = vi.mocked(runtimeModule.runtimeInvoke);
    mockInvoke.mockResolvedValueOnce({
      status: 200,
      body: { choices: [{ message: { content: "ok" } }] },
    });

    const adapter = createProviderAdapter({
      provider: "ollama",
      apiKey: "ignored-key",
      model: "qwen2.5:14b",
    });

    await adapter.turn({
      systemPrompt: "sys",
      messages: [{ role: "user", content: "hi" }],
      tools: [],
      toolChoice: "auto",
      reasoningEffort: "high",
    });

    const invokeArgs = mockInvoke.mock.calls[0][1];
    expect(invokeArgs.req.headers["Authorization"]).toBeUndefined();
    expect(invokeArgs.req.body_json.reasoning_effort).toBeUndefined();
  });

  it("handles Custom provider: sends reasoning only when customSendReasoning=true", async () => {
    const mockInvoke = vi.mocked(runtimeModule.runtimeInvoke);
    mockInvoke.mockResolvedValue({
      status: 200,
      body: { choices: [{ message: { content: "ok" } }] },
    });

    const adapterWithoutReasoning = createProviderAdapter({
      provider: "custom",
      apiKey: "custom-key",
      model: "local-model",
      endpoint: "http://127.0.0.1:8000/v1",
      customSendReasoning: false,
    });

    await adapterWithoutReasoning.turn({
      systemPrompt: "sys",
      messages: [{ role: "user", content: "hi" }],
      tools: [],
      toolChoice: "auto",
      reasoningEffort: "medium",
    });

    expect(mockInvoke.mock.calls[0][1].req.body_json.reasoning_effort).toBeUndefined();

    const adapterWithReasoning = createProviderAdapter({
      provider: "custom",
      apiKey: "custom-key",
      model: "local-model",
      endpoint: "http://127.0.0.1:8000/v1",
      customSendReasoning: true,
    });

    await adapterWithReasoning.turn({
      systemPrompt: "sys",
      messages: [{ role: "user", content: "hi" }],
      tools: [],
      toolChoice: "auto",
      reasoningEffort: "medium",
    });

    expect(mockInvoke.mock.calls[1][1].req.body_json.reasoning_effort).toBe("medium");
  });

  it("throws TransientProviderError on 429", async () => {
    const mockInvoke = vi.mocked(runtimeModule.runtimeInvoke);
    mockInvoke.mockResolvedValueOnce({
      status: 429,
      body: { error: "Rate limit exceeded" },
    });

    const adapter = createProviderAdapter({
      provider: "openai",
      apiKey: "test",
      model: "gpt-4o",
    });

    await expect(
      adapter.turn({
        systemPrompt: "sys",
        messages: [{ role: "user", content: "hi" }],
        tools: [],
        toolChoice: "auto",
      })
    ).rejects.toThrow(TransientProviderError);
  });

  it("throws TransientProviderError on 503", async () => {
    const mockInvoke = vi.mocked(runtimeModule.runtimeInvoke);
    mockInvoke.mockResolvedValueOnce({
      status: 503,
      body: { error: "Service unavailable" },
    });

    const adapter = createProviderAdapter({
      provider: "openai",
      apiKey: "test",
      model: "gpt-4o",
    });

    await expect(
      adapter.turn({
        systemPrompt: "sys",
        messages: [{ role: "user", content: "hi" }],
        tools: [],
        toolChoice: "auto",
      })
    ).rejects.toThrow(TransientProviderError);
  });

  it("throws ToolsUnsupportedError when 400 indicates tools not supported", async () => {
    const mockInvoke = vi.mocked(runtimeModule.runtimeInvoke);
    mockInvoke.mockResolvedValueOnce({
      status: 400,
      body: { error: { message: "This model does not support tools or function calling" } },
    });

    const adapter = createProviderAdapter({
      provider: "openai",
      apiKey: "test",
      model: "gpt-3.5-turbo",
    });

    await expect(
      adapter.turn({
        systemPrompt: "sys",
        messages: [{ role: "user", content: "hi" }],
        tools: [{ name: "test", description: "desc", parameters: {} }],
        toolChoice: "auto",
      })
    ).rejects.toThrow(ToolsUnsupportedError);
  });

  it("retries on 400 reasoning_effort error automatically (reusing legacy fallback)", async () => {
    const mockInvoke = vi.mocked(runtimeModule.runtimeInvoke);
    // First call: 400 with reasoning_effort in body
    mockInvoke.mockResolvedValueOnce({
      status: 400,
      body: { error: "Unknown parameter: reasoning_effort is not supported" },
    });
    // Retry call: 200 success without reasoning_effort
    mockInvoke.mockResolvedValueOnce({
      status: 200,
      body: { choices: [{ message: { content: "Fallback success" } }] },
    });

    const adapter = createProviderAdapter({
      provider: "openai",
      apiKey: "test",
      model: "gpt-4o",
    });

    const res = await adapter.turn({
      systemPrompt: "sys",
      messages: [{ role: "user", content: "hi" }],
      tools: [],
      toolChoice: "auto",
      reasoningEffort: "medium",
    });

    expect(mockInvoke).toHaveBeenCalledTimes(2);
    // 2nd call body should not have reasoning_effort
    expect(mockInvoke.mock.calls[1][1].req.body_json.reasoning_effort).toBeUndefined();
    expect(res.content).toBe("Fallback success");
  });

  it("cancels request when AbortSignal triggers", async () => {
    const mockInvoke = vi.mocked(runtimeModule.runtimeInvoke);
    const controller = new AbortController();

    mockInvoke.mockImplementationOnce(async (cmd, args) => {
      controller.abort();
      return { status: 200, body: { choices: [{ message: { content: "done" } }] } };
    });
    mockInvoke.mockResolvedValue({ status: 200, body: true });

    const adapter = createProviderAdapter({
      provider: "openai",
      apiKey: "test",
      model: "gpt-4o",
    });

    await adapter.turn({
      systemPrompt: "sys",
      messages: [{ role: "user", content: "hi" }],
      tools: [],
      toolChoice: "auto",
      requestId: "req-abort-test",
      signal: controller.signal,
    });

    expect(mockInvoke).toHaveBeenCalledWith(
      "cancel_llm_request",
      expect.objectContaining({ requestId: "req-abort-test" })
    );
  });
});
