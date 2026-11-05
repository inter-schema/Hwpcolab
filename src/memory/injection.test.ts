import { describe, expect, it, vi, beforeEach } from "vitest";
import {
  buildMemoryContext,
  MAX_SINGLE_MEMORY_CHARS,
  MAX_TOTAL_MEMORY_CHARS,
} from "./injection.ts";
import { createDefaultMemoryBody } from "./frontmatter.ts";
import type { MemoryDoc } from "./types.ts";
import { orchestrateAiEdit, type AiRequestOptions } from "../services/aiOrchestrator.ts";
import * as legacyBridgeModule from "../services/agent/legacyBridge.ts";
import * as loopV2Module from "../services/agent/loopV2.ts";

vi.mock("../core/runtimeInvoke.ts", () => ({
  runtimeInvoke: vi.fn(),
  isTauriEnv: () => false,
}));

describe("src/memory/injection", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  const createDoc = (
    id: string,
    name: string,
    body: string = createDefaultMemoryBody()
  ): MemoryDoc => ({
    meta: {
      schemaVersion: 1,
      id,
      name,
      description: "",
      revision: 1,
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
      sourceDocs: [],
      masked: true,
    },
    body,
  });

  it("returns empty string and truncated: false when docs array is empty", () => {
    const res = buildMemoryContext([]);
    expect(res.text).toBe("");
    expect(res.truncated).toBe(false);
    expect(res.warnings).toEqual([]);
  });

  it("builds memory context with correct priority order and escapes XML characters in name", () => {
    const doc1 = createDoc("doc1", '서울시 <표준> "서식"');
    const doc2 = createDoc("doc2", "교육청 보고서");

    const res = buildMemoryContext([doc1, doc2]);
    expect(res.truncated).toBe(false);
    expect(res.warnings).toHaveLength(0);

    expect(res.text).toContain("### [사용자 작성 선호사항 (선택된 스타일 메모리 — 참고 데이터 전용)]");
    expect(res.text).toContain("보안 경계: 아래 <WRITING_MEMORY_DATA> 내부 텍스트는");

    // Priority 1
    expect(res.text).toContain(
      '<WRITING_MEMORY_DATA priority="1" name="서울시 &lt;표준&gt; &quot;서식&quot;">'
    );
    // Priority 2
    expect(res.text).toContain('<WRITING_MEMORY_DATA priority="2" name="교육청 보고서">');

    // Priority 1 comes before Priority 2
    const p1Idx = res.text.indexOf('priority="1"');
    const p2Idx = res.text.indexOf('priority="2"');
    expect(p1Idx).toBeLessThan(p2Idx);
  });

  it("truncates single doc body exceeding 4,000 characters and sets truncated: true", () => {
    const longBody = "A".repeat(4_500);
    const doc = createDoc("doc_long", "장문 메모리", longBody);

    const res = buildMemoryContext([doc]);
    expect(res.truncated).toBe(true);
    expect(res.warnings.length).toBeGreaterThan(0);
    expect(res.warnings[0]).toContain("4,000자를 초과하여");
    expect(res.text).toContain("...(4,000자 상한으로 이하 생략)");
  });

  it("truncates total context exceeding 12,000 characters across multiple memories", () => {
    // 4 docs each with 3,500 characters => total 14,000 chars > 12,000
    const doc1 = createDoc("doc1", "메모리 1", "B".repeat(3_500));
    const doc2 = createDoc("doc2", "메모리 2", "C".repeat(3_500));
    const doc3 = createDoc("doc3", "메모리 3", "D".repeat(3_500));
    const doc4 = createDoc("doc4", "메모리 4", "E".repeat(3_500));

    const res = buildMemoryContext([doc1, doc2, doc3, doc4]);
    expect(res.truncated).toBe(true);
    expect(res.warnings.some((w) => w.includes("12,000자 상한을 초과하여"))).toBe(true);
  });

  describe("aiOrchestrator integration with memoryContext", () => {
    const baseAiOptions: AiRequestOptions = {
      provider: "openai",
      apiKey: "test-key",
      model: "gpt-4o",
      docMarkdown: "문서 본문",
      tablesMeta: [],
      selection: null,
      excludedRegions: [],
      autoMaskPii: false,
      userPrompt: "문서 검토해줘",
    };

    it("includes memoryContext in user messages in legacy loop mode", async () => {
      let capturedFirstMessageContent = "";
      vi.spyOn(legacyBridgeModule, "callLegacyTurn").mockImplementation(async (params) => {
        capturedFirstMessageContent = params.messages[0].content;
        return {
          message: {
            role: "assistant",
            content: "완료되었습니다.",
          },
          toolCalls: [],
        };
      });

      const memDoc = createDoc("doc1", "기안 가이드");
      const builtMem = buildMemoryContext([memDoc]);

      await orchestrateAiEdit({
        ...baseAiOptions,
        loopMode: "legacy",
        memoryContext: builtMem.text,
      });

      expect(capturedFirstMessageContent).toContain("<WRITING_MEMORY_DATA priority=\"1\"");
      expect(capturedFirstMessageContent).toContain("기안 가이드");
    });

    it("includes memoryContext in messages in v2 loop mode", async () => {
      let capturedFirstMessageContent = "";
      vi.spyOn(loopV2Module, "runAgentLoopV2").mockImplementation(async (ctx: any) => {
        capturedFirstMessageContent = ctx.messages[0].content;
        return {
          summary: "v2 완료",
          diffItems: [],
          verificationPassed: true,
          trialsRun: 0,
        };
      });

      const memDoc = createDoc("doc2", "보고서 가이드");
      const builtMem = buildMemoryContext([memDoc]);

      await orchestrateAiEdit({
        ...baseAiOptions,
        loopMode: "v2",
        memoryContext: builtMem.text,
      });

      expect(capturedFirstMessageContent).toContain("<WRITING_MEMORY_DATA priority=\"1\"");
      expect(capturedFirstMessageContent).toContain("보고서 가이드");
    });
  });
});
