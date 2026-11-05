import { createProviderAdapter } from "../services/agent/adapters/index.ts";
import { normalizeMemoryBody } from "./frontmatter.ts";
import { computeSha256_8, maskMemorySourceText, scanAndRedactPii } from "./piiPatterns.ts";
import type {
  GenerateMemoryArgs,
  GenerateMemoryResult,
  MemoryDoc,
  SourceDocRef,
} from "./types.ts";

export const MAX_INPUT_DOCS = 10;
export const MAX_CHARS_PER_DOC = 30_000;
export const MAX_TOTAL_INPUT_CHARS = 80_000;

export const MEMORY_GENERATION_SYSTEM_PROMPT = `당신은 한국어 공문서·보고서·업무 양식의 작성 스타일과 규칙을 추출하는 전문가입니다.
제공된 예시 문서들의 공통 문체, 어투, 용어 표기, 서식·표 구성 규칙, 제목 번호 체계, 지양 표현을 분석하여 재사용 가능한 마크다운 스타일 가이드(메모리)를 작성하십시오.

[절대 준수 규칙]
1. 반드시 아래 7개 H2(##) 섹션 제목을 정확한 이름과 순서로 모두 출력하십시오(YAML frontmatter나 코드펜스는 출력하지 마십시오):
## 문서 유형·양식
## 어투·문체
## 용어·표기
## 서식·표 규칙
## 구조(제목 체계)
## 금지·지양
## 예시 문구(일반화됨)
2. 개인정보(실명, 주민번호, 전화번호, 이메일, 계좌번호, 주소, 기관 고유 사건번호 등)는 절대 원문 그대로 인용하지 마십시오. 예시 문구에는 반드시 <이름>, <부서명>, <일자>, <금액>, <주소>, <전화번호> 같은 일반화된 플레이스홀더만 사용하십시오.
3. 문서의 구체적 사실관계(특정 사업명·수치)가 아니라 '반복 적용할 작성 양식·문체·서식 규칙'만 간결한 개조식(- 항목)으로 정리하십시오.`;

export async function generateMemory(
  args: GenerateMemoryArgs
): Promise<GenerateMemoryResult> {
  const trimmedName = args.name?.trim();
  if (!trimmedName) {
    throw new Error("작성 선호사항 이름을 입력해 주세요.");
  }

  const validDocs = (args.docs || []).filter(
    (d) => d && typeof d.text === "string" && d.text.trim().length > 0
  );
  if (validDocs.length === 0) {
    throw new Error(
      "분석할 문서 텍스트가 없습니다. 내용이 있는 문서를 선택해 주세요."
    );
  }

  const warnings: string[] = [];

  let inputDocsToUse = validDocs;
  if (inputDocsToUse.length > MAX_INPUT_DOCS) {
    inputDocsToUse = inputDocsToUse.slice(0, MAX_INPUT_DOCS);
    warnings.push("최대 10개 문서까지만 반영되어 나머지 문서는 제외되었습니다.");
  }

  const sourceDocs: SourceDocRef[] = [];
  const processedDocs: Array<{ name: string; text: string }> = [];
  let inputRedactionCount = 0;
  let accumulatedChars = 0;

  for (const doc of inputDocsToUse) {
    const sha256_8 = computeSha256_8(doc.text);
    sourceDocs.push({ name: doc.name, sha256_8 });

    let slicedText = doc.text;
    if (slicedText.length > MAX_CHARS_PER_DOC) {
      slicedText = slicedText.slice(0, MAX_CHARS_PER_DOC);
      warnings.push(
        `"${doc.name}" 문서가 30,000자를 초과하여 앞부분만 반영되었습니다.`
      );
    }

    const masked = maskMemorySourceText(slicedText);
    inputRedactionCount += masked.totalCount;

    if (accumulatedChars + masked.text.length > MAX_TOTAL_INPUT_CHARS) {
      const allowed = MAX_TOTAL_INPUT_CHARS - accumulatedChars;
      if (allowed > 0) {
        processedDocs.push({
          name: doc.name,
          text: masked.text.slice(0, allowed),
        });
        accumulatedChars += allowed;
      }
      warnings.push(
        "전체 입력 텍스트가 80,000자 상한에 도달하여 일부 내용이 절단되었습니다."
      );
      break;
    } else {
      processedDocs.push({
        name: doc.name,
        text: masked.text,
      });
      accumulatedChars += masked.text.length;
    }
  }

  if (inputRedactionCount > 0) {
    warnings.push(
      `입력 문서에서 개인정보 패턴 ${inputRedactionCount}건을 사전 비식별화하여 전송했습니다.`
    );
  }

  const docsText = processedDocs
    .map((d, i) => `### [문서 ${i + 1}: ${d.name}]\n${d.text}`)
    .join("\n\n");
  const userPrompt = `아래 ${processedDocs.length}개 예시 문서들의 공통 작성 스타일, 어투, 용어 표기, 서식/표 규칙, 제목 체계, 지양 표현을 분석하여 마크다운 스타일 가이드(메모리)를 작성해 주십시오.\n\n${docsText}`;

  const adapter = createProviderAdapter(args.provider);
  const resp = await adapter.turn({
    systemPrompt: MEMORY_GENERATION_SYSTEM_PROMPT,
    messages: [{ role: "user", content: userPrompt }],
    tools: [],
    toolChoice: "none",
    reasoningEffort: "medium",
    timeoutMs: 180_000,
    signal: args.signal,
  });

  if (!resp.content || resp.content.trim().length === 0) {
    throw new Error("AI 모델로부터 작성 선호사항 응답을 받지 못했습니다.");
  }

  const normalized = normalizeMemoryBody(resp.content);
  const outputScan = scanAndRedactPii(normalized);
  const outputRedactionCount = outputScan.totalCount;

  if (outputRedactionCount > 0) {
    warnings.push(
      `생성 결과에 남은 개인정보 패턴 ${outputRedactionCount}건(${outputScan.hits.join(", ")})을 플레이스홀더로 자동 치환했습니다.`
    );
  }

  const now = new Date().toISOString();
  const doc: MemoryDoc = {
    meta: {
      schemaVersion: 1,
      id: `mem_${crypto.randomUUID()}`,
      name: trimmedName,
      description: (args.description || "").trim(),
      revision: 1,
      createdAt: now,
      updatedAt: now,
      sourceDocs,
      masked: true,
    },
    body: outputScan.text,
  };

  return {
    doc,
    warnings,
    inputRedactionCount,
    outputRedactionCount,
  };
}
