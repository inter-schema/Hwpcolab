import { describe, it, expect, vi, beforeEach } from "vitest";
import { ClaudeAdapter } from "./claude.ts";
import { createProviderAdapter } from "./index.ts";
import { ToolsUnsupportedError, TransientProviderError } from "../types.ts";
import * as runtimeModule from "../../../core/runtimeInvoke.ts";

vi.mock("../../../core/runtimeInvoke.ts", () => ({
  runtimeInvoke: vi.fn(),
  isTauriEnv: () => false,
}));

describe("ClaudeAdapter", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("merges consecutive tool results into a single user message and enforces role alternation", async () => {
    const mockInvoke = vi.mocked(runtimeModule.runtimeInvoke);
    mockInvoke.mockResolvedValueOnce({
      status: 200,
      body: {
        id: "msg_123",
        type: "message",
        role: "assistant",
        content: [
          { type: "text", text: "I have inspected the layout." },
          {
            type: "tool_use",
            id: "toolu_456",
            name: "propose_hwp_edits",
            input: { diffItems: [] },
          },
        ],
        stop_reason: "tool_use",
      },
    });

    const adapter = createProviderAdapter({
      provider: "claude",
      apiKey: "sk-ant-test",
      model: "claude-sonnet-4-20250514",
    });

    const res = await adapter.turn({
      systemPrompt: "You are a document editor.",
      messages: [
        { role: "system", content: "Extra system instruction" },
        { role: "user", content: "Please fix document" },
        {
          role: "assistant",
          content: "Let me check the layout first",
          toolCalls: [
            { id: "call_layout", name: "inspect_layout", argumentsJson: "{}" },
            { id: "call_read", name: "read_document", argumentsJson: '{"start":0}' },
          ],
        },
        // Two consecutive tool replies (must be merged into ONE user message)
        { role: "tool", toolCallId: "call_layout", content: '{"status":"ok"}' },
        { role: "tool", toolCallId: "call_read", content: '{"paragraphs":["line1"]}' },
      ],
      tools: [
        {
          name: "propose_hwp_edits",
          description: "Propose edits",
          parameters: { type: "object", properties: {} },
        },
      ],
      toolChoice: { name: "propose_hwp_edits" },
      requestId: "claude-req-1",
    });

    expect(mockInvoke).toHaveBeenCalledTimes(1);
    const invokeArgs = mockInvoke.mock.calls[0][1];
    expect(invokeArgs.req.url).toBe("https://api.anthropic.com/v1/messages");
    expect(invokeArgs.req.headers["x-api-key"]).toBe("sk-ant-test");
    expect(invokeArgs.req.headers["anthropic-version"]).toBe("2023-06-01");

    const body = invokeArgs.req.body_json;
    expect(body.max_tokens).toBe(8192);
    expect(body.system).toContain("You are a document editor.");
    expect(body.system).toContain("Extra system instruction");

    // Check message sequence:
    // Message 0: user (text: "Please fix document")
    // Message 1: assistant (text: "Let me check..." + 2 tool_uses)
    // Message 2: user (2 tool_results merged together!)
    expect(body.messages.length).toBe(3);
    expect(body.messages[0].role).toBe("user");
    expect(body.messages[1].role).toBe("assistant");
    expect(body.messages[2].role).toBe("user");

    const toolResultBlocks = body.messages[2].content;
    expect(toolResultBlocks).toEqual([
      { type: "tool_result", tool_use_id: "call_layout", content: '{"status":"ok"}' },
      { type: "tool_result", tool_use_id: "call_read", content: '{"paragraphs":["line1"]}' },
    ]);

    expect(body.tool_choice).toEqual({
      type: "tool",
      name: "propose_hwp_edits",
    });

    expect(res.content).toBe("I have inspected the layout.");
    expect(res.toolCalls).toEqual([
      {
        id: "toolu_456",
        name: "propose_hwp_edits",
        argumentsJson: '{"diffItems":[]}',
      },
    ]);
    expect(res.stopReason).toBe("tool_use");
  });

  it("omits tools and tool_choice when toolChoice is 'none'", async () => {
    const mockInvoke = vi.mocked(runtimeModule.runtimeInvoke);
    mockInvoke.mockResolvedValueOnce({
      status: 200,
      body: {
        content: [{ type: "text", text: "Answer without tools" }],
      },
    });

    const adapter = new ClaudeAdapter({
      provider: "claude",
      apiKey: "test",
      model: "claude-sonnet-4-20250514",
    });

    await adapter.turn({
      systemPrompt: "sys",
      messages: [{ role: "user", content: "hi" }],
      tools: [{ name: "test_tool", description: "desc", parameters: {} }],
      toolChoice: "none",
    });

    const invokeArgs = mockInvoke.mock.calls[0][1];
    expect(invokeArgs.req.body_json.tools).toBeUndefined();
    expect(invokeArgs.req.body_json.tool_choice).toBeUndefined();
  });

  it("sets tool_choice to { type: 'auto' } when toolChoice is 'auto'", async () => {
    const mockInvoke = vi.mocked(runtimeModule.runtimeInvoke);
    mockInvoke.mockResolvedValueOnce({
      status: 200,
      body: {
        content: [{ type: "text", text: "Auto tool turn" }],
      },
    });

    const adapter = new ClaudeAdapter({
      provider: "claude",
      apiKey: "test",
      model: "claude-sonnet-4-20250514",
    });

    await adapter.turn({
      systemPrompt: "sys",
      messages: [{ role: "user", content: "hi" }],
      tools: [{ name: "test_tool", description: "desc", parameters: {} }],
      toolChoice: "auto",
    });

    const invokeArgs = mockInvoke.mock.calls[0][1];
    expect(invokeArgs.req.body_json.tools).toBeDefined();
    expect(invokeArgs.req.body_json.tool_choice).toEqual({ type: "auto" });
  });

  it("throws TransientProviderError on 429 or 5xx", async () => {
    const mockInvoke = vi.mocked(runtimeModule.runtimeInvoke);
    mockInvoke.mockResolvedValueOnce({
      status: 429,
      body: { error: { message: "Claude rate limit exceeded" } },
    });

    const adapter = new ClaudeAdapter({
      provider: "claude",
      apiKey: "test",
      model: "claude-sonnet-4-20250514",
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

  it("throws ToolsUnsupportedError when 400 indicates tool unsupported", async () => {
    const mockInvoke = vi.mocked(runtimeModule.runtimeInvoke);
    mockInvoke.mockResolvedValueOnce({
      status: 400,
      body: { error: { message: "tools is not supported for this model" } },
    });

    const adapter = new ClaudeAdapter({
      provider: "claude",
      apiKey: "test",
      model: "claude-sonnet-4-20250514",
    });

    await expect(
      adapter.turn({
        systemPrompt: "sys",
        messages: [{ role: "user", content: "hi" }],
        tools: [{ name: "t", description: "d", parameters: {} }],
        toolChoice: "auto",
      })
    ).rejects.toThrow(ToolsUnsupportedError);
  });

  it("cancels request when AbortSignal triggers", async () => {
    const mockInvoke = vi.mocked(runtimeModule.runtimeInvoke);
    const controller = new AbortController();

    mockInvoke.mockImplementationOnce(async () => {
      controller.abort();
      return { status: 200, body: { content: [{ type: "text", text: "done" }] } };
    });
    mockInvoke.mockResolvedValue({ status: 200, body: true });

    const adapter = new ClaudeAdapter({
      provider: "claude",
      apiKey: "test",
      model: "claude-sonnet-4-20250514",
    });

    await adapter.turn({
      systemPrompt: "sys",
      messages: [{ role: "user", content: "hi" }],
      tools: [],
      toolChoice: "auto",
      requestId: "claude-abort-test",
      signal: controller.signal,
    });

    expect(mockInvoke).toHaveBeenCalledWith(
      "cancel_llm_request",
      expect.objectContaining({ requestId: "claude-abort-test" })
    );
  });
});
