export interface CellBBox {
  cellIdx: number;
  row: number;
  col: number;
  rowSpan: number;
  colSpan: number;
  pageIndex: number; // 0-based
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface TableCoordinateMeta {
  tableIndex: number; // 0-based
  sec: number;        // for create.js (.hwp)
  para: number;       // for create.js (.hwp)
  ctrl: number;       // for create.js (.hwp)
  rowCount: number;
  colCount: number;
  cells: Array<{
    idx: number;
    row: number;
    col: number;
    rowSpan: number;
    colSpan: number;
    text: string;
    bbox?: CellBBox;
  }>;
}

export type SelectionTarget =
  | {
      kind: "body_range";
      sec: number;
      startPara: number;
      startOffset: number;
      endPara: number;
      endOffset: number;
      selectedText: string;
      rects: Array<{ pageIndex: number; x: number; y: number; width: number; height: number }>;
    }
  | {
      kind: "table_cells";
      tableIndex: number;
      sec: number;
      para: number;
      ctrl: number;
      startRow: number;
      startCol: number;
      endRow: number;
      endCol: number;
      selectedCells: Array<{
        cellIdx: number;
        row: number;
        col: number;
        text: string;
        bbox: CellBBox;
      }>;
      unionRect: { pageIndex: number; x: number; y: number; width: number; height: number };
    };

export type DirectEditingTarget =
  | {
      kind: "cell";
      tableIndex: number;
      row: number;
      col: number;
      text: string;
      bbox: CellBBox;
    }
  | {
      kind: "paragraph";
      sec: number;
      para: number;
      text: string;
      rect: { pageIndex: number; x: number; y: number; width: number; height: number };
    };

export interface OpenedDocSession {
  original_path: string;
  staging_path: string;
  file_name: string;
  is_hwpx: boolean;
  bytes: number[] | Uint8Array;
}
