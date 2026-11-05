import { RhwpDocumentWrapper } from "./rhwpEngine.ts";

export const HU_PER_PX = 75;
export const HU_PER_MM = 283.465;
export const PX_PER_MM = HU_PER_MM / HU_PER_PX; // ~3.779533

export type LayoutIssueCode =
  | "CELL_OVERFLOW_V"
  | "CELL_OVERFLOW_H"
  | "TABLE_EXCEEDS_PAGE";

export interface CellLayoutInfo {
  cellIdx: number;
  row: number;
  col: number;
  rowSpan: number;
  colSpan: number;
  widthMm: number;
  heightMm: number;
  availWidthMm: number;
  availHeightMm: number;
  estimatedLines: number;
  contentHeightMm: number;
  isVerticalOverflow: boolean;
  isHorizontalOverflow: boolean;
  overflowAmountMm: number;
  text: string;
}

export interface LayoutIssue {
  id: string;
  code: LayoutIssueCode;
  severity: "error" | "warning";
  message: string;
  cell?: {
    tableIndex: number;
    sec?: number;
    para?: number;
    ctrl?: number;
    row: number;
    col: number;
    cellIdx: number;
  };
  tableIndex?: number;
  metrics: {
    cellHeightMm?: number;
    contentHeightMm?: number;
    overflowMm?: number;
    lines?: number;
    tableWidthMm?: number;
    printableWidthMm?: number;
    overflowWidthMm?: number;
  };
  text?: string;
}

export interface LayoutReport {
  timestamp: number;
  tableCount: number;
  issuesCount: number;
  issues: LayoutIssue[];
  tables: Array<{
    tableIndex: number;
    section: number;
    para: number;
    ctrl: number;
    cellCount: number;
    tableWidthMm?: number;
    printableWidthMm?: number;
    cells: CellLayoutInfo[];
  }>;
}

export interface LayoutInspectorOptions {
  safetyMarginHorizontal?: number; // default: 0.05 (5%)
  safetyMarginVertical?: number;   // default: 0.08 (8%)
  defaultFontPt?: number;          // default: 10
  lineSpacingRatio?: number;       // default: 1.10
}

/**
 * CJK 문자와 영숫자/기호의 폭을 실측 비율(CJK 1.0em, 영숫자 0.55em, 공백 0.4em)로 계산합니다.
 */
export function measureKoreanTextPt(text: string, fontPt: number): number {
  let ptWidth = 0;
  for (const ch of String(text || "")) {
    const code = ch.charCodeAt(0);
    if ((code >= 0xac00 && code <= 0xd7a3) || (code >= 0x3000 && code <= 0x303f)) {
      ptWidth += fontPt * 1.0; // 한글 및 CJK 정방형 기호
    } else if (ch === " " || ch === "\t") {
      ptWidth += fontPt * 0.4;
    } else if (ch >= "A" && ch <= "Z") {
      ptWidth += fontPt * 0.65;
    } else if (ch >= "a" && ch <= "z") {
      ptWidth += fontPt * 0.55;
    } else if (ch >= "0" && ch <= "9") {
      ptWidth += fontPt * 0.55;
    } else {
      ptWidth += fontPt * 0.5;
    }
  }
  return ptWidth;
}

/**
 * 이미 생성된 RhwpDocumentWrapper를 기반으로 문서의 표 및 셀 레이아웃을 정밀 계측합니다.
 */
