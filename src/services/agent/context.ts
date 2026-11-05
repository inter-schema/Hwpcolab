import type { TableCoordinateMeta, SelectionTarget } from "../../types/hwp.ts";
import type { LayoutReport } from "../../core/layoutInspector.ts";
import { formatLayoutReportForPrompt } from "../../core/layoutInspector.ts";
import type { NeutralMessage } from "./types.ts";

export const DOC_MARKDOWN_THRESHOLD = 12000;
export const TOOL_RESULT_MAX_LENGTH = 4000;

/**
 * Truncate a tool result string if it exceeds maxLength (default 4000).
 * Appends `\n...(생략 ${omitted}자)...` when truncated.
 */
export function truncateToolResult(text: string, maxLength: number = TOOL_RESULT_MAX_LENGTH): string {
  if (!text || text.length <= maxLength) {
    return text;
  }
  const omitted = text.length - maxLength;
  return text.slice(0, maxLength) + `\n...(생략 ${omitted}자)...`;
}

/**
 * Summarize a single tool result into a 1-line string.
 */
export function summarizeToolContent(content: string): string {
  if (content.startsWith("[이전 도구 실행 결과 축약]")) {
    return content;
  }
  try {
    const parsed = JSON.parse(content);
    if (parsed && typeof parsed === "object") {
      if (parsed.summary) {
        return `[이전 도구 실행 결과 축약] 요약: ${parsed.summary}`;
      }
      if (parsed.error) {
        return `[이전 도구 실행 결과 축약] 오류: ${parsed.error}`;
      }
      if (typeof parsed.ok === "boolean") {
        const detail = JSON.stringify(parsed);
        return `[이전 도구 실행 결과 축약] ok: ${parsed.ok}, ${detail.slice(0, 60)}`;
      }
    }
  } catch {}

  const firstLine = content
    .split("\n")
    .map((l) => l.trim())
    .find((l) => l.length > 0) || "";
  const snippet = firstLine.length > 80 ? firstLine.slice(0, 80) + "..." : firstLine;
  return `[이전 도구 실행 결과 축약] ${snippet}`;
}

/**
 * Compact old tool results to a 1-line summary while preserving message structure
 * and toolCallId for protocol compliance.
 *
 * Keeps the most recent `keepRecentTurns` tool interaction turns uncompacted.
 */
export function compactOldToolResults(
  messages: NeutralMessage[],
  keepRecentTurns: number = 3
): NeutralMessage[] {
  // 1. Identify assistant turns that invoked tools
  const assistantToolIndices: number[] = [];
  for (let i = 0; i < messages.length; i++) {
    const msg = messages[i];
    if (msg.role === "assistant" && msg.toolCalls && msg.toolCalls.length > 0) {
      assistantToolIndices.push(i);
    }
  }

  // Determine threshold index: any tool call initiated before this assistant turn is old
  let cutOffAssistantIndex = -1;
  if (assistantToolIndices.length > keepRecentTurns) {
    const recentStartSlot = assistantToolIndices.length - keepRecentTurns;
    cutOffAssistantIndex = assistantToolIndices[recentStartSlot];
  }

  if (cutOffAssistantIndex === -1) {
    // Fewer than or equal to keepRecentTurns; nothing to compact
    return [...messages];
  }

  // Collect toolCallIds belonging to the recent turns
  const recentToolCallIds = new Set<string>();
  for (let i = cutOffAssistantIndex; i < messages.length; i++) {
    const msg = messages[i];
    if (msg.role === "assistant" && msg.toolCalls) {
      for (const tc of msg.toolCalls) {
        recentToolCallIds.add(tc.id);
      }
    }
  }

  return messages.map((msg) => {
    if (msg.role !== "tool") {
      return msg;
    }
    // If this tool message belongs to a recent turn, keep as is
    if (recentToolCallIds.has(msg.toolCallId)) {
      return msg;
    }
    // Otherwise compact it
    return {
      ...msg,
      content: summarizeToolContent(msg.content),
    };
  });
}

