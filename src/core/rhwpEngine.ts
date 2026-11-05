import type { CellBBox, TableCoordinateMeta } from "../types/hwp.ts";
import { syncHwpxTableIndex } from "./hwpxTableIndex.ts";

export interface PageGeometry {
  width: number;
  height: number;
}

export interface PageLayoutInfo {
  pageIndex: number;
  width: number;
  height: number;
}

// 1. measureTextWidth stub (must register BEFORE rhwp init)
let measureCtx: CanvasRenderingContext2D | null = null;
let lastFont = "";
if (typeof (globalThis as any).measureTextWidth !== "function") {
  (globalThis as any).measureTextWidth = (font: string, text: string): number => {
    if (!measureCtx && typeof document !== "undefined") {
      measureCtx = document.createElement("canvas").getContext("2d");
    }
    if (measureCtx && font !== lastFont) {
      measureCtx.font = font;
      lastFont = font;
    }
    if (measureCtx) return measureCtx.measureText(text).width;
    // CJK 1.0em calibrated fallback
    const fs = parseFloat(font) || 10;
    let w = 0;
    for (const ch of String(text || "")) {
      const code = ch.charCodeAt(0);
      if ((code >= 0xac00 && code <= 0xd7a3) || (code >= 0x3000 && code <= 0x303f)) {
        w += fs * 1.0;
      } else if (ch === " " || ch === "\t") {
        w += fs * 0.4;
      } else if (ch >= "A" && ch <= "Z") {
        w += fs * 0.65;
      } else {
        w += fs * 0.55;
      }
    }
    return w * (96 / 72);
  };
}

let rhwpModulePromise: Promise<any> | null = null;

export async function getRhwpModule(): Promise<any> {
  if (!rhwpModulePromise) {
    rhwpModulePromise = (async () => {
      if (typeof window === "undefined") {
        const fs = await import("node:fs");
        const path = await import("node:path");
        const url = await import("node:url");
        const candidates = [
          path.resolve(process.cwd(), "resources/claw-hwp/scripts/vendor/rhwp"),
          path.resolve(process.cwd(), "vendor/rhwp-core"),
          path.resolve(process.cwd(), "public/vendor/rhwp"),
        ];
        let chosenDir = candidates.find(d => fs.existsSync(path.join(d, "rhwp.js")));
        if (!chosenDir) chosenDir = candidates[0];
        const wasmPath = path.join(chosenDir, "rhwp_bg.wasm");
        const jsPath = path.join(chosenDir, "rhwp.js");
        const wasmBytes = fs.readFileSync(wasmPath);
        const rhwp = await import(/* @vite-ignore */ url.pathToFileURL(jsPath).href);
        await rhwp.default({ module_or_path: wasmBytes });
        return rhwp;
      }
      const rhwpJsUrl = new URL("/vendor/rhwp/rhwp.js", window.location.origin).href;
      const rhwpWasmUrl = new URL("/vendor/rhwp/rhwp_bg.wasm", window.location.origin).href;
      const rhwp = await import(/* @vite-ignore */ rhwpJsUrl);
      await rhwp.default({ module_or_path: rhwpWasmUrl });
      return rhwp;
    })();
  }
  return rhwpModulePromise;
}

export class RhwpDocumentWrapper {
  private rhwpDoc: any;
  public pageGeometries: PageGeometry[] = [];
  public tablesMeta: TableCoordinateMeta[] = [];

  private constructor(doc: any) {
    this.rhwpDoc = doc;
  }

