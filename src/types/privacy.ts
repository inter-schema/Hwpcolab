export interface ExcludedRegion {
  id: string;
  label: string;
  targetType: "cell" | "body_text";
  tableIndex?: number;
  row?: number;
  col?: number;
  rawText: string;
  rects?: Array<{ pageIndex: number; x: number; y: number; width: number; height: number }>;
}

export interface PiiTokenMap {
  token: string;
  rawValue: string;
}
