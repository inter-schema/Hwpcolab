import * as XLSX from "xlsx";
import { unzipSync, strFromU8 } from "fflate";
import { runtimeInvoke } from "./runtimeInvoke";

export interface ParsedAttachment {
  id: string;
  fileName: string;
  filePath: string;
  kind: "excel" | "docx" | "pdf" | "hwp" | "image" | "text";
  markdownSummary: string;
  structuredData?: any;
}

function parseDocxBytesToMarkdown(fileBytes: Uint8Array): string {
  const unzipped = unzipSync(fileBytes);
  const docXmlBytes = unzipped["word/document.xml"];
  if (!docXmlBytes) {
    throw new Error("word/document.xml을 찾을 수 없습니다.");
  }
  const xml = strFromU8(docXmlBytes);
  const paragraphs: string[] = [];
  const pMatches = xml.match(/<w:p[\s>][\s\S]*?<\/w:p>/g) || [];
  for (const pXml of pMatches) {
    const tMatches = pXml.match(/<w:t(?:\s[^>]*)?>([\s\S]*?)<\/w:t>/g) || [];
    const line = tMatches
      .map((t) => t.replace(/<[^>]+>/g, ""))
      .join("")
      .replace(/&amp;/g, "&")
      .replace(/&lt;/g, "<")
      .replace(/&gt;/g, ">")
      .replace(/&quot;/g, '"')
      .trim();
    if (line) {
      paragraphs.push(line);
    }
  }
  return paragraphs.join("\n");
}

export async function parseLocalAttachment(filePath: string, fileBytes: Uint8Array): Promise<ParsedAttachment> {
  const ext = filePath.split(".").pop()?.toLowerCase() || "";
  const fileName = filePath.split(/[\\/]/).pop() || filePath;

  // 1. Word (.docx) -> OXML word/document.xml 텍스트 추출
  if (ext === "docx") {
    try {
      const md = parseDocxBytesToMarkdown(fileBytes);
      return {
        id: crypto.randomUUID(),
        fileName,
        filePath,
        kind: "docx",
        markdownSummary: md || "[빈 DOCX 문서]",
      };
    } catch (e: any) {
      return {
        id: crypto.randomUUID(),
        fileName,
        filePath,
        kind: "docx",
        markdownSummary: `[DOCX 파싱 실패: ${e?.message || e}]`,
      };
    }
  }

  // 2. Excel (.xlsx, .xls, .csv) -> 시트별 마크다운 표 + 2D 배열 추출
  if (["xlsx", "xls", "csv"].includes(ext)) {
    try {
      const wb = XLSX.read(fileBytes, { type: "array" });
      const mdParts: string[] = [];
      const sheetsData: Record<string, any[][]> = {};

      for (const sheetName of wb.SheetNames) {
        const sheet = wb.Sheets[sheetName];
        const rows: any[][] = XLSX.utils.sheet_to_json(sheet, { header: 1, defval: "" });
        sheetsData[sheetName] = rows;
        if (rows.length > 0) {
          const header = rows[0].map((c) => String(c).replace(/\n/g, " "));
          const divider = header.map(() => "---");
          const body = rows.slice(1).map((r) => r.map((c) => String(c).replace(/\n/g, " ")));
          mdParts.push(
            `### [시트: ${sheetName}]\n` +
            `| ${header.join(" | ")} |\n| ${divider.join(" | ")} |\n` +
            body.map((r) => `| ${r.join(" | ")} |`).join("\n")
          );
        }
      }
      return {
        id: crypto.randomUUID(),
        fileName,
        filePath,
        kind: "excel",
        markdownSummary: mdParts.join("\n\n"),
        structuredData: sheetsData,
      };
    } catch (e: any) {
      return {
        id: crypto.randomUUID(),
        fileName,
        filePath,
        kind: "excel",
        markdownSummary: `[엑셀 파싱 실패: ${e?.message || e}]`,
      };
    }
  }

  // 3. 다른 HWP / HWPX 파일 -> claw-hwp extract_text.js로 마크다운 + 표 구조 추출
  if (["hwp", "hwpx"].includes(ext)) {
    try {
      const md = await runtimeInvoke<string>("extract_markdown", { filePath });
      let inspect: any = null;
      try {
        inspect = await runtimeInvoke<any>("inspect_document", { filePath });
      } catch {}
      return {
        id: crypto.randomUUID(),
        fileName,
        filePath,
        kind: "hwp",
        markdownSummary: md,
        structuredData: inspect,
      };
    } catch (e: any) {
      return {
        id: crypto.randomUUID(),
        fileName,
        filePath,
        kind: "hwp",
        markdownSummary: `[HWP 파싱 실패: ${e?.message || e}]`,
      };
    }
  }

  // 4. PDF -> pdfjs-dist (로컬 번들 워커 사용하여 완전 오프라인 동작 보장)
  if (ext === "pdf") {
    try {
      const pdfjsLib = await import("pdfjs-dist");
      if ((pdfjsLib as any).GlobalWorkerOptions && !(pdfjsLib as any).GlobalWorkerOptions.workerSrc) {
        try {
          (pdfjsLib as any).GlobalWorkerOptions.workerSrc = new URL(
            "pdfjs-dist/build/pdf.worker.min.mjs",
            import.meta.url
          ).toString();
        } catch {}
      }
      const loadingTask = pdfjsLib.getDocument({ data: fileBytes });
      const pdf = await loadingTask.promise;
      const pagesText: string[] = [];
      for (let i = 1; i <= Math.min(pdf.numPages, 30); i++) {
        const page = await pdf.getPage(i);
        const content = await page.getTextContent();
        const strings = content.items.map((item: any) => item.str);
        pagesText.push(`--- [PDF ${i}페이지] ---\n` + strings.join(" "));
      }
      return {
        id: crypto.randomUUID(),
        fileName,
        filePath,
        kind: "pdf",
        markdownSummary: pagesText.join("\n\n"),
      };
    } catch (e: any) {
      return {
        id: crypto.randomUUID(),
        fileName,
        filePath,
        kind: "pdf",
        markdownSummary: `[PDF 파싱 실패: ${e?.message || e}]`,
      };
    }
  }

  // 5. 일반 텍스트 (.txt, .md, .json)
  if (["txt", "md", "json"].includes(ext)) {
    const text = new TextDecoder().decode(fileBytes);
    return {
      id: crypto.randomUUID(),
      fileName,
      filePath,
      kind: "text",
      markdownSummary: text,
    };
  }

  // 6. 이미지 (.png, .jpg, .jpeg)
  return {
    id: crypto.randomUUID(),
    fileName,
    filePath,
    kind: "image",
    markdownSummary: `[로컬 이미지 파일 첨부됨: ${filePath} — insert_image 또는 place_seal의 path/source 인자로 직접 사용 가능]`,
  };
}