  public static async create(fileBytes: Uint8Array): Promise<RhwpDocumentWrapper> {
    const rhwp = await getRhwpModule();
    const doc = new rhwp.HwpDocument(fileBytes);
    const wrapper = new RhwpDocumentWrapper(doc);

    // Auto-fix reflow
    try {
      doc.reflowLinesegs();
    } catch (e) {
      console.warn("reflowLinesegs warning:", e);
    }

    // Sweep 1: getPageInfo for all pages
    const pageCount: number = doc.pageCount();
    wrapper.pageGeometries = [];
    for (let i = 0; i < pageCount; i++) {
      try {
        const infoStr = doc.getPageInfo(i);
        const info = JSON.parse(infoStr);
        wrapper.pageGeometries.push({
          width: info.width ?? 595,
          height: info.height ?? 842,
        });
      } catch (err) {
        console.warn(`getPageInfo(${i}) error:`, err);
        wrapper.pageGeometries.push({ width: 595, height: 842 });
      }
    }

    // Sweep 3: Index tables & cell bboxes
    const topTables = wrapper.indexAllTables();
    const isHwpx = fileBytes.length >= 2 && fileBytes[0] === 0x50 && fileBytes[1] === 0x4b;
    if (isHwpx) {
      try {
        wrapper.tablesMeta = syncHwpxTableIndex(fileBytes, topTables);
      } catch (e) {
        console.warn("[rhwpEngine] Failed to sync HWPX table index:", e);
        wrapper.tablesMeta = topTables.map((t) => ({ ...t, depth: 0 }));
      }
    } else {
      wrapper.tablesMeta = topTables.map((t) => ({ ...t, depth: 0 }));
    }

    return wrapper;
  }

  public getPageCount(): number {
    return this.pageGeometries.length;
  }

  public renderPage(pageIndex: number, canvas: HTMLCanvasElement, scale: number) {
    if (!this.rhwpDoc) return;
    try {
      this.rhwpDoc.renderPageToCanvas(pageIndex, canvas, scale);
    } finally {
      // Golden Rule 5: MUST call getPageTextLayout to release Rust borrow
      try {
        this.rhwpDoc.getPageTextLayout(pageIndex);
      } catch (e) {
        // ignore layout release errors
      }
    }
  }

  private indexAllTables(): TableCoordinateMeta[] {
    const tables: TableCoordinateMeta[] = [];
    if (!this.rhwpDoc) return tables;

    let secCount = 1;
    try {
      secCount = this.rhwpDoc.getSectionCount();
    } catch {}

    let globalTableIndex = 0;

    for (let sec = 0; sec < secCount; sec++) {
      let paraCount = 0;
      try {
        paraCount = this.rhwpDoc.getParagraphCount(sec);
      } catch {}

      for (let p = 0; p < paraCount; p++) {
        // Scan ctrl up to 64; do NOT break on error, continue!
        for (let c = 0; c < 64; c++) {
          let testCellInfo: any = null;
          try {
            const raw = this.rhwpDoc.getCellInfo(sec, p, c, 0);
            testCellInfo = JSON.parse(raw);
          } catch {
            continue;
          }

          if (!testCellInfo || typeof testCellInfo.row !== "number") {
            continue;
          }

          // This control is a table!
          // Retrieve cell bboxes
          let bboxes: any[] = [];
          try {
            const bboxesRaw = this.rhwpDoc.getTableCellBboxes(sec, p, c, null);
            bboxes = JSON.parse(bboxesRaw);
          } catch (e) {
            console.warn(`getTableCellBboxes failed for table (${sec}, ${p}, ${c}):`, e);
          }

          const cells: TableCoordinateMeta["cells"] = [];
          let maxRow = 0;
          let maxCol = 0;

          for (let i = 0; i < 10000; i++) {
            let cellInfo: any = null;
            try {
              cellInfo = JSON.parse(this.rhwpDoc.getCellInfo(sec, p, c, i));
            } catch {
              break;
            }
            if (!cellInfo || typeof cellInfo.row !== "number") break;

            let text = "";
            try {
              const nPara = this.rhwpDoc.getCellParagraphCount(sec, p, c, i);
              const parts: string[] = [];
              for (let pi = 0; pi < nPara; pi++) {
                try {
                  parts.push(this.rhwpDoc.getTextInCell(sec, p, c, i, pi, 0, 100000));
                } catch {}
              }
              text = parts.join("\n");
            } catch {
              try {
                text = this.rhwpDoc.getTextInCell(sec, p, c, i, 0, 0, 100000);
              } catch {}
            }

            const row = cellInfo.row;
            const col = cellInfo.col;
            const rowSpan = cellInfo.rowSpan ?? 1;
            const colSpan = cellInfo.colSpan ?? 1;

            maxRow = Math.max(maxRow, row + rowSpan);
            maxCol = Math.max(maxCol, col + colSpan);

            // Match bbox if available
            const matchingBbox = bboxes.find((b: any) => b.cellIdx === i || (b.row === row && b.col === col));
            let cellBBox: CellBBox | undefined = undefined;
            if (matchingBbox) {
              cellBBox = {
                cellIdx: i,
                row,
                col,
                rowSpan,
                colSpan,
                pageIndex: matchingBbox.pageIndex ?? matchingBbox.page ?? 0,
                x: matchingBbox.x ?? 0,
                y: matchingBbox.y ?? 0,
                w: matchingBbox.w ?? matchingBbox.width ?? 0,
                h: matchingBbox.h ?? matchingBbox.height ?? 0,
              };
            }

            cells.push({
              idx: i,
              row,
              col,
              rowSpan,
              colSpan,
              text,
              bbox: cellBBox,
            });
          }

          tables.push({
            tableIndex: globalTableIndex++,
            sec,
            para: p,
            ctrl: c,
            rowCount: maxRow,
            colCount: maxCol,
            cells,
          });
        }
      }
    }

    return tables;
  }