export function inspectRhwpWrapper(
  wrapper: RhwpDocumentWrapper,
  options?: LayoutInspectorOptions
): LayoutReport {
  const safetyH = options?.safetyMarginHorizontal ?? 0.05;
  const safetyV = options?.safetyMarginVertical ?? 0.08;
  const fontPt = options?.defaultFontPt ?? 10;
  const lineSpacingRatio = options?.lineSpacingRatio ?? 1.10;

  const fontPx = fontPt * (96 / 72);
  const lineAdvancePx = fontPx * lineSpacingRatio;

  const tablesReport: LayoutReport["tables"] = [];
  const issues: LayoutIssue[] = [];

  const tablesMeta = wrapper.tablesMeta;

  for (const tableMeta of tablesMeta) {
    const sec = tableMeta.sec;
    const para = tableMeta.para;
    const ctrl = tableMeta.ctrl;
    const tIdx = tableMeta.tableIndex;

    const pageDef = wrapper.getPageDef(sec) || {};
    const pageWidthHu = pageDef.width ?? 59528;
    const marginLHu = pageDef.marginLeft ?? pageDef.margin_left ?? 8504;
    const marginRHu = pageDef.marginRight ?? pageDef.margin_right ?? 8504;
    const printableWidthMm = Number(((pageWidthHu - marginLHu - marginRHu) / HU_PER_MM).toFixed(2));

    const cellsInfo: CellLayoutInfo[] = [];

    // Calculate column widths to find total table width
    const colMaxX = new Map<number, number>();
    for (const c of tableMeta.cells) {
      if (c.bbox && c.colSpan === 1) {
        colMaxX.set(c.col, Math.max(colMaxX.get(c.col) || 0, c.bbox.w));
      }
    }

    let tableWidthPx = 0;
    colMaxX.forEach((w) => { tableWidthPx += w; });
    const tableWidthMm = Number((tableWidthPx / PX_PER_MM).toFixed(2));

    const isNested = (tableMeta.depth ?? 0) > 0;
    let allowedWidthMm: number | null = null;
    let isParentCellLimit = false;

    if (isNested) {
      const parentTable = tablesMeta.find((t) => t.tableIndex === tableMeta.parentTableIndex);
      if (parentTable && tableMeta.parentCell) {
        const parentCell = parentTable.cells.find(
          (c) => c.row === tableMeta.parentCell!.row && c.col === tableMeta.parentCell!.col
        );
        if (parentCell?.bbox && parentCell.bbox.w > 0) {
          const pProps = wrapper.getCellProperties(
            parentTable.sec,
            parentTable.para,
            parentTable.ctrl,
            parentCell.idx
          ) || {};
          const padLPx = (pProps.paddingLeft ?? 400) / HU_PER_PX;
          const padRPx = (pProps.paddingRight ?? 400) / HU_PER_PX;
          const availWPx = Math.max(0, parentCell.bbox.w - padLPx - padRPx);
          allowedWidthMm = Number((availWPx / PX_PER_MM).toFixed(2));
          isParentCellLimit = true;
        }
      }
    } else {
      allowedWidthMm = printableWidthMm;
    }

    if (allowedWidthMm != null && tableWidthMm > allowedWidthMm + 1.0) {
      const overflowMm = Number((tableWidthMm - allowedWidthMm).toFixed(2));
      const targetDesc = isParentCellLimit
        ? `부모 셀 내부 폭(${allowedWidthMm}mm)`
        : `인쇄 가능 폭(${allowedWidthMm}mm)`;
      issues.push({
        id: `table:${tIdx}:exceeds_page`,
        code: "TABLE_EXCEEDS_PAGE",
        severity: "warning",
        message: `표 ${tIdx}의 너비(${tableWidthMm}mm)가 ${targetDesc}을 ${overflowMm}mm 초과했습니다.`,
        tableIndex: tIdx,
        metrics: {
          tableWidthMm,
          printableWidthMm: allowedWidthMm,
          overflowWidthMm: overflowMm,
        },
      });
    }

    for (const cell of tableMeta.cells) {
      const cellIdx = cell.idx;
      const row = cell.row;
      const col = cell.col;
      const rowSpan = cell.rowSpan ?? 1;
      const colSpan = cell.colSpan ?? 1;

      const props = wrapper.getCellProperties(sec, para, ctrl, cellIdx) || {};
      const padLPx = (props.paddingLeft ?? 400) / HU_PER_PX;
      const padRPx = (props.paddingRight ?? 400) / HU_PER_PX;
      const padTPx = (props.paddingTop ?? 400) / HU_PER_PX;
      const padBPx = (props.paddingBottom ?? 400) / HU_PER_PX;

      const bb = cell.bbox || { w: 100, h: 24, x: 0, y: 0, pageIndex: 0 };
      const widthPx = bb.w;
      const heightPx = bb.h;

      const widthMm = Number((widthPx / PX_PER_MM).toFixed(2));
      const heightMm = Number((heightPx / PX_PER_MM).toFixed(2));

      const availWidthPx = Math.max(0, widthPx - padLPx - padRPx);
      const availHeightPx = Math.max(0, heightPx - padTPx - padBPx);

      // Extract lines of text
      const nPara = wrapper.getCellParagraphCount(sec, para, ctrl, cellIdx);
      const paraTexts: string[] = [];
      if (nPara > 0) {
        for (let pi = 0; pi < nPara; pi++) {
          paraTexts.push(wrapper.getTextInCell(sec, para, ctrl, cellIdx, pi));
        }
      } else if (cell.text) {
        paraTexts.push(...cell.text.split("\n"));
      }
      const fullText = paraTexts.join("\n").replace(/[\r\x00-\x1f]+/g, " ").trim();

      const effectiveWidthPx = availWidthPx * (1 - safetyH);
      let totalEstimatedLines = 0;
      const lineDetails: { text: string; widthPx: number; lines: number }[] = [];

      for (const pText of paraTexts) {
        if (!pText || pText.length === 0) {
          totalEstimatedLines += 1;
          continue;
        }
        const textPt = measureKoreanTextPt(pText, fontPt);
        const textPx = textPt * (96 / 72);
        const lines = effectiveWidthPx > 0 ? Math.max(1, Math.ceil(textPx / effectiveWidthPx)) : 1;
        totalEstimatedLines += lines;
        lineDetails.push({ text: pText, widthPx: textPx, lines });
      }

      const padYPx = padTPx + padBPx;
      const contentHeightPx = totalEstimatedLines <= 0 ? 0 : fontPx + (totalEstimatedLines - 1) * lineAdvancePx + padYPx;
      const contentHeightMm = Number((contentHeightPx / PX_PER_MM).toFixed(2));

      const thresholdPx = Math.max(6.0, heightPx * safetyV);
      const overflowDiffPx = contentHeightPx - heightPx;
      const isVerticalOverflow = overflowDiffPx > thresholdPx;

      const maxLineWidthPx = Math.max(0, ...lineDetails.map((l) => l.widthPx));
      const isHorizontalOverflow = (maxLineWidthPx - availWidthPx) > (availWidthPx * safetyH + 4.0);
      const overflowAmountMm = Number(Math.max(0, contentHeightMm - heightMm).toFixed(2));

      const cellMetric: CellLayoutInfo = {
        cellIdx,
        row,
        col,
        rowSpan,
        colSpan,
        widthMm,
        heightMm,
        availWidthMm: Number((availWidthPx / PX_PER_MM).toFixed(2)),
        availHeightMm: Number((availHeightPx / PX_PER_MM).toFixed(2)),
        estimatedLines: totalEstimatedLines,
        contentHeightMm,
        isVerticalOverflow,
        isHorizontalOverflow,
        overflowAmountMm,
        text: fullText,
      };

      cellsInfo.push(cellMetric);

      if (isVerticalOverflow) {
        issues.push({
          id: `cell:${sec}:${para}:${ctrl}:${cellIdx}:v_overflow`,
          code: "CELL_OVERFLOW_V",
          severity: "error",
          message: `셀 [행 ${row}, 열 ${col}]: 내용물 높이(${contentHeightMm}mm)가 셀 높이(${heightMm}mm)를 +${overflowAmountMm}mm 초과했습니다. (${totalEstimatedLines}줄 예상)`,
          cell: {
            tableIndex: tIdx,
            sec,
            para,
            ctrl,
            row,
            col,
            cellIdx,
          },
          tableIndex: tIdx,
          metrics: {
            cellHeightMm: heightMm,
            contentHeightMm,
            overflowMm: overflowAmountMm,
            lines: totalEstimatedLines,
          },
          text: fullText.slice(0, 40),
        });
      }
    }

    tablesReport.push({
      tableIndex: tIdx,
      section: sec,
      para,
      ctrl,
      cellCount: cellsInfo.length,
      tableWidthMm,
      printableWidthMm: allowedWidthMm ?? printableWidthMm,
      cells: cellsInfo,
    });
  }

  return {
    timestamp: Date.now(),
    tableCount: tablesReport.length,
    issuesCount: issues.length,
    issues,
    tables: tablesReport,
  };
}

