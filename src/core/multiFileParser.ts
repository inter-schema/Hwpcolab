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

const WORDPROCESSINGML_NAMESPACE = 'http://schemas.openxmlformats.org/wordprocessingml/2006/main';
const DOCX_BLOCK_CONTAINERS = new Set(['sdt', 'sdtContent', 'customXml', 'ins', 'moveTo', 'smartTag']);

function isWordElement(element: Element, localName: string): boolean {
  return element.namespaceURI === WORDPROCESSINGML_NAMESPACE && element.localName === localName;
}

function directWordChildren(parent: Element, localName: string): Element[] {
  return Array.from(parent.children).filter((child) => isWordElement(child, localName));
}

function extractWordParagraphText(node: Node): string {
  if (node.nodeType === Node.TEXT_NODE) return node.nodeValue ?? '';
  if (node.nodeType !== Node.ELEMENT_NODE) return '';

  const element = node as Element;
  if (element.namespaceURI === WORDPROCESSINGML_NAMESPACE) {
    switch (element.localName) {
      case 't':
        return element.textContent ?? '';
      case 'tab':
        return '\t';
      case 'br':
      case 'cr':
        return '\n';
      case 'noBreakHyphen':
        return '\u2011';
      case 'softHyphen':
        return '\u00ad';
      case 'del':
      case 'delText':
      case 'instrText':
      case 'pPr':
      case 'rPr':
        return '';
    }
  }

  let text = '';
  for (const child of Array.from(element.childNodes)) {
    text += extractWordParagraphText(child);
  }
  return text;
}

function renderWordTable(table: Element): string {
  const rows = directWordChildren(table, 'tr').map((row) =>
    directWordChildren(row, 'tc').map((cell) => renderWordBlocks(cell).join('<br>')),
  );
  if (rows.length === 0) return '';

  const columnCount = Math.max(...rows.map((row) => row.length));
  if (columnCount === 0) return '';

  const normalizeRow = (row: string[]) =>
    Array.from({ length: columnCount }, (_, index) =>
      (row[index] ?? '').replace(/\|/g, '\\|').replace(/\r?\n/g, '<br>'),
    );
  const renderRow = (row: string[]) => '| ' + normalizeRow(row).join(' | ') + ' |';
  const header = renderRow(rows[0]);
  const divider = '| ' + Array.from({ length: columnCount }, () => '---').join(' | ') + ' |';
  return [header, divider, ...rows.slice(1).map(renderRow)].join('\n');
}

function renderWordBlocks(container: Element): string[] {
  const blocks: string[] = [];
  for (const child of Array.from(container.children)) {
    if (isWordElement(child, 'p')) {
      const paragraph = extractWordParagraphText(child);
      if (paragraph.length > 0) blocks.push(paragraph);
    } else if (isWordElement(child, 'tbl')) {
      const table = renderWordTable(child);
      if (table) blocks.push(table);
    } else if (
      child.namespaceURI === WORDPROCESSINGML_NAMESPACE
      && DOCX_BLOCK_CONTAINERS.has(child.localName)
    ) {
      blocks.push(...renderWordBlocks(child));
    }
  }
  return blocks;
}

function parseDocxBytesToMarkdown(fileBytes: Uint8Array): string {
  const unzipped = unzipSync(fileBytes);
  const docXmlBytes = unzipped['word/document.xml'];
  if (!docXmlBytes) {
    throw new Error('word/document.xml을 찾을 수 없습니다.');
  }

  if (typeof DOMParser === 'undefined') {
    throw new Error('DOCX XML 파서를 사용할 수 없는 환경입니다.');
  }

  const xml = strFromU8(docXmlBytes);
  const document = new DOMParser().parseFromString(xml, 'application/xml');
  if (document.getElementsByTagName('parsererror').length > 0) {
    throw new Error('word/document.xml의 XML 형식이 올바르지 않습니다.');
  }

  const body = document.getElementsByTagNameNS(WORDPROCESSINGML_NAMESPACE, 'body').item(0);
  if (!body) {
    throw new Error('word/document.xml에서 WordprocessingML 본문을 찾을 수 없습니다.');
  }

  return renderWordBlocks(body).join('\n\n');
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
