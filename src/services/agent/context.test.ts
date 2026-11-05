import { describe, it, expect } from "vitest";
import {
  truncateToolResult,
  summarizeToolContent,
  compactOldToolResults,
  generateDocumentOutline,
  executeReadDocument,
  buildDocumentContext,
  DOC_MARKDOWN_THRESHOLD,
} from "./context.ts";
import type { NeutralMessage } from "./types.ts";
import type { TableCoordinateMeta } from "../../types/hwp.ts";

describe("context.ts - Agent Context Helpers", () => {
  describe("truncateToolResult", () => {
    it("returns short text as is", () => {
      const text = "짧은 도구 결과";
      expect(truncateToolResult(text, 100)).toBe(text);
    });

    it("truncates text exceeding maxLength and appends omission notice", () => {
      const longText = "A".repeat(5000);
      const truncated = truncateToolResult(longText, 4000);
      expect(truncated.length).toBeLessThan(longText.length);
      expect(truncated.startsWith("A".repeat(4000))).toBe(true);
      expect(truncated).toContain("(생략 1000자)");
    });
  });

  describe("summarizeToolContent", () => {
    it("summarizes JSON with summary field", () => {
      const json = JSON.stringify({ summary: "표 1 레이아웃 최적화 완료", resolvedCount: 3 });
      expect(summarizeToolContent(json)).toBe("[이전 도구 실행 결과 축약] 요약: 표 1 레이아웃 최적화 완료");
    });

    it("summarizes JSON with error field", () => {
      const json = JSON.stringify({ error: "Table 99 not found" });
      expect(summarizeToolContent(json)).toBe("[이전 도구 실행 결과 축약] 오류: Table 99 not found");
    });

    it("summarizes multiline plain text to single line snippet", () => {
      const text = "첫 번째 줄의 중요한 내용입니다.\n두 번째 줄 세부사항\n세 번째 줄";
      const summarized = summarizeToolContent(text);
      expect(summarized).toContain("[이전 도구 실행 결과 축약]");
      expect(summarized).toContain("첫 번째 줄의 중요한 내용입니다.");
      expect(summarized).not.toContain("\n");
    });
  });

  describe("compactOldToolResults", () => {
    it("does not compact tool results if assistant tool turns <= keepRecentTurns", () => {
      const messages: NeutralMessage[] = [
        { role: "user", content: "질문" },
        {
          role: "assistant",
          content: null,
          toolCalls: [{ id: "call_1", name: "inspect_layout", argumentsJson: "{}" }],
        },
        { role: "tool", toolCallId: "call_1", content: "상세 결과 1" },
        {
          role: "assistant",
          content: null,
          toolCalls: [{ id: "call_2", name: "inspect_layout", argumentsJson: "{}" }],
        },
        { role: "tool", toolCallId: "call_2", content: "상세 결과 2" },
      ];

      const compacted = compactOldToolResults(messages, 3);
      expect(compacted).toEqual(messages);
    });

    it("compacts older tool results while keeping recent turns intact and preserving toolCallIds", () => {
      const messages: NeutralMessage[] = [
        // Turn 1
        {
          role: "assistant",
          content: null,
          toolCalls: [{ id: "call_1", name: "inspect_layout", argumentsJson: "{}" }],
        },
        { role: "tool", toolCallId: "call_1", content: "오래된 긴 상세 결과 1\n여러 줄의 내용\n..." },
        // Turn 2
        {
          role: "assistant",
          content: null,
          toolCalls: [{ id: "call_2", name: "inspect_layout", argumentsJson: "{}" }],
        },
        { role: "tool", toolCallId: "call_2", content: "오래된 긴 상세 결과 2\n여러 줄의 내용\n..." },
        // Turn 3
        {
          role: "assistant",
          content: null,
          toolCalls: [{ id: "call_3", name: "inspect_layout", argumentsJson: "{}" }],
        },
        { role: "tool", toolCallId: "call_3", content: "최근 결과 3" },
        // Turn 4
        {
          role: "assistant",
          content: null,
          toolCalls: [{ id: "call_4", name: "inspect_layout", argumentsJson: "{}" }],
        },
        { role: "tool", toolCallId: "call_4", content: "최근 결과 4" },
      ];

      // Keep recent 2 turns (Turn 3, Turn 4)
      const compacted = compactOldToolResults(messages, 2);

      // Turn 1 & Turn 2 tool messages should be compacted
      expect(compacted[1].role).toBe("tool");
      if (compacted[1].role === "tool") {
        expect(compacted[1].toolCallId).toBe("call_1");
        expect(compacted[1].content).toContain("[이전 도구 실행 결과 축약]");
      }

      if (compacted[3].role === "tool") {
        expect(compacted[3].toolCallId).toBe("call_2");
        expect(compacted[3].content).toContain("[이전 도구 실행 결과 축약]");
      }

      // Recent Turn 3 & 4 should be unchanged
      if (compacted[5].role === "tool") {
        expect(compacted[5].toolCallId).toBe("call_3");
        expect(compacted[5].content).toBe("최근 결과 3");
      }
      if (compacted[7].role === "tool") {
        expect(compacted[7].toolCallId).toBe("call_4");
        expect(compacted[7].content).toBe("최근 결과 4");
      }
    });
  });

  describe("executeReadDocument", () => {
    const mockMarkdown = [
      "# 첫 번째 섹션",
      "첫 번째 문단입니다. 내용이 들어갑니다.",
      "두 번째 문단입니다. 자세한 설명이 있습니다.",
      "세 번째 문단입니다. 결론 부분입니다.",
    ].join("\n\n");

    const mockTablesMeta: TableCoordinateMeta[] = [
      {
        tableIndex: 0,
        sec: 0,
        para: 1,
        ctrl: 0,
        rowCount: 2,
        colCount: 2,
        cells: [
          { idx: 0, row: 0, col: 0, rowSpan: 1, colSpan: 1, text: "항목" },
          { idx: 1, row: 0, col: 1, rowSpan: 1, colSpan: 1, text: "금액" },
          { idx: 2, row: 1, col: 0, rowSpan: 1, colSpan: 1, text: "인건비" },
          { idx: 3, row: 1, col: 1, rowSpan: 1, colSpan: 1, text: "10,000" },
        ],
      },
    ];

    it("reads paragraphs by start and count", () => {
      const resJson = executeReadDocument({ kind: "paragraphs", start: 1, count: 2 }, mockMarkdown);
      const res = JSON.parse(resJson);
      expect(res.ok).toBe(true);
      expect(res.kind).toBe("paragraphs");
      expect(res.start).toBe(1);
      expect(res.paragraphs.length).toBe(2);
      expect(res.paragraphs[0].index).toBe(1);
      expect(res.paragraphs[0].text).toContain("첫 번째 문단");
      expect(res.paragraphs[1].index).toBe(2);
      expect(res.paragraphs[1].text).toContain("두 번째 문단");
    });

    it("returns error when paragraph start index is out of bounds", () => {
      const resJson = executeReadDocument({ kind: "paragraphs", start: 10, count: 2 }, mockMarkdown);
      const res = JSON.parse(resJson);
      expect(res.ok).toBe(false);
      expect(res.error).toContain("문단 범위 초과");
    });

    it("reads table cells by row range", () => {
      const resJson = executeReadDocument(
        { kind: "table", tableIndex: 0, start: 0, count: 2 },
        mockMarkdown,
        mockTablesMeta
      );
      const res = JSON.parse(resJson);
      expect(res.ok).toBe(true);
      expect(res.kind).toBe("table");
      expect(res.tableIndex).toBe(0);
      expect(res.rows.length).toBe(2);
      expect(res.rows[0].cells[0].text).toBe("항목");
      expect(res.rows[1].cells[1].text).toBe("10,000");
    });

    it("returns error when table is not found", () => {
      const resJson = executeReadDocument(
        { kind: "table", tableIndex: 99 },
        mockMarkdown,
        mockTablesMeta
      );
      const res = JSON.parse(resJson);
      expect(res.ok).toBe(false);
      expect(res.error).toContain("표 99를 찾을 수 없습니다");
    });
  });

  describe("buildDocumentContext & generateDocumentOutline", () => {
    it("includes full markdown when document length <= 12,000 characters", () => {
      const shortMd = "# 보고서\n\n짧은 내용입니다.";
      const ctx = buildDocumentContext({
        docMarkdown: shortMd,
        userPrompt: "수정해줘",
      });
      expect(ctx).toContain("### [문서 전체 마크다운 내용]");
      expect(ctx).toContain(shortMd);
      expect(ctx).not.toContain("12,000자를 초과");
    });

    it("generates outline and suppresses full text when length > 12,000 characters", () => {
      const longMd = "# 대형 보고서\n\n" + "내용 ".repeat(5000);
      expect(longMd.length).toBeGreaterThan(DOC_MARKDOWN_THRESHOLD);

      const ctx = buildDocumentContext({
        docMarkdown: longMd,
        userPrompt: "수정해줘",
      });
      expect(ctx).not.toContain("### [문서 전체 마크다운 내용]");
      expect(ctx).toContain("### [문서 개요 - 주요 제목 구조]");
      expect(ctx).toContain("read_document");
      expect(ctx).toContain("12,000자를 초과");
    });
  });
});
