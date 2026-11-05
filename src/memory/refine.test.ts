import { describe, expect, it, vi } from "vitest";
import {
  applySectionDecisions,
  computeSectionDiffs,
  proposeRefinement,
} from "./refine.ts";
import { createDefaultMemoryBody } from "./frontmatter.ts";
import type { MemoryDoc, SectionDiff } from "./types.ts";
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

describe("src/memory/refine", () => {
  const baseDoc: MemoryDoc = {
    meta: {
      schemaVersion: 1,
      id: "mem_refine_1",
      name: "공문서 스타일",
      description: "기본 설명",
      revision: 2,
      createdAt: "2026-11-05T00:00:00.000Z",
      updatedAt: "2026-11-05T01:00:00.000Z",
      sourceDocs: [{ name: "원문1.hwpx", sha256_8: "11223344" }],
      masked: true,
    },
    body: `## 문서 유형·양식
- 일반 기안문

## 어투·문체
- 표준 공문서체

## 용어·표기
- 행정 용어

## 서식·표 규칙
- 표 기본 여백

## 구조(제목 체계)
- 1. -> 가.

## 금지·지양
- 모호한 표현 금지

## 예시 문구(일반화됨)
- 붙임 문서를 참조 바람`,
  };

  it("computeSectionDiffs computes token diffs and changed flags per section", () => {
    const afterBody = baseDoc.body.replace(
      "## 어투·문체\n- 표준 공문서체",
      "## 어투·문체\n- 간결한 개조식 공문서체"
    );

    const diffs = computeSectionDiffs(baseDoc.body, afterBody);
    expect(diffs).toHaveLength(7);

    const toneDiff = diffs.find((d) => d.sectionTitle === "어투·문체");
    expect(toneDiff).toBeDefined();
    expect(toneDiff!.changed).toBe(true);
    expect(toneDiff!.tokens.some((t) => t.op === 1)).toBe(true); // addition

    const docTypeDiff = diffs.find((d) => d.sectionTitle === "문서 유형·양식");
    expect(docTypeDiff).toBeDefined();
    expect(docTypeDiff!.changed).toBe(false);
  });

  it("proposeRefinement pre-redacts instruction/extraDocs and re-redacts LLM output PII", async () => {
    let capturedPrompt = "";

    const mockTurn = vi.fn().mockImplementation(async (req) => {
      capturedPrompt = req.messages[0].content;
      return {
        content: `## 문서 유형·양식
- 일반 기안문 및 보고서 (신청인: 이순신)

## 어투·문체
- 극도로 간결한 개조식

## 용어·표기
- 행정 용어 준수

## 서식·표 규칙
- 표 테두리 실선 0.4mm

## 구조(제목 체계)
- 1. -> 가. -> 1)

## 금지·지양
- 피동 표현 금지

## 예시 문구(일반화됨)
- 연락처: 010-5555-6666`,
        toolCalls: [],
      };
    });

    (createProviderAdapter as any).mockReturnValue({
      id: "custom",
      supportsTools: true,
      turn: mockTurn,
    });

    const result = await proposeRefinement({
      doc: baseDoc,
      instruction: "기안자: 김유신 010-1111-2222의 요청에 맞춰 다듬어줘",
      extraDocs: [
        {
          name: "참고문서.txt",
          text: "참고인: 홍길동 계좌 110-123-456789 적용 요망",
        },
      ],
      provider: mockProvider,
    });

    // 1. Verify LLM received no raw PII
    expect(capturedPrompt).not.toContain("김유신");
    expect(capturedPrompt).not.toContain("010-1111-2222");
    expect(capturedPrompt).not.toContain("홍길동");
    expect(capturedPrompt).not.toContain("110-123-456789");
    expect(capturedPrompt).toContain("<이름>");
    expect(capturedPrompt).toContain("<전화번호>");
    expect(capturedPrompt).toContain("<계좌번호>");

    // 2. Verify leaked PII from LLM response was sanitized
    expect(result.proposedBody).not.toContain("이순신");
    expect(result.proposedBody).not.toContain("010-5555-6666");
    expect(result.proposedBody).toContain("<이름>");
    expect(result.proposedBody).toContain("<전화번호>");

    // 3. Verify section diffs and extra source docs
    expect(result.sectionDiffs.length).toBe(7);
    expect(result.extraSourceDocs.length).toBe(1);
    expect(result.extraSourceDocs[0].name).toBe("참고문서.txt");
  });

  it("applySectionDecisions partially merges accepted sections and retains unaccepted sections", () => {
    const afterBody = baseDoc.body
      .replace(
        "## 문서 유형·양식\n- 일반 기안문",
        "## 문서 유형·양식\n- 개정된 기안문 양식"
      )
      .replace(
        "## 어투·문체\n- 표준 공문서체",
        "## 어투·문체\n- 개조식 어투 적용"
      );

    const diffs = computeSectionDiffs(baseDoc.body, afterBody);

    // Accept only "어투·문체", reject "문서 유형·양식"
    const acceptedSections = ["어투·문체"];
    const extraDocs = [{ name: "학습2.hwpx", sha256_8: "55667788" }];

    const updatedDoc = applySectionDecisions(
      baseDoc,
      diffs,
      acceptedSections,
      extraDocs
    );

    // "어투·문체" should have afterContent
    expect(updatedDoc.body).toContain("- 개조식 어투 적용");
    // "문서 유형·양식" should remain beforeContent
    expect(updatedDoc.body).toContain("- 일반 기안문");
    expect(updatedDoc.body).not.toContain("- 개정된 기안문 양식");

    // Revision should remain same (will be bumped on store.save)
    expect(updatedDoc.meta.revision).toBe(baseDoc.meta.revision);

    // Source docs should be merged
    expect(updatedDoc.meta.sourceDocs).toHaveLength(2);
    expect(updatedDoc.meta.sourceDocs[0].name).toBe("원문1.hwpx");
    expect(updatedDoc.meta.sourceDocs[1].name).toBe("학습2.hwpx");
  });
});
