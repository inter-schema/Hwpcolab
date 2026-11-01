import { SelectionTarget, CellBBox, TableCoordinateMeta } from "../types/hwp";
import { RhwpDocumentWrapper } from "./rhwpEngine";

export interface HitPoint {
  pageIndex: number;
  pageX: number;
  pageY: number;
}

export function findCellAtPoint(
  point: HitPoint,
  tablesMeta: TableCoordinateMeta[]
): { tableMeta: TableCoordinateMeta; cell: TableCoordinateMeta["cells"][0] } | null {
  for (const table of tablesMeta) {
    for (const cell of table.cells) {
      if (cell.bbox && cell.bbox.pageIndex === point.pageIndex) {
        const { x, y, w, h } = cell.bbox;
        if (point.pageX >= x && point.pageX <= x + w && point.pageY >= y && point.pageY <= y + h) {
          return { tableMeta: table, cell };
        }
      }
    }
  }
  return null;
}

export function calculateTableCellSelection(
  tableMeta: TableCoordinateMeta,
  anchorCell: TableCoordinateMeta["cells"][0],
  hoverCell: TableCoordinateMeta["cells"][0]
): SelectionTarget {
  const minRow = Math.min(anchorCell.row, hoverCell.row);
  const maxRow = Math.max(
    anchorCell.row + (anchorCell.rowSpan ?? 1) - 1,
    hoverCell.row + (hoverCell.rowSpan ?? 1) - 1
  );
  const minCol = Math.min(anchorCell.col, hoverCell.col);
  const maxCol = Math.max(
    anchorCell.col + (anchorCell.colSpan ?? 1) - 1,
    hoverCell.col + (hoverCell.colSpan ?? 1) - 1
  );

  const selectedCells = tableMeta.cells.filter(
    (c) => c.row >= minRow && c.row <= maxRow && c.col >= minCol && c.col <= maxCol
  );

  let unionX1 = Infinity;
  let unionY1 = Infinity;
  let unionX2 = -Infinity;
  let unionY2 = -Infinity;
  let pageIndex = anchorCell.bbox?.pageIndex ?? 0;

  for (const c of selectedCells) {
    if (c.bbox) {
      pageIndex = c.bbox.pageIndex;
      unionX1 = Math.min(unionX1, c.bbox.x);
      unionY1 = Math.min(unionY1, c.bbox.y);
      unionX2 = Math.max(unionX2, c.bbox.x + c.bbox.w);
      unionY2 = Math.max(unionY2, c.bbox.y + c.bbox.h);
    }
  }

  const unionRect = {
    pageIndex,
    x: isFinite(unionX1) ? unionX1 : 0,
    y: isFinite(unionY1) ? unionY1 : 0,
    width: isFinite(unionX2 - unionX1) ? unionX2 - unionX1 : 0,
    height: isFinite(unionY2 - unionY1) ? unionY2 - unionY1 : 0,
  };

  return {
    kind: "table_cells",
    tableIndex: tableMeta.tableIndex,
    sec: tableMeta.sec,
    para: tableMeta.para,
    ctrl: tableMeta.ctrl,
    startRow: minRow,
    startCol: minCol,
    endRow: maxRow,
    endCol: maxCol,
    selectedCells: selectedCells.map((c) => ({
      cellIdx: c.idx,
      row: c.row,
      col: c.col,
      text: c.text,
      bbox: c.bbox as CellBBox,
    })),
    unionRect,
  };
}

export function calculateBodyTextSelection(
  doc: RhwpDocumentWrapper,
  anchor: { sec: number; para: number; offset: number },
  focus: { sec: number; para: number; offset: number }
): SelectionTarget | null {
  let start = anchor;
  let end = focus;

  // Ordering
  if (
    start.sec > end.sec ||
    (start.sec === end.sec && (start.para > end.para || (start.para === end.para && start.offset > end.offset)))
  ) {
    start = focus;
    end = anchor;
  }

  const rects = doc.getSelectionRects(start.sec, start.para, start.offset, end.para, end.offset);

  // Extract text across paragraphs
  let selectedText = "";
  if (start.sec === end.sec) {
    if (start.para === end.para) {
      selectedText = doc.getTextRange(start.sec, start.para, start.offset, end.offset - start.offset);
    } else {
      const parts: string[] = [];
      parts.push(doc.getTextRange(start.sec, start.para, start.offset, 100000));
      for (let p = start.para + 1; p < end.para; p++) {
        parts.push(doc.getTextRange(start.sec, p, 0, 100000));
      }
      parts.push(doc.getTextRange(start.sec, end.para, 0, end.offset));
      selectedText = parts.join("\n");
    }
  }

  return {
    kind: "body_range",
    sec: start.sec,
    startPara: start.para,
    startOffset: start.offset,
    endPara: end.para,
    endOffset: end.offset,
    selectedText,
    rects: (rects || []).map((r: any) => ({
      pageIndex: r.pageIndex ?? r.page ?? 0,
      x: r.x,
      y: r.y,
      width: r.width ?? r.w,
      height: r.height ?? r.h,
    })),
  };
}
