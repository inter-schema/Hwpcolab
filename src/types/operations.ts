export type UnifiedOp =
  // 1. 본문 텍스트 치환 및 문단 조작
  | { type: "replace_body_text"; find: string; replace: string }
  | { type: "set_paragraph_text"; paraIndex: number; text: string; oldTextForHwp?: string }
  | { type: "append_paragraph"; text: string; align?: "LEFT" | "CENTER" | "RIGHT" | "JUSTIFY" }
  | { type: "append_heading"; level: 1 | 2 | 3 | 4; text: string }
  // 2. 표 셀 값 채우기 및 수정
  | {
      type: "set_cell_text";
      tableIndex: number; // 0-based
      sec?: number;       // tableMeta에서 자동 주입
      para?: number;      // tableMeta에서 자동 주입
      ctrl?: number;      // tableMeta에서 자동 주입
      row: number;
      col: number;
      text: string;
      fit?: boolean;      // 서명란 등 공백 길이 유지 여부
    }
  // 3. 표 구조 제어
  | { type: "insert_table"; afterParaIndex?: number; headers: string[]; rows: string[][] }
  | { type: "insert_table_row"; tableIndex: number; row: number; position: "above" | "below"; cells?: string[] }
  | { type: "delete_table_row"; tableIndex: number; row: number }
  | { type: "insert_table_col"; tableIndex: number; col: number; position: "left" | "right"; cells?: string[] }
  | { type: "delete_table_col"; tableIndex: number; col: number }
  | { type: "merge_cells"; tableIndex: number; fromRow: number; fromCol: number; toRow: number; toCol: number }
  | { type: "unmerge_cells"; tableIndex: number; row: number; col: number }
  | { type: "split_cell"; tableIndex: number; row: number; col: number; intoRows: number; intoCols?: number }
  | { type: "equalize_table"; tableIndex: number; mode: "width" | "height" | "both" }
  // 4. 표 셀 스타일링
  | { type: "set_cell_background"; tableIndex: number; row: number; col: number; color: string }
  | {
      type: "set_cell_border";
      tableIndex: number;
      row: number;
      col: number;
      sides: Array<"top" | "bottom" | "left" | "right"> | "all";
      color?: string;
      widthMm?: number;
      lineType?: "solid" | "dashed" | "dotted" | "double";
    }
  // 5. 문단 및 글자 서식
  | {
      type: "apply_text_style";
      targetText: string;
      fontFamily?: string;     // 한컴 A-set 폰트 (함초롬바탕 등)
      sizePt?: number;
      bold?: boolean;
      italic?: boolean;
      underline?: boolean;
      color?: string;
      highlight?: string | boolean;
      letterSpacing?: number;  // 자간 (%)
      charRatio?: number;      // 장평 (%)
    }
  | {
      type: "apply_paragraph_style";
      paraIndex?: number | "last";
      targetText?: string;
      align?: "LEFT" | "CENTER" | "RIGHT" | "JUSTIFY" | "DISTRIBUTE";
      indentHu?: number;       // 들여쓰기 (HWPUNIT)
      lineSpacingPercent?: number; // 줄간격 (%)
      spacingBeforeHu?: number;
      spacingAfterHu?: number;
    }
  | { type: "set_list_style"; paraIndex?: number; targetText?: string; mode: "numbered" | "bullet" }
  // 6. 개체 및 페이지 설정
  | { type: "insert_image"; path: string; anchorText?: string; widthMm?: number; heightMm?: number; cell?: { tableIndex: number; row: number; col: number } }
  | { type: "place_seal"; anchorText: string; sealImagePath: string; mode?: "auto" | "overlap" | "right" }
  | { type: "set_header_footer"; where: "header" | "footer"; text: string }
  | { type: "insert_page_number"; where?: "header" | "footer"; align?: "left" | "center" | "right" }
  | { type: "insert_footnote"; anchorText: string; text: string; paraIndex?: number }
  | { type: "set_columns"; count: 1 | 2 | 3; spacingMm?: number }
  // 7. 표 및 페이지 레이아웃 치수 제어 (신규 6종)
  | {
      type: "set_col_width";
      tableIndex: number;
      col: number;
      widthMm: number;
      keepTableWidth?: boolean;
    }
  | {
      type: "set_row_height";
      tableIndex: number;
      row: number;
      heightMm: number;
    }
  | {
      type: "set_cell_padding";
      tableIndex: number;
      row: number;
      col: number;
      leftMm?: number;
      rightMm?: number;
      topMm?: number;
      bottomMm?: number;
      marginMm?: number;
    }
  | {
      type: "fit_table_to_page";
      tableIndex: number;
      targetWidthMm?: number;
    }
  | {
      type: "set_page_margin";
      leftMm?: number;
      rightMm?: number;
      topMm?: number;
      bottomMm?: number;
      marginMm?: number;
    }
  | {
      type: "shrink_cell_font";
      tableIndex: number;
      row: number;
      col: number;
      sizePt: number;
      minPt?: number;
    };

export interface DiffItem {
  id: string;
  status: "pending" | "accepted" | "rejected";
  scope: "paragraph" | "cell" | "table_structure" | "style" | "document";
  title: string;
  beforeText: string;
  afterText: string;
  unifiedOps: UnifiedOp[];
  targetCoord?: {
    sec?: number;
    para?: number;
    tableIndex?: number;
    row?: number;
    col?: number;
  };
  bbox?: {
    pageIndex: number;
    x: number;
    y: number;
    w: number;
    h: number;
  };
  verificationStatus?: "verified" | "warning_unresolved" | "warning_new_issues" | "unverified";
  warnings?: string[];
  metricsDelta?: {
    beforeOverflowMm?: number;
    afterOverflowMm?: number;
    widthBeforeMm?: number;
    widthAfterMm?: number;
    heightBeforeMm?: number;
    heightAfterMm?: number;
  };
}