/**
 * 바이너리 바이트로부터 문서를 로드하여 레이아웃을 계측합니다.
 */
export async function inspectDocumentBytes(
  fileBytes: Uint8Array,
  options?: LayoutInspectorOptions
): Promise<LayoutReport> {
  const wrapper = await RhwpDocumentWrapper.create(fileBytes);
  try {
    return inspectRhwpWrapper(wrapper, options);
  } finally {
    wrapper.free();
  }
}

/**
 * LayoutReport를 LLM 프롬프트 또는 검토 화면에 표시하기 위한 마크다운 텍스트로 포맷팅합니다.
 */
export function formatLayoutReportForPrompt(report: LayoutReport): string {
  if (report.issuesCount === 0) {
    return `[레이아웃 검사 결과: 정상] 표 ${report.tableCount}개에 대해 시각적 넘침이나 여백 침범 이슈가 검출되지 않았습니다.`;
  }

  const lines: string[] = [
    `[레이아웃 검사 결과: 이슈 ${report.issuesCount}건 검출]`,
    `총 ${report.tableCount}개의 표에서 발견된 문제 목록:`,
  ];

  for (let i = 0; i < report.issues.length; i++) {
    const iss = report.issues[i];
    if (iss.code === "CELL_OVERFLOW_V" && iss.cell) {
      lines.push(
        `${i + 1}. [세로 넘침 / ${iss.severity}] 표 ${iss.cell.tableIndex} [행 ${iss.cell.row}, 열 ${iss.cell.col}]: 내용(${iss.metrics.contentHeightMm}mm) > 셀(${iss.metrics.cellHeightMm}mm) (+${iss.metrics.overflowMm}mm 초과, ${iss.metrics.lines}줄) | "${iss.text || ""}"`
      );
    } else if (iss.code === "TABLE_EXCEEDS_PAGE") {
      lines.push(
        `${i + 1}. [표 폭 초과 / ${iss.severity}] 표 ${iss.tableIndex}: 표 너비 ${iss.metrics.tableWidthMm}mm > 인쇄 영역 ${iss.metrics.printableWidthMm}mm (+${iss.metrics.overflowWidthMm}mm 여백 침범)`
      );
    } else {
      lines.push(`${i + 1}. [${iss.code} / ${iss.severity}] ${iss.message}`);
    }
  }

  return lines.join("\n");
}