/**
 * Generate a document outline (headings and table summaries) when docMarkdown is too long.
 */
export function generateDocumentOutline(
  docMarkdown: string,
  tablesMeta: TableCoordinateMeta[] = []
): string {
  const lines = docMarkdown.split("\n");
  const headingLines: string[] = [];
  for (let idx = 0; idx < lines.length; idx++) {
    const line = lines[idx].trim();
    if (/^#{1,6}\s+/.test(line)) {
      headingLines.push(`(문단 ${idx + 1}) ${line}`);
    }
  }

  const sections: string[] = [];

  sections.push("### [문서 개요 - 주요 제목 구조]");
  if (headingLines.length > 0) {
    sections.push(headingLines.slice(0, 50).join("\n"));
  } else {
    sections.push("(제목 마크다운 서식이 없습니다. 본문 첫 3문단 참고)");
    const paras = docMarkdown
      .split(/\n{2,}/)
      .map((p) => p.trim())
      .filter(Boolean);
    sections.push(paras.slice(0, 3).map((p, i) => `[문단 ${i + 1}]: ${p.slice(0, 100)}...`).join("\n"));
  }

  if (tablesMeta.length > 0) {
    sections.push("### [문서 내 표 구조 요약]");
    const tblSummaries = tablesMeta.map((t) => {
      const headerRow = t.cells.filter((c) => c.row === 0).map((c) => c.text.replace(/\n/g, " ").trim());
      const headerPreview = headerRow.slice(0, 8).join(" | ");
      return `- 표 ${t.tableIndex}: ${t.rowCount}행 x ${t.colCount}열${headerPreview ? ` (헤더: ${headerPreview})` : ""}`;
    });
    sections.push(tblSummaries.join("\n"));
  }

  sections.push(
    `\n[안내: 문서 본문이 12,000자를 초과(총 ${docMarkdown.length.toLocaleString()}자)하여 제목/표 개요 및 선택영역만 표시되었습니다.\n상세 문단이나 표 내용은 \`read_document\` 도구를 사용하여 필요한 범위를 조회하십시오.]`
  );

  return sections.join("\n\n");
}

/**
 * Read parts of the document on-demand (paragraphs or table cells).
 */
export function executeReadDocument(
  args: {
    kind: "paragraphs" | "table";
    start?: number;
    count?: number;
    tableIndex?: number;
  },
  docMarkdown: string,
  tablesMeta: TableCoordinateMeta[] = []
): string {
  if (args.kind === "paragraphs") {
    const paras = docMarkdown
      .split(/\n{2,}/)
      .map((p) => p.trim())
      .filter(Boolean);

    const totalParagraphs = paras.length;
    const start = Math.max(0, args.start ?? 0);
    const count = Math.max(1, Math.min(30, args.count ?? 5));

    if (start >= totalParagraphs) {
      return JSON.stringify({
        ok: false,
        error: `문단 범위 초과: start=${start}, 전체 문단 수=${totalParagraphs}`,
        totalParagraphs,
      });
    }

    const sliced = paras.slice(start, start + count);
    return JSON.stringify({
      ok: true,
      kind: "paragraphs",
      start,
      count: sliced.length,
      totalParagraphs,
      paragraphs: sliced.map((text, idx) => ({
        index: start + idx,
        text,
      })),
    });
  }

  if (args.kind === "table") {
    const tableIndex = args.tableIndex ?? 0;
    const table = tablesMeta.find((t) => t.tableIndex === tableIndex);

    if (!table) {
      return JSON.stringify({
        ok: false,
        error: `표 ${tableIndex}를 찾을 수 없습니다. (전체 표 개수: ${tablesMeta.length})`,
        availableTables: tablesMeta.map((t) => ({
          tableIndex: t.tableIndex,
          rowCount: t.rowCount,
          colCount: t.colCount,
        })),
      });
    }

    const startRow = Math.max(0, args.start ?? 0);
    const count = Math.max(1, Math.min(20, args.count ?? 5));
    const endRow = Math.min(table.rowCount - 1, startRow + count - 1);

    const rowsMap = new Map<number, Array<{ col: number; text: string; rowSpan: number; colSpan: number }>>();
    for (const cell of table.cells) {
      if (cell.row >= startRow && cell.row <= endRow) {
        if (!rowsMap.has(cell.row)) {
          rowsMap.set(cell.row, []);
        }
        rowsMap.get(cell.row)!.push({
          col: cell.col,
          text: cell.text,
          rowSpan: cell.rowSpan,
          colSpan: cell.colSpan,
        });
      }
    }

    const rowsData = Array.from(rowsMap.entries())
      .sort(([r1], [r2]) => r1 - r2)
      .map(([row, cells]) => ({
        row,
        cells: cells.sort((c1, c2) => c1.col - c2.col),
      }));

    return JSON.stringify({
      ok: true,
      kind: "table",
      tableIndex,
      rowCount: table.rowCount,
      colCount: table.colCount,
      startRow,
      endRow,
      rows: rowsData,
    });
  }

  return JSON.stringify({ ok: false, error: `지원하지 않는 kind: ${(args as any).kind}` });
}

