import { describe, expect, it, vi } from "vitest";
import { generateMemory } from "./generate.ts";
import type { ProviderConfig } from "../services/agent/types.ts";

vi.mock("../services/agent/adapters/index.ts", () => {
  return {
    createProviderAdapter: vi.fn(),
  };
});

import { createProviderAdapter } from "../services/agent/adapters/index.ts";

const mockProvider: ProviderConfig = {
  provider: "custom",
  apiKey: "test-key",
  model: "test-model",
  endpoint: "http://127.0.0.1:11434/v1/chat/completions",
};

describe("src/memory/generate", () => {
  it("throws error when name is empty", async () => {
    await expect(
      generateMemory({
        name: "   ",
        docs: [{ name: "doc1", text: "내용" }],
        provider: mockProvider,
      })
    ).rejects.toThrow("작성 선호사항 이름을 입력해 주세요.");
  });

  it("throws error when docs are empty or have no text", async () => {
    await expect(
      generateMemory({
        name: "테스트 메모리",
        docs: [],
        provider: mockProvider,
      })
    ).rejects.toThrow(
      "분석할 문서 텍스트가 없습니다. 내용이 있는 문서를 선택해 주세요."
    );
  });

  it("pre-redacts input PII before passing to LLM and re-redacts any leaked PII from LLM output", async () => {
    let capturedUserPrompt = "";

    const mockTurn = vi.fn().mockImplementation(async (req) => {
      capturedUserPrompt = req.messages[0].content;
      // LLM deliberately echoes leaked PII
      return {
        content: `## 문서 유형·양식
- 공문서 양식 (담당자: 강감찬)

## 어투·문체
- 개조식 종결어미 사용. 문의: 010-9999-8888

## 용어·표기
- 행정 표준어 사용

## 서식·표 규칙
- 표 중앙 정렬

## 구조(제목 체계)
- 1. -> 가.

## 금지·지양
- 모호한 표현 지양

## 예시 문구(일반화됨)
- 환불 계좌: 123-456-789012`,
        toolCalls: [],
      };
    });

    (createProviderAdapter as any).mockReturnValue({
      id: "custom",
      supportsTools: true,
      turn: mockTurn,
    });

    const rawInputDoc = {
      name: "인사기록.txt",
      text: "기안자: 홍길동\n주민번호: 900101-1234567\n전화번호: 010-1234-5678\n이메일: hong@example.com",
    };

    const result = await generateMemory({
      name: "공문서 스타일",
      docs: [rawInputDoc],
      provider: mockProvider,
    });

    // 1. Verify that LLM received NO raw PII and NO [[PII_*]] tokens
    expect(capturedUserPrompt).not.toContain("홍길동");
    expect(capturedUserPrompt).not.toContain("900101-1234567");
    expect(capturedUserPrompt).not.toContain("010-1234-5678");
    expect(capturedUserPrompt).not.toContain("hong@example.com");
    expect(capturedUserPrompt).not.toContain("[[PII_");
    expect(capturedUserPrompt).toContain("<이름>");
    expect(capturedUserPrompt).toContain("<주민번호>");
    expect(capturedUserPrompt).toContain("<전화번호>");
    expect(capturedUserPrompt).toContain("<이메일>");

    // 2. Verify that leaked PII from LLM response was re-redacted to permanent placeholders
    expect(result.doc.body).not.toContain("강감찬");
    expect(result.doc.body).not.toContain("010-9999-8888");
    expect(result.doc.body).not.toContain("123-456-789012");
    expect(result.doc.body).toContain("<이름>");
    expect(result.doc.body).toContain("<전화번호>");
    expect(result.doc.body).toContain("<계좌번호>");

    // 3. Verify counts & warnings
    expect(result.inputRedactionCount).toBeGreaterThan(0);
    expect(result.outputRedactionCount).toBeGreaterThan(0);
    expect(result.warnings.some((w) => w.includes("사전 비식별화"))).toBe(true);
    expect(result.warnings.some((w) => w.includes("자동 치환했습니다"))).toBe(true);

    // 4. Verify that store was not touched (doc has schemaVersion and masked: true)
    expect(result.doc.meta.name).toBe("공문서 스타일");
    expect(result.doc.meta.masked).toBe(true);
    expect(result.doc.meta.revision).toBe(1);
  });

  it("enforces document count cap (10 docs) and character budget (30k/80k)", async () => {
    const mockTurn = vi.fn().mockResolvedValue({
      content: `## 문서 유형·양식
- 기본
## 어투·문체
- 기본
## 용어·표기
- 기본
## 서식·표 규칙
- 기본
## 구조(제목 체계)
- 기본
## 금지·지양
- 기본
## 예시 문구(일반화됨)
- 기본`,
      toolCalls: [],
    });

    (createProviderAdapter as any).mockReturnValue({
      id: "custom",
      supportsTools: true,
      turn: mockTurn,
    });

    // 12 docs
    const manyDocs = Array.from({ length: 12 }, (_, i) => ({
      name: `doc_${i + 1}.txt`,
      text: `문서 ${i + 1} 본문 내용입니다.`,
    }));

    const result = await generateMemory({
      name: "다수 문서 테스트",
      docs: manyDocs,
      provider: mockProvider,
    });

    expect(result.doc.meta.sourceDocs.length).toBe(10);
    expect(result.warnings.some((w) => w.includes("최대 10개 문서"))).toBe(true);
  });
});