/**
 * 수정 전(before)과 수정 후(after)의 LayoutReport를 비교하여 이슈 해결 상태를 진단합니다.
 */
export function compareLayoutReports(
  before: LayoutReport,
  after: LayoutReport
): {
  ok: boolean;
  resolvedIssues: LayoutIssue[];
  remainingIssues: LayoutIssue[];
  newIssues: LayoutIssue[];
  summaryText: string;
} {
  const beforeIds = new Set(before.issues.map((i) => i.id));
  const afterIds = new Set(after.issues.map((i) => i.id));

  const resolvedIssues = before.issues.filter((i) => !afterIds.has(i.id));
  const remainingIssues = after.issues.filter((i) => beforeIds.has(i.id));
  const newIssues = after.issues.filter((i) => !beforeIds.has(i.id));

  // ok is true if no new issues were introduced, and all previous issues were resolved (or strictly decreased)
  const ok = newIssues.length === 0 && (after.issuesCount === 0 || resolvedIssues.length > 0);

  const parts: string[] = [];
  if (resolvedIssues.length > 0) parts.push(`해결된 이슈: ${resolvedIssues.length}건`);
  if (remainingIssues.length > 0) parts.push(`잔여 이슈: ${remainingIssues.length}건`);
  if (newIssues.length > 0) parts.push(`새로 발생한 이슈: ${newIssues.length}건`);
  if (parts.length === 0) parts.push("이슈 없음 (정상)");

  const summaryText = parts.join(", ");

  return {
    ok,
    resolvedIssues,
    remainingIssues,
    newIssues,
    summaryText,
  };
}
