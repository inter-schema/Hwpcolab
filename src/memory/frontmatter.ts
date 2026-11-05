import {
  FIXED_MEMORY_SECTIONS,
  type MemoryDoc,
  type MemorySectionTitle,
  type SourceDocRef,
} from "./types.ts";

export function createDefaultMemoryBody(): string {
  return FIXED_MEMORY_SECTIONS.map((title) => `## ${title}\n- (내용 없음)`).join(
    "\n\n"
  );
}

function matchSectionTitle(rawTitle: string): MemorySectionTitle | null {
  const cleaned = rawTitle.replace(/^\d+[\.\)]\s*/, "").trim();
  const normalized = cleaned.replace(/\s+/g, "");

  for (const fixed of FIXED_MEMORY_SECTIONS) {
    if (cleaned === fixed || normalized === fixed.replace(/\s+/g, "")) {
      return fixed;
    }
  }

  if (normalized.includes("문서유형") || normalized.includes("양식")) {
    return "문서 유형·양식";
  }
  if (normalized.includes("어투") || normalized.includes("문체")) {
    return "어투·문체";
  }
  if (normalized.includes("용어") || normalized.includes("표기")) {
    return "용어·표기";
  }
  if (normalized.includes("서식") || normalized.includes("표규칙")) {
    return "서식·표 규칙";
  }
  if (normalized.includes("구조") || normalized.includes("제목체계")) {
    return "구조(제목 체계)";
  }
  if (normalized.includes("금지") || normalized.includes("지양")) {
    return "금지·지양";
  }
  if (normalized.includes("예시문구") || normalized.includes("예시")) {
    return "예시 문구(일반화됨)";
  }

  return null;
}

export function parseMemorySections(
  body: string
): Record<MemorySectionTitle, string> {
  let cleaned = body.trim();

  // Remove code fences if wrapped
  cleaned = cleaned.replace(/^```(?:markdown)?\s*\r?\n([\s\S]*?)\r?\n```\s*$/g, "$1");

  // Remove leading frontmatter if present
  cleaned = cleaned.replace(/^---\r?\n[\s\S]*?\r?\n---\r?\n?/, "");

  const sections: Record<MemorySectionTitle, string[]> = {
    "문서 유형·양식": [],
    "어투·문체": [],
    "용어·표기": [],
    "서식·표 규칙": [],
    "구조(제목 체계)": [],
    "금지·지양": [],
    "예시 문구(일반화됨)": [],
  };

  const lines = cleaned.split(/\r?\n/);
  let currentTitle: MemorySectionTitle | null = null;
  const unmappedLines: string[] = [];

  for (const line of lines) {
    if (/^#\s+/.test(line)) {
      // Document title (H1) line: skip
      continue;
    }
    const headingMatch = line.match(/^##\s+(.+?)\s*$/);
    if (headingMatch) {
      const parsedTitle = matchSectionTitle(headingMatch[1]);
      if (parsedTitle) {
        currentTitle = parsedTitle;
      } else {
        // Unrecognized section: treat as unmapped
        currentTitle = null;
        unmappedLines.push(line);
      }
    } else {
      if (currentTitle) {
        sections[currentTitle].push(line);
      } else if (line.trim().length > 0) {
        unmappedLines.push(line);
      }
    }
  }

  if (unmappedLines.length > 0) {
    const extraContent = unmappedLines.join("\n").trim();
    if (extraContent) {
      sections["서식·표 규칙"].push(extraContent);
    }
  }

  const result = {} as Record<MemorySectionTitle, string>;
  for (const title of FIXED_MEMORY_SECTIONS) {
    const content = sections[title].join("\n").trim();
    result[title] = content || "- (특별한 규칙 없음)";
  }

  return result;
}

export function serializeMemorySections(
  sections: Record<MemorySectionTitle, string>
): string {
  return FIXED_MEMORY_SECTIONS.map((title) => {
    const content = sections[title]?.trim() || "- (특별한 규칙 없음)";
    return `## ${title}\n${content}`;
  }).join("\n\n");
}

export function normalizeMemoryBody(rawBody: string): string {
  return serializeMemorySections(parseMemorySections(rawBody));
}

export function serializeMemoryMarkdown(doc: MemoryDoc): string {
  const normBody = normalizeMemoryBody(doc.body);
  return `---
schemaVersion: 1
id: ${JSON.stringify(doc.meta.id)}
name: ${JSON.stringify(doc.meta.name)}
description: ${JSON.stringify(doc.meta.description)}
revision: ${doc.meta.revision}
createdAt: ${JSON.stringify(doc.meta.createdAt)}
updatedAt: ${JSON.stringify(doc.meta.updatedAt)}
sourceDocs: ${JSON.stringify(doc.meta.sourceDocs)}
masked: true
---

${normBody}`;
}

export function parseMemoryMarkdown(rawMd: string): {
  doc: MemoryDoc;
  hadFrontmatter: boolean;
} {
  const trimmed = rawMd.trim();
  const fmMatch = trimmed.match(/^---\r?\n([\s\S]*?)\r?\n---\r?\n?([\s\S]*)$/);

  if (fmMatch) {
    const fmLines = fmMatch[1].split(/\r?\n/);
    const bodyPart = fmMatch[2] || "";

    const parsedData: Record<string, any> = {};
    for (const line of fmLines) {
      const colonIdx = line.indexOf(":");
      if (colonIdx === -1) continue;
      const key = line.slice(0, colonIdx).trim();
      const valStr = line.slice(colonIdx + 1).trim();

      if (
        valStr.startsWith('"') ||
        valStr.startsWith("[") ||
        valStr.startsWith("{")
      ) {
        try {
          parsedData[key] = JSON.parse(valStr);
        } catch {
          parsedData[key] = valStr.replace(/^["']|["']$/g, "");
        }
      } else if (valStr === "true") {
        parsedData[key] = true;
      } else if (valStr === "false") {
        parsedData[key] = false;
      } else if (/^\d+$/.test(valStr)) {
        parsedData[key] = parseInt(valStr, 10);
      } else {
        parsedData[key] = valStr;
      }
    }

    const doc: MemoryDoc = {
      meta: {
        schemaVersion: 1,
        id: String(parsedData.id || `mem_${crypto.randomUUID()}`),
        name: String(parsedData.name || "가져온 작성 선호사항"),
        description: String(parsedData.description || ""),
        revision: typeof parsedData.revision === "number" ? parsedData.revision : 1,
        createdAt: String(parsedData.createdAt || new Date().toISOString()),
        updatedAt: String(parsedData.updatedAt || new Date().toISOString()),
        sourceDocs: Array.isArray(parsedData.sourceDocs)
          ? (parsedData.sourceDocs as SourceDocRef[])
          : [],
        masked: true,
      },
      body: normalizeMemoryBody(bodyPart),
    };

    return { doc, hadFrontmatter: true };
  }

  // No frontmatter
  let detectedName = "가져온 작성 선호사항";
  for (const line of trimmed.split(/\r?\n/)) {
    const h1Match = line.match(/^#\s+(.+?)\s*$/);
    if (h1Match) {
      detectedName = h1Match[1].trim();
      break;
    }
  }

  const doc: MemoryDoc = {
    meta: {
      schemaVersion: 1,
      id: `mem_${crypto.randomUUID()}`,
      name: detectedName,
      description: "",
      revision: 1,
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
      sourceDocs: [],
      masked: true,
    },
    body: normalizeMemoryBody(trimmed),
  };

  return { doc, hadFrontmatter: false };
}