  public hitTest(pageIndex: number, pageX: number, pageY: number) {
    if (!this.rhwpDoc) return null;
    try {
      const raw = this.rhwpDoc.hitTest(pageIndex, pageX, pageY);
      return JSON.parse(raw);
    } catch {
      return null;
    }
  }

  public getSelectionRects(sec: number, startPara: number, startOffset: number, endPara: number, endOffset: number) {
    if (!this.rhwpDoc) return [];
    try {
      const raw = this.rhwpDoc.getSelectionRects(sec, startPara, startOffset, endPara, endOffset);
      return JSON.parse(raw);
    } catch {
      return [];
    }
  }

  public getTextRange(sec: number, para: number, offset: number, len: number): string {
    if (!this.rhwpDoc) return "";
    try {
      return this.rhwpDoc.getTextRange(sec, para, offset, len);
    } catch {
      return "";
    }
  }

  public getRawDoc(): any {
    return this.rhwpDoc;
  }

  public getSectionCount(): number {
    if (!this.rhwpDoc) return 0;
    try {
      return this.rhwpDoc.getSectionCount();
    } catch {
      return 1;
    }
  }

  public getParagraphCount(sec: number): number {
    if (!this.rhwpDoc) return 0;
    try {
      return this.rhwpDoc.getParagraphCount(sec);
    } catch {
      return 0;
    }
  }

  public getPageDef(sec = 0): any {
    if (!this.rhwpDoc) return null;
    try {
      const raw = this.rhwpDoc.getPageDef(sec);
      return typeof raw === "string" ? JSON.parse(raw) : raw;
    } catch {
      return null;
    }
  }

  public getCellProperties(sec: number, p: number, c: number, i: number): any {
    if (!this.rhwpDoc) return null;
    try {
      const raw = this.rhwpDoc.getCellProperties(sec, p, c, i);
      return typeof raw === "string" ? JSON.parse(raw) : raw;
    } catch {
      return null;
    }
  }

  public getTableCellBboxes(sec: number, p: number, c: number): any[] {
    if (!this.rhwpDoc) return [];
    try {
      const raw = this.rhwpDoc.getTableCellBboxes(sec, p, c, null);
      return typeof raw === "string" ? JSON.parse(raw) : raw;
    } catch {
      return [];
    }
  }

  public getCellParagraphCount(sec: number, p: number, c: number, i: number): number {
    if (!this.rhwpDoc) return 0;
    try {
      return this.rhwpDoc.getCellParagraphCount(sec, p, c, i);
    } catch {
      return 0;
    }
  }

  public getTextInCell(sec: number, p: number, c: number, i: number, pi = 0): string {
    if (!this.rhwpDoc) return "";
    try {
      return this.rhwpDoc.getTextInCell(sec, p, c, i, pi, 0, 100000) || "";
    } catch {
      return "";
    }
  }

  public free() {
    if (this.rhwpDoc && typeof this.rhwpDoc.free === "function") {
      try {
        this.rhwpDoc.free();
      } catch {}
      this.rhwpDoc = null;
    }
  }
}