/**
 * Build the full context prompt for multi-turn agent loop.
 */
export function buildDocumentContext(options: {
  docMarkdown?: string;
  tablesMeta?: TableCoordinateMeta[];
  selection?: SelectionTarget | null;
  initialReport?: LayoutReport | null;
  attachmentsContext?: string;
  userPrompt?: string;
}): string {
  const parts: string[] = [];
  const tablesMeta = options.tablesMeta || [];

  // 1. Tables summary
  const tablesSummary = tablesMeta
    .map(
      (t) =>
        `- 표 ${t.tableIndex} (${t.rowCount}행 x ${t.colCount}열): ` +
        t.cells
          .slice(0, 30)
          .map((c) => `[${c.row},${c.col}]="${c.text.replace(/\n/g, " ")}"`)
          .join(", ")
    )
    .join("\n");

  parts.push(
    `### [현재 문서 구조 요약]\n표 개수: ${tablesMeta.length}${tablesSummary ? "\n" + tablesSummary : ""}`
  );

  // 2. Initial Layout Report
  if (options.initialReport) {
    parts.push(
      `### [문서 레이아웃 실측 진단 보고서 (WASM Inspector)]\n${formatLayoutReportForPrompt(options.initialReport)}`
    );
  }

  // 3. Selection
  if (options.selection) {
    if (options.selection.kind === "table_cells") {
      parts.push(
        `### [사용자가 마우스로 블록 지정한 표 셀 영역]\n` +
          `표 인덱스: ${options.selection.tableIndex} (${options.selection.startRow + 1}~${options.selection.endRow + 1}행, ${options.selection.startCol + 1}~${options.selection.endCol + 1}열)\n` +
          `선택된 셀 데이터:\n` +
          options.selection.selectedCells.map((c) => `- [${c.row}행 ${c.col}열]: "${c.text}"`).join("\n")
      );
    } else if (options.selection.kind === "body_range") {
      parts.push(
        `### [사용자가 마우스로 드래그 선택한 본문 텍스트]\n` +
          `문단 범위: ${options.selection.startPara + 1} ~ ${options.selection.endPara + 1}\n` +
          `내용:\n${options.selection.selectedText}`
      );
    }
  }

  // 4. Document Body (full or outline if > 12000 chars)
  if (options.docMarkdown) {
    if (options.docMarkdown.length > DOC_MARKDOWN_THRESHOLD) {
      parts.push(generateDocumentOutline(options.docMarkdown, tablesMeta));
    } else {
      parts.push(`### [문서 전체 마크다운 내용]\n${options.docMarkdown}`);
    }
  }

  // 5. Attachments
  if (options.attachmentsContext) {
    parts.push(`### [사용자가 첨부한 로컬 참고자료 (Excel / PDF / HWP 등)]\n${options.attachmentsContext}`);
  }

  // 6. User prompt
  if (options.userPrompt) {
    parts.push(`### [사용자 지시사항]\n${options.userPrompt}`);
  }

  return parts.join("\n\n");
}
