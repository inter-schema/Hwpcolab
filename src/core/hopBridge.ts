import type { WasmBridge, EventBus } from "@/upstream/core";
import type { CanvasView } from "@/upstream/view";
import type { InputHandler } from "@/upstream/editor";
import type { CellBBox, SelectionTarget, TableCoordinateMeta } from "../types/hwp";

export interface HopRuntimeHandle {
  wasm: WasmBridge;
  canvasView: CanvasView;
  inputHandler: InputHandler;
  eventBus: EventBus;
  openBytesInHop: (bytes: Uint8Array, fileName: string) => Promise<void>;
}

class HopBridgeManager {
  private handle: HopRuntimeHandle | null = null;
  private readyResolvers: Array<(h: HopRuntimeHandle) => void> = [];
  private isProgrammaticReload = false;
  private onHumanDocumentMutated: (() => void) | null = null;
  private onHopDocumentOpened: ((info: { fileName: string; sourceBytes: Uint8Array | null }) => void) | null = null;
  private onSelectionChanged: ((sel: SelectionTarget | null, toolbarPos: { x: number; y: number } | null) => void) | null = null;

  public registerRuntime(handle: HopRuntimeHandle): void {
    this.handle = handle;
    (window as any).__hopBridge = this;

    // Listen to HOP's document-changed events triggered by direct user edits
    handle.eventBus.on("document-changed", () => {
      if (this.isProgrammaticReload) return;
      this.onHumanDocumentMutated?.();
    });

    for (const resolve of this.readyResolvers) {
      resolve(handle);
    }
    this.readyResolvers = [];
  }

  public waitForReady(): Promise<HopRuntimeHandle> {
    if (this.handle) return Promise.resolve(this.handle);
    return new Promise((resolve) => {
      this.readyResolvers.push(resolve);
    });
  }

  public getRuntime(): HopRuntimeHandle | null {
    return this.handle;
  }

  public setHumanMutationListener(cb: (() => void) | null): void {
    this.onHumanDocumentMutated = cb;
  }

  public setSelectionListener(
    cb: ((sel: SelectionTarget | null, toolbarPos: { x: number; y: number } | null) => void) | null
  ): void {
    this.onSelectionChanged = cb;
  }

  public notifySelectionChanged(sel: SelectionTarget | null, toolbarPos: { x: number; y: number } | null): void {
    this.onSelectionChanged?.(sel, toolbarPos);
  }

  /**
   * HOP 자체 UI(파일 > 열기, 최근 문서, 드래그&드롭 등)로 문서가 열렸을 때 호출됩니다.
   * AI 스토어(useDocStore)가 같은 문서를 스테이징 파일로 인식하도록 연결하는 용도이며,
   * AI/스토어가 프로그램적으로 로드한 경우(loadDocumentIntoHop)에는 알리지 않습니다.
   */
  public notifyHopDocumentInitialized(info: { fileName: string; sourceBytes: Uint8Array | null }): void {
    if (this.isProgrammaticReload) return;
    this.onHopDocumentOpened?.(info);
  }

  public setHopDocumentOpenedListener(
    cb: ((info: { fileName: string; sourceBytes: Uint8Array | null }) => void) | null
  ): void {
    this.onHopDocumentOpened = cb;
  }

  public async loadDocumentIntoHop(bytes: Uint8Array, fileName: string): Promise<void> {
    const rt = await this.waitForReady();
    this.isProgrammaticReload = true;
    try {
      await rt.openBytesInHop(bytes, fileName);
    } finally {
      setTimeout(() => {
        this.isProgrammaticReload = false;
      }, 60);
    }
  }

  public exportHopBytes(isHwpx: boolean): Uint8Array | null {
    if (!this.handle || !this.handle.wasm.hasLoadedDocument()) return null;
    try {
      return isHwpx ? this.handle.wasm.exportHwpx() : this.handle.wasm.exportHwp();
    } catch (e) {
      console.warn("[HopBridge] exportHopBytes failed:", e);
      return null;
    }
  }

  /**
   * HOP WASM에서 실제 표 셀들의 바운딩박스(pageIndex, x, y, w, h)를 조회해 tablesMeta에 병합합니다.
   */
  public enrichTablesMetaWithBboxes(tablesMeta: TableCoordinateMeta[]): TableCoordinateMeta[] {
    if (!this.handle || !this.handle.wasm.hasLoadedDocument()) return tablesMeta;
    const wasm = this.handle.wasm;

    let tableControls: Array<{ list: number; para: number; controlIndex: number }> = [];
    try {
      const allCtrls = wasm.getControls();
      tableControls = allCtrls.filter((c) => c.ctrlId === "tbl " || c.ctrlId === "tbl");
    } catch {}

    let topIdx = 0;
    return tablesMeta.map((table) => {
      if (table.depth && table.depth > 0) {
        return table;
      }
      const ctrlRef = tableControls[topIdx++];
      const sec = ctrlRef?.list ?? table.sec ?? 0;
      const para = ctrlRef?.para ?? table.para ?? 0;
      const ctrl = ctrlRef?.controlIndex ?? table.ctrl ?? 0;

      let bboxes: CellBBox[] = [];
      try {
        const rawBboxes = wasm.getTableCellBboxes(sec, para, ctrl);
        if (Array.isArray(rawBboxes)) {
          bboxes = rawBboxes.map((b: any) => ({
            cellIdx: b.cellIdx ?? 0,
            row: b.row ?? 0,
            col: b.col ?? 0,
            rowSpan: b.rowSpan ?? 1,
            colSpan: b.colSpan ?? 1,
            pageIndex: b.pageIndex ?? 0,
            x: b.x ?? 0,
            y: b.y ?? 0,
            w: b.w ?? 80,
            h: b.h ?? 24,
          }));
        }
      } catch {}

      return {
        ...table,
        sec,
        para,
        ctrl,
        cells: table.cells.map((cell) => {
          const matchedBbox =
            bboxes.find((b) => b.row === cell.row && b.col === cell.col) ||
            bboxes.find((b) => b.cellIdx === cell.idx) ||
            cell.bbox;
          return {
            ...cell,
            bbox: matchedBbox,
          };
        }),
      };
    });
  }

