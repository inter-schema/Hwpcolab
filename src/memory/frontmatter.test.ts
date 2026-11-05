import { describe, expect, it } from "vitest";
import {
  createDefaultMemoryBody,
  normalizeMemoryBody,
  parseMemoryMarkdown,
  parseMemorySections,
  serializeMemoryMarkdown,
  serializeMemorySections,
} from "./frontmatter.ts";
import { FIXED_MEMORY_SECTIONS, type MemoryDoc } from "./types.ts";

describe("src/memory/frontmatter", () => {
  it("createDefaultMemoryBody generates 7 fixed sections with placeholder", () => {
    const body = createDefaultMemoryBody();
    for (const title of FIXED_MEMORY_SECTIONS) {
      expect(body).toContain(`## ${title}`);
    }
  });

  it("handles round-trip serialization and parsing lossless (including quotes and colons in name)", () => {
    const doc: MemoryDoc = {
      meta: {
        schemaVersion: 1,
        id: "mem_test_123",
        name: '서울시 "공문서": 표준 [스타일]',
        description: "보고서: 결재 양식, '기안용'",
        revision: 3,
        createdAt: "2026-11-05T09:00:00.000Z",
        updatedAt: "2026-11-05T10:00:00.000Z",
        sourceDocs: [{ name: '공문서 "예시".hwpx', sha256_8: "1a2b3c4d" }],
        masked: true,
      },
      body: `## 문서 유형·양식
- 기안문 및 보고서

## 어투·문체
- 명확한 개조식

## 용어·표기
- 행정용어 준수

## 서식·표 규칙
- 표 상단 중앙 정렬

## 구조(제목 체계)
- 1. -> 가. -> 1)

## 금지·지양
- 모호한 피동형 지양

## 예시 문구(일반화됨)
- 추진 계획을 다음과 같이 보고함`,
    };

    const serialized = serializeMemoryMarkdown(doc);
    const parsed = parseMemoryMarkdown(serialized);

    expect(parsed.hadFrontmatter).toBe(true);
    expect(parsed.doc.meta.id).toBe(doc.meta.id);
    expect(parsed.doc.meta.name).toBe(doc.meta.name);
    expect(parsed.doc.meta.description).toBe(doc.meta.description);
    expect(parsed.doc.meta.revision).toBe(doc.meta.revision);
    expect(parsed.doc.meta.createdAt).toBe(doc.meta.createdAt);
    expect(parsed.doc.meta.updatedAt).toBe(doc.meta.updatedAt);
    expect(parsed.doc.meta.sourceDocs).toEqual(doc.meta.sourceDocs);
    expect(parsed.doc.meta.masked).toBe(true);
    expect(parsed.doc.body).toBe(normalizeMemoryBody(doc.body));
  });

  it("parses raw markdown without frontmatter, extracts H1 title, and fills missing sections", () => {
    const rawMd = `# 업무 매뉴얼 스타일 가이드

## 어투·문체
- 친절하고 명확한 문체 사용

## 금지·지양
- 비속어 및 줄임말 사용 금지
`;

    const { doc, hadFrontmatter } = parseMemoryMarkdown(rawMd);
    expect(hadFrontmatter).toBe(false);
    expect(doc.meta.name).toBe("업무 매뉴얼 스타일 가이드");
    expect(doc.meta.revision).toBe(1);
    expect(doc.meta.sourceDocs).toEqual([]);
    expect(doc.meta.masked).toBe(true);

    const sections = parseMemorySections(doc.body);
    expect(sections["어투·문체"]).toBe("- 친절하고 명확한 문체 사용");
    expect(sections["금지·지양"]).toBe("- 비속어 및 줄임말 사용 금지");
    expect(sections["문서 유형·양식"]).toBe("- (특별한 규칙 없음)");
    expect(sections["서식·표 규칙"]).toBe("- (특별한 규칙 없음)");
  });

  it("removes markdown code fences", () => {
    const wrapped = `\`\`\`markdown
## 문서 유형·양식
- 회의록

## 어투·문체
- 간결체
\`\`\``;

    const sections = parseMemorySections(wrapped);
    expect(sections["문서 유형·양식"]).toBe("- 회의록");
    expect(sections["어투·문체"]).toBe("- 간결체");
  });

  it("merges unrecognized sections into '서식·표 규칙'", () => {
    const raw = `## 어투·문체
- 간결체

## 기타 참고사항
- 특별 부록 표 서식 참고할 것
`;

    const sections = parseMemorySections(raw);
    expect(sections["어투·문체"]).toBe("- 간결체");
    expect(sections["서식·표 규칙"]).toContain("기타 참고사항");
    expect(sections["서식·표 규칙"]).toContain("- 특별 부록 표 서식 참고할 것");
  });
});