  /**
   * HOP의 page 좌표를 #scroll-container 기준 화면 좌표로 변환합니다.
   */
  public pageBoxToContainerCoords(pageIndex: number, pageX: number, pageY: number): { x: number; y: number } | null {
    if (!this.handle) return null;
    const scrollContainer = document.getElementById("scroll-container");
    const scrollContent = document.getElementById("scroll-content");
    if (!scrollContainer || !scrollContent) return null;

    const vs = this.handle.canvasView.getVirtualScroll();
    const zoom = this.handle.canvasView.getViewportManager().getZoom();
    const pageTop = vs.getPageOffset(pageIndex);
    const pageLeft = vs.getPageLeftResolved(pageIndex, scrollContent.clientWidth);

    const xInContent = pageLeft + pageX * zoom;
    const yInContent = pageTop + pageY * zoom;

    return {
      x: xInContent - scrollContainer.scrollLeft,
      y: yInContent - scrollContainer.scrollTop,
    };
  }

  /**
   * HOP의 현재 선택 상태(표 셀 블록 선택, 셀 내부 텍스트 드래그, 본문 텍스트 드래그)를 읽어
   * SelectionTarget 및 인라인 플로팅 툴바 위치를 반환합니다.
   */
  public readCurrentSelection(tablesMeta: TableCoordinateMeta[]): {
    selection: SelectionTarget | null;
    toolbarPos: { x: number; y: number } | null;
  } {
    if (!this.handle || !this.handle.wasm.hasLoadedDocument()) {
      return { selection: null, toolbarPos: null };
    }

    const { wasm, inputHandler } = this.handle;
    const anyInput = inputHandler as any;
    const cursor = anyInput.cursor;
    if (!cursor) return { selection: null, toolbarPos: null };

    // 1. Check if a multi-cell block is selected in a table
    const cellBlock = typeof anyInput.getSelectedCellBlock === "function" ? anyInput.getSelectedCellBlock() : null;
    if (cellBlock && typeof cellBlock.sec === "number") {
      const tableIdx = Math.max(
        0,
        tablesMeta.findIndex(
          (t) => t.sec === cellBlock.sec && t.para === cellBlock.ppi && t.ctrl === cellBlock.ci
        )
      );
      const tableMeta = tablesMeta[tableIdx];
      let bboxes: CellBBox[] = [];
      try {
        bboxes = wasm.getTableCellBboxes(cellBlock.sec, cellBlock.ppi, cellBlock.ci) as any;
      } catch {}

      const startRow = cellBlock.startRow ?? 0;
      const endRow = cellBlock.endRow ?? startRow;
      const startCol = cellBlock.startCol ?? 0;
      const endCol = cellBlock.endCol ?? startCol;

      const selectedCells = (tableMeta?.cells || [])
        .filter((c) => c.row >= startRow && c.row <= endRow && c.col >= startCol && c.col <= endCol)
        .map((c) => {
          const bbox =
            c.bbox ||
            bboxes.find((b) => b.row === c.row && b.col === c.col) || {
              cellIdx: c.idx,
              row: c.row,
              col: c.col,
              rowSpan: c.rowSpan,
              colSpan: c.colSpan,
              pageIndex: 0,
              x: 100,
              y: 120,
              w: 120,
              h: 28,
            };
          return {
            cellIdx: c.idx,
            row: c.row,
            col: c.col,
            text: c.text,
            bbox,
          };
        });

      const firstBbox = selectedCells[0]?.bbox || bboxes[0] || { pageIndex: 0, x: 120, y: 120, w: 120, h: 28 };
      const unionRect = {
        pageIndex: firstBbox.pageIndex,
        x: firstBbox.x,
        y: firstBbox.y,
        width: firstBbox.w,
        height: firstBbox.h,
      };
      const pos = this.pageBoxToContainerCoords(
        unionRect.pageIndex,
        unionRect.x + unionRect.width / 2,
        Math.max(10, unionRect.y - 8)
      );

      return {
        selection: {
          kind: "table_cells",
          tableIndex: tableIdx,
          sec: cellBlock.sec,
          para: cellBlock.ppi,
          ctrl: cellBlock.ci,
          startRow,
          startCol,
          endRow,
          endCol,
          selectedCells,
          unionRect,
        },
        toolbarPos: pos,
      };
    }

    // 2. Check ordered text selection (either in a cell or in body paragraphs)
    const ordered = inputHandler.getSelection();
    if (ordered) {
      const { start, end } = ordered;
      const isCollapsed =
        start.sectionIndex === end.sectionIndex &&
        start.paragraphIndex === end.paragraphIndex &&
        start.charOffset === end.charOffset &&
        start.parentParaIndex === end.parentParaIndex &&
        start.cellIndex === end.cellIndex &&
        start.cellParaIndex === end.cellParaIndex;

      if (!isCollapsed) {
        // 2a. Selection inside a table cell -> expose as table_cells selection for that cell so [표 채우기/계산] & [공문서 어투] both work!
        if (start.parentParaIndex !== undefined && start.controlIndex !== undefined && start.cellIndex !== undefined) {
          const sec = start.sectionIndex;
          const ppi = start.parentParaIndex;
          const ci = start.controlIndex;
          const cellIdx = start.cellIndex;

          const tableIdx = Math.max(
            0,
            tablesMeta.findIndex((t) => t.sec === sec && t.para === ppi && t.ctrl === ci)
          );
          const tableMeta = tablesMeta[tableIdx];
          let bboxes: CellBBox[] = [];
          try {
            bboxes = wasm.getTableCellBboxes(sec, ppi, ci) as any;
          } catch {}

          const matchedBbox =
            bboxes.find((b) => b.cellIdx === cellIdx) || {
              cellIdx,
              row: 0,
              col: 0,
              rowSpan: 1,
              colSpan: 1,
              pageIndex: 0,
              x: 120,
              y: 120,
              w: 140,
              h: 28,
            };
          const matchedCell = tableMeta?.cells.find(
            (c) => c.idx === cellIdx || (c.row === matchedBbox.row && c.col === matchedBbox.col)
          );

          let cellText = matchedCell?.text || "";
          if (!cellText) {
            try {
              const len = wasm.getCellParagraphLength(sec, ppi, ci, cellIdx, start.cellParaIndex ?? 0);
              cellText = wasm.getTextInCell(sec, ppi, ci, cellIdx, start.cellParaIndex ?? 0, 0, len);
            } catch {}
          }

          const pos = this.pageBoxToContainerCoords(
            matchedBbox.pageIndex,
            matchedBbox.x + matchedBbox.w / 2,
            Math.max(10, matchedBbox.y - 8)
          );

          return {
            selection: {
              kind: "table_cells",
              tableIndex: tableIdx,
              sec,
              para: ppi,
              ctrl: ci,
              startRow: matchedBbox.row,
              startCol: matchedBbox.col,
              endRow: matchedBbox.row,
              endCol: matchedBbox.col,
              selectedCells: [
                {
                  cellIdx,
                  row: matchedBbox.row,
                  col: matchedBbox.col,
                  text: cellText,
                  bbox: matchedBbox,
                },
              ],
              unionRect: {
                pageIndex: matchedBbox.pageIndex,
                x: matchedBbox.x,
                y: matchedBbox.y,
                width: matchedBbox.w,
                height: matchedBbox.h,
              },
            },
            toolbarPos: pos,
          };
        }

        // 2b. Body text selection
        let selectedText = "";
        try {
          const parts: string[] = [];
          for (let p = start.paragraphIndex; p <= end.paragraphIndex; p++) {
            const pLen = wasm.getParagraphLength(start.sectionIndex, p);
            const from = p === start.paragraphIndex ? start.charOffset : 0;
            const to = p === end.paragraphIndex ? end.charOffset : pLen;
            if (to > from) {
              const slice = (wasm as any).getText(start.sectionIndex, p, from, to - from);
              if (slice) parts.push(slice);
            }
          }
          selectedText = parts.join("\n").trim();
        } catch {}

        if (selectedText) {
          let rect = { pageIndex: 0, x: 120, y: 100, width: 220, height: 20 };
          try {
            const cRect = wasm.getCursorRect(start.sectionIndex, start.paragraphIndex, start.charOffset);
            if (cRect) {
              rect = {
                pageIndex: cRect.pageIndex ?? 0,
                x: cRect.x ?? 120,
                y: cRect.y ?? 100,
                width: 240,
                height: cRect.height ?? 20,
              };
            }
          } catch {}

          const pos = this.pageBoxToContainerCoords(
            rect.pageIndex,
            rect.x + rect.width / 2,
            Math.max(10, rect.y - 8)
          );

          return {
            selection: {
              kind: "body_range",
              sec: start.sectionIndex,
              startPara: start.paragraphIndex,
              startOffset: start.charOffset,
              endPara: end.paragraphIndex,
              endOffset: end.charOffset,
              selectedText,
              rects: [rect],
            },
            toolbarPos: pos,
          };
        }
      }
    }

    return { selection: null, toolbarPos: null };
  }
}

export const hopBridge = new HopBridgeManager();
