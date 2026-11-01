import type { UnifiedOp } from "../types/operations.ts";
import type { TableCoordinateMeta } from "../types/hwp.ts";

export interface EnginePayload {
  isHwpx: boolean;
  stdinJson: string;
}

export class OpTranslator {
  /**
   * AI가 제안한 텍스트에 포함된 마크다운 별표(**) 제거 및 정제 (Known Pitfall 1)
   */
  public static sanitizeMarkdownFormatting(text: string): string {
    if (!text) return text;
    return text.replace(/\*\*(.*?)\*\*/g, "$1").replace(/\*(.*?)\*/g, "$1");
  }

  /**
   * UnifiedOp 배열을 받아 .hwp용 create.js 페이로드 또는 .hwpx용 hwpx-edit.js 페이로드로 변환합니다.
   */
  static toEnginePayload(
    stagingFilePath: string,
    isHwpx: boolean,
    ops: UnifiedOp[],
    tablesMeta: TableCoordinateMeta[],
    isNewDocument = false
  ): EnginePayload {
    if (isHwpx) {
      const hwpxOps = this.toHwpxOps(ops, isNewDocument);
      if (isNewDocument) {
        return {
          isHwpx: false, // create.js 호출
          stdinJson: JSON.stringify({
            path: stagingFilePath,
            operations: this.toHwpCreateOps(ops, tablesMeta, true),
          }),
        };
      }
      return {
        isHwpx: true, // hwpx-edit.js 호출
        stdinJson: JSON.stringify({
          path: stagingFilePath,
          output: stagingFilePath,
          operations: hwpxOps,
        }),
      };
    }

    // .hwp 파일 (create.js 사용)
    const hwpOps = this.toHwpCreateOps(ops, tablesMeta, isNewDocument);
    return {
      isHwpx: false,
      stdinJson: JSON.stringify({
        path: stagingFilePath,
        operations: hwpOps,
      }),
    };
  }

  private static resolveHwpTableCoords(
    tableIndex: number,
    tablesMeta: TableCoordinateMeta[],
    op: { sec?: number; para?: number; ctrl?: number }
  ) {
    if (typeof op.sec === "number" && typeof op.para === "number" && typeof op.ctrl === "number") {
      return { section: op.sec, para: op.para, control: op.ctrl };
    }
    const meta = tablesMeta[tableIndex];
    if (!meta) {
      // Fallback: if table index is out of bounds (e.g. newly added table), assume sec:0, para:0, ctrl:0
      return { section: 0, para: 0, control: 0 };
    }
    return { section: meta.sec, para: meta.para, control: meta.ctrl };
  }

  /**
   * .hwp 전용: create.js / cell-patch.js 오퍼레이션 변환
   */
  private static toHwpCreateOps(
    ops: UnifiedOp[],
    tablesMeta: TableCoordinateMeta[],
    isNewDocument: boolean
  ): any[] {
    const out: any[] = [];

    if (isNewDocument) {
      out.push({ type: "setup_document", page_size: "a4", orientation: "portrait", margin_mm: 20 });
    }

    for (const op of ops) {
      switch (op.type) {
        case "replace_body_text":
          if (op.find) {
            out.push({
              type: "replace_text",
              query: op.find,
              replacement: this.sanitizeMarkdownFormatting(op.replace),
            });
          }
          break;

        case "set_paragraph_text":
          if (!op.oldTextForHwp) {
            // If oldTextForHwp is not provided, append as paragraph rather than empty query replace_text
            if (op.text) {
              out.push({
                type: "append_paragraph",
                text: this.sanitizeMarkdownFormatting(op.text),
              });
            }
          } else {
            out.push({
              type: "replace_text",
              query: op.oldTextForHwp,
              replacement: this.sanitizeMarkdownFormatting(op.text),
            });
          }
          break;

        case "append_paragraph":
          out.push({
            type: "append_paragraph",
            text: this.sanitizeMarkdownFormatting(op.text),
            ...(op.align ? { align: op.align } : {}),
          });
          break;

        case "append_heading":
          out.push({
            type: "append_heading",
            level: op.level,
            text: this.sanitizeMarkdownFormatting(op.text),
          });
          break;

        case "set_cell_text": {
          const coords = this.resolveHwpTableCoords(op.tableIndex, tablesMeta, op);
          out.push({
            type: "set_cell_text",
            ...coords,
            row: op.row,
            col: op.col,
            text: this.sanitizeMarkdownFormatting(op.text),
            ...(op.fit ? { fit: true } : {}),
          });
          break;
        }

        case "insert_table": {
          // .hwp append_table: headers + rows
          out.push({
            type: "append_table",
            headers: op.headers.map((h) => this.sanitizeMarkdownFormatting(h)),
            rows: op.rows.map((row) => row.map((c) => this.sanitizeMarkdownFormatting(c))),
          });
          break;
        }

        case "insert_table_row": {
          const coords = this.resolveHwpTableCoords(op.tableIndex, tablesMeta, {});
          out.push({
            type: "insert_table_row",
            ...coords,
            row: op.row,
            position: op.position,
          });
          if (op.cells && op.cells.length > 0) {
            const targetRow = op.position === "below" ? op.row + 1 : op.row;
            op.cells.forEach((cellText, colIdx) => {
              if (cellText !== "") {
                out.push({
                  type: "set_cell_text",
                  ...coords,
                  row: targetRow,
                  col: colIdx,
                  text: this.sanitizeMarkdownFormatting(cellText),
                });
              }
            });
          }
          break;
        }

        case "delete_table_row": {
          const coords = this.resolveHwpTableCoords(op.tableIndex, tablesMeta, {});
          out.push({ type: "delete_table_row", ...coords, row: op.row });
          break;
        }

        case "insert_table_col": {
          const coords = this.resolveHwpTableCoords(op.tableIndex, tablesMeta, {});
          out.push({
            type: "insert_table_col",
            ...coords,
            col: op.col,
            position: op.position,
          });
          break;
        }

        case "delete_table_col": {
          const coords = this.resolveHwpTableCoords(op.tableIndex, tablesMeta, {});
          out.push({ type: "delete_table_col", ...coords, col: op.col });
          break;
        }

        case "merge_cells": {
          const coords = this.resolveHwpTableCoords(op.tableIndex, tablesMeta, {});
          out.push({
            type: "merge_cells",
            ...coords,
            from_row: op.fromRow,
            from_col: op.fromCol,
            to_row: op.toRow,
            to_col: op.toCol,
          });
          break;
        }

        case "unmerge_cells": {
          const coords = this.resolveHwpTableCoords(op.tableIndex, tablesMeta, {});
          out.push({ type: "unmerge_cells", ...coords, row: op.row, col: op.col });
          break;
        }

        case "split_cell": {
          const coords = this.resolveHwpTableCoords(op.tableIndex, tablesMeta, {});
          out.push({
            type: "split_cell",
            ...coords,
            row: op.row,
            col: op.col,
            into_rows: op.intoRows,
          });
          break;
        }

        case "equalize_table": {
          const coords = this.resolveHwpTableCoords(op.tableIndex, tablesMeta, {});
          if (op.mode === "width" || op.mode === "both") {
            out.push({ type: "equalize_table_columns", ...coords });
          }
          if (op.mode === "height" || op.mode === "both") {
            out.push({ type: "equalize_table_rows", ...coords });
          }
          break;
        }

        case "set_cell_background": {
          const coords = this.resolveHwpTableCoords(op.tableIndex, tablesMeta, {});
          out.push({
            type: "set_cell_background",
            ...coords,
            row: op.row,
            col: op.col,
            background_color: op.color,
          });
          break;
        }

        case "set_cell_border": {
          const coords = this.resolveHwpTableCoords(op.tableIndex, tablesMeta, {});
          out.push({
            type: "set_cell_border",
            ...coords,
            row: op.row,
            col: op.col,
            sides: op.sides,
            ...(op.color ? { color: op.color } : {}),
            ...(op.lineType ? { line_type: op.lineType } : {}),
          });
          break;
        }

        case "apply_text_style":
          out.push({
            type: "apply_text_style",
            target: op.targetText,
            ...(op.fontFamily ? { font_family: op.fontFamily } : {}),
            ...(op.sizePt ? { size: op.sizePt } : {}),
            ...(typeof op.bold === "boolean" ? { bold: op.bold } : {}),
            ...(typeof op.italic === "boolean" ? { italic: op.italic } : {}),
            ...(typeof op.underline === "boolean" ? { underline: op.underline } : {}),
            ...(op.color ? { color: op.color } : {}),
            ...(op.highlight !== undefined ? { highlight: op.highlight } : {}),
            ...(op.letterSpacing !== undefined ? { letter_spacing: op.letterSpacing } : {}),
            ...(op.charRatio !== undefined ? { char_ratio: op.charRatio } : {}),
          });
          break;

        case "apply_paragraph_style": {
          const idx = typeof op.paraIndex === "number" && op.paraIndex >= 0 ? op.paraIndex : undefined;
          out.push({
            type: "apply_paragraph_style",
            ...(op.targetText ? { target: op.targetText } : { index: idx ?? 0 }),
            ...(op.align ? { align: op.align } : {}),
            ...(op.indentHu !== undefined ? { indent: op.indentHu } : {}),
            ...(op.lineSpacingPercent !== undefined ? { line_spacing: op.lineSpacingPercent } : {}),
            ...(op.spacingBeforeHu !== undefined ? { spacing_before: op.spacingBeforeHu } : {}),
            ...(op.spacingAfterHu !== undefined ? { spacing_after: op.spacingAfterHu } : {}),
          });
          break;
        }

        case "set_list_style":
          out.push({
            type: op.mode === "numbered" ? "set_numbered_list" : "set_bullet_list",
            ...(op.paraIndex !== undefined ? { index: op.paraIndex } : {}),
            ...(op.targetText ? { target: op.targetText } : {}),
          });
          break;

        case "insert_image": {
          if (op.cell) {
            const coords = this.resolveHwpTableCoords(op.cell.tableIndex, tablesMeta, {});
            out.push({
              type: "insert_image",
              path: op.path,
              cell: { row: op.cell.row, col: op.cell.col, para: coords.para, control: coords.control },
              ...(op.widthMm ? { width_mm: op.widthMm } : {}),
              ...(op.heightMm ? { height_mm: op.heightMm } : {}),
            });
          } else {
            out.push({
              type: "insert_image",
              path: op.path,
              ...(op.anchorText ? { anchor: op.anchorText } : {}),
              ...(op.widthMm ? { width_mm: op.widthMm } : {}),
              ...(op.heightMm ? { height_mm: op.heightMm } : {}),
            });
          }
          break;
        }

        case "place_seal":
          out.push({
            type: "place_seal",
            anchor: op.anchorText,
            source: op.sealImagePath,
            mode: op.mode || "auto",
          });
          break;

        case "set_header_footer":
          out.push({
            type: op.where === "header" ? "insert_header_text" : "insert_footer_text",
            text: op.text,
          });
          break;

        case "insert_page_number":
          out.push({
            type: "insert_page_number",
            where: op.where || "footer",
            align: op.align || "center",
          });
          break;

        case "insert_footnote":
          out.push({ type: "insert_footnote", anchor: op.anchorText, text: op.text });
          break;

        case "set_columns":
          out.push({ type: "set_columns", count: op.count, ...(op.spacingMm ? { spacing_mm: op.spacingMm } : {}) });
          break;

        case "set_col_width": {
          const coords = this.resolveHwpTableCoords(op.tableIndex, tablesMeta, {});
          out.push({
            type: "set_col_width",
            para: coords.para,
            control: coords.control,
            col: op.col,
            width_mm: op.widthMm,
            ...(op.keepTableWidth !== undefined ? { keep_table_width: op.keepTableWidth } : {}),
          });
          break;
        }

        case "set_row_height": {
          const coords = this.resolveHwpTableCoords(op.tableIndex, tablesMeta, {});
          out.push({
            type: "set_row_height",
            para: coords.para,
            control: coords.control,
            row: op.row,
            height_mm: op.heightMm,
          });
          break;
        }

        case "set_cell_padding": {
          const coords = this.resolveHwpTableCoords(op.tableIndex, tablesMeta, {});
          out.push({
            type: "set_cell_padding",
            para: coords.para,
            control: coords.control,
            row: op.row,
            col: op.col,
            ...(op.leftMm !== undefined ? { left_mm: op.leftMm } : {}),
            ...(op.rightMm !== undefined ? { right_mm: op.rightMm } : {}),
            ...(op.topMm !== undefined ? { top_mm: op.topMm } : {}),
            ...(op.bottomMm !== undefined ? { bottom_mm: op.bottomMm } : {}),
            ...(op.marginMm !== undefined ? { margin_mm: op.marginMm } : {}),
          });
          break;
        }

        case "fit_table_to_page": {
          const coords = this.resolveHwpTableCoords(op.tableIndex, tablesMeta, {});
          out.push({
            type: "fit_table_to_page",
            para: coords.para,
            control: coords.control,
            ...(op.targetWidthMm !== undefined ? { target_width_mm: op.targetWidthMm } : {}),
          });
          break;
        }

        case "set_page_margin": {
          out.push({
            type: "set_page_margin",
            ...(op.leftMm !== undefined ? { left_mm: op.leftMm } : {}),
            ...(op.rightMm !== undefined ? { right_mm: op.rightMm } : {}),
            ...(op.topMm !== undefined ? { top_mm: op.topMm } : {}),
            ...(op.bottomMm !== undefined ? { bottom_mm: op.bottomMm } : {}),
            ...(op.marginMm !== undefined ? { margin_mm: op.marginMm } : {}),
          });
          break;
        }

        case "shrink_cell_font": {
          const coords = this.resolveHwpTableCoords(op.tableIndex, tablesMeta, {});
          out.push({
            type: "shrink_cell_font",
            para: coords.para,
            control: coords.control,
            row: op.row,
            col: op.col,
            size_pt: op.sizePt,
            ...(op.minPt !== undefined ? { min_pt: op.minPt } : {}),
          });
          break;
        }
      }
    }

    return out;
  }

  /**
   * .hwpx 전용: hwpx-edit.js 오퍼레이션 변환
   */
  private static toHwpxOps(ops: UnifiedOp[], _isNewDocument: boolean): any[] {
    const out: any[] = [];

    for (const op of ops) {
      switch (op.type) {
        case "replace_body_text":
          if (op.find) {
            out.push({
              type: "replace_text",
              find: op.find,
              replace: this.sanitizeMarkdownFormatting(op.replace),
            });
          }
          break;

        case "set_paragraph_text":
          out.push({
            type: "set_paragraph_text",
            index: op.paraIndex ?? 0,
            text: this.sanitizeMarkdownFormatting(op.text),
          });
          break;

        case "append_paragraph": {
          const cleanText = this.sanitizeMarkdownFormatting(op.text);
          out.push({ type: "append_paragraph", text: cleanText });
          if (op.align) {
            out.push({
              type: "apply_paragraph_style",
              index: -1,
              ...(cleanText ? { target: cleanText } : {}),
              align: op.align,
            });
          }
          break;
        }

        case "append_heading": {
          const cleanHeading = this.sanitizeMarkdownFormatting(op.text);
          out.push({ type: "append_paragraph", text: cleanHeading });
          const headingSizes: Record<number, number> = { 1: 16, 2: 14, 3: 12, 4: 11 };
          if (cleanHeading) {
            out.push({
              type: "apply_text_style",
              target: cleanHeading,
              bold: true,
              size_pt: headingSizes[op.level] || 13,
            });
          }
          break;
        }

        case "set_cell_text":
          out.push({
            type: "set_cell_text",
            table: op.tableIndex,
            row: op.row,
            col: op.col,
            text: this.sanitizeMarkdownFormatting(op.text),
            ...(op.fit ? { fit: true } : {}),
          });
          break;

        case "insert_table": {
          const allCells = [
            op.headers.map((h) => this.sanitizeMarkdownFormatting(h)),
            ...op.rows.map((r) => r.map((c) => this.sanitizeMarkdownFormatting(c))),
          ];
          out.push({
            type: "insert_table",
            index: op.afterParaIndex ?? -1,
            rows: allCells.length,
            cols: op.headers.length,
            cells: allCells,
          });
          break;
        }

        case "insert_table_row":
          out.push({
            type: "insert_table_row",
            table: op.tableIndex,
            row: op.row,
            where: op.position === "above" ? "before" : "after",
            ...(op.cells ? { cells: op.cells.map((c) => this.sanitizeMarkdownFormatting(c)) } : {}),
          });
          break;

        case "delete_table_row":
          out.push({ type: "delete_table_row", table: op.tableIndex, row: op.row });
          break;

        case "insert_table_col":
          out.push({
            type: "insert_table_column",
            table: op.tableIndex,
            col: op.col,
            where: op.position === "left" ? "before" : "after",
            ...(op.cells ? { cells: op.cells.map((c) => this.sanitizeMarkdownFormatting(c)) } : {}),
          });
          break;

        case "delete_table_col":
          out.push({ type: "delete_table_column", table: op.tableIndex, col: op.col });
          break;

        case "merge_cells": {
          const rowSpan = op.toRow - op.fromRow + 1;
          const colSpan = op.toCol - op.fromCol + 1;
          if (rowSpan === 1 && colSpan >= 2) {
            out.push({
              type: "merge_cells",
              table: op.tableIndex,
              mode: "horizontal",
              row: op.fromRow,
              start: op.fromCol,
              count: colSpan,
            });
          } else if (colSpan === 1 && rowSpan >= 2) {
            out.push({
              type: "merge_cells",
              table: op.tableIndex,
              mode: "vertical",
              col: op.fromCol,
              start: op.fromRow,
              count: rowSpan,
            });
          } else if (rowSpan >= 2 && colSpan >= 2) {
            for (let r = op.fromRow; r <= op.toRow; r++) {
              out.push({
                type: "merge_cells",
                table: op.tableIndex,
                mode: "horizontal",
                row: r,
                start: op.fromCol,
                count: colSpan,
              });
            }
            out.push({
              type: "merge_cells",
              table: op.tableIndex,
              mode: "vertical",
              col: op.fromCol,
              start: op.fromRow,
              count: rowSpan,
            });
          }
          break;
        }

        case "unmerge_cells":
          out.push({ type: "unmerge_cells", table: op.tableIndex, row: op.row, col: op.col });
          break;

        case "split_cell":
          out.push({
            type: "split_cell",
            table: op.tableIndex,
            row: op.row,
            col: op.col,
            rows: op.intoRows,
            cols: op.intoCols ?? 1,
          });
          break;

        case "equalize_table":
          out.push({ type: "distribute_table", table: op.tableIndex, mode: op.mode });
          break;

        case "set_cell_background":
          out.push({
            type: "set_cell_background",
            table: op.tableIndex,
            row: op.row,
            col: op.col,
            color: op.color,
            mode: "both",
          });
          break;

        case "set_cell_border": {
          const sides =
            op.sides === "all"
              ? ["LEFT", "RIGHT", "TOP", "BOTTOM"]
              : op.sides.map((s) => s.toUpperCase());
          out.push({
            type: "set_cell_border",
            table: op.tableIndex,
            row: op.row,
            col: op.col,
            color: op.color || "#000000",
            width: op.widthMm ? `${op.widthMm} mm` : "0.3 mm",
            sides,
          });
          break;
        }

        case "apply_text_style":
          out.push({
            type: "apply_text_style",
            target: op.targetText,
            ...(op.fontFamily ? { fontFace: op.fontFamily } : {}),
            ...(op.sizePt ? { size_pt: op.sizePt } : {}), // size_pt MUST be used for .hwpx!
            ...(typeof op.bold === "boolean" ? { bold: op.bold } : {}),
            ...(typeof op.italic === "boolean" ? { italic: op.italic } : {}),
            ...(typeof op.underline === "boolean" ? { underline: op.underline } : {}),
            ...(op.color ? { color: op.color.replace(/^#/, "") } : {}),
            ...(op.highlight !== undefined ? { highlight: op.highlight } : {}),
            ...(op.letterSpacing !== undefined ? { letter_spacing: op.letterSpacing } : {}),
            ...(op.charRatio !== undefined ? { char_ratio: op.charRatio } : {}),
          });
          break;

        case "apply_paragraph_style":
          out.push({
            type: "apply_paragraph_style",
            index: op.paraIndex === "last" ? -1 : (op.paraIndex ?? 0),
            ...(op.targetText ? { target: op.targetText } : {}),
            ...(op.align ? { align: op.align } : {}),
            ...(op.indentHu !== undefined ? { indent: op.indentHu } : {}),
            ...(op.lineSpacingPercent !== undefined ? { lineSpacing: op.lineSpacingPercent } : {}),
            ...(op.spacingBeforeHu !== undefined ? { spacing_before: op.spacingBeforeHu } : {}),
            ...(op.spacingAfterHu !== undefined ? { spacing_after: op.spacingAfterHu } : {}),
          });
          break;

        case "set_list_style":
          out.push({
            type: op.mode === "numbered" ? "set_number_list" : "set_bullet_list",
            index: op.paraIndex ?? 0,
            ...(op.targetText ? { target: op.targetText } : {}),
          });
          break;

        case "insert_image":
          if (op.cell) {
            out.push({
              type: "set_cell_image",
              table: op.cell.tableIndex,
              row: op.cell.row,
              col: op.cell.col,
              source: op.path,
              ...(op.widthMm ? { width_mm: op.widthMm } : {}),
              ...(op.heightMm ? { height_mm: op.heightMm } : {}),
            });
          } else {
            out.push({
              type: "insert_image",
              source: op.path,
              ...(op.widthMm ? { width_mm: op.widthMm } : {}),
              ...(op.heightMm ? { height_mm: op.heightMm } : {}),
            });
          }
          break;

        case "place_seal":
          out.push({
            type: "place_seal",
            anchor: op.anchorText,
            source: op.sealImagePath,
            mode: op.mode || "auto",
          });
          break;

        case "set_header_footer":
          out.push({
            type: op.where === "header" ? "set_header" : "set_footer",
            text: op.text,
          });
          break;

        case "insert_page_number":
          out.push({
            type: "set_page_number",
            where: op.where || "footer",
            align: (op.align || "center").toUpperCase(),
          });
          break;

        case "insert_footnote":
          out.push({
            type: "insert_footnote",
            index: op.paraIndex ?? -1,
            anchor: op.anchorText,
            text: op.text,
          });
          break;

        case "set_columns":
          out.push({ type: "set_columns", count: op.count, ...(op.spacingMm ? { gap_mm: op.spacingMm } : {}) });
          break;

        case "set_col_width": {
          out.push({
            type: "set_col_width",
            table: op.tableIndex,
            col: op.col,
            width_mm: op.widthMm,
            ...(op.keepTableWidth !== undefined ? { keep_table_width: op.keepTableWidth } : {}),
          });
          break;
        }

        case "set_row_height": {
          out.push({
            type: "set_row_height",
            table: op.tableIndex,
            row: op.row,
            height_mm: op.heightMm,
          });
          break;
        }

        case "set_cell_padding": {
          out.push({
            type: "set_cell_padding",
            table: op.tableIndex,
            row: op.row,
            col: op.col,
            ...(op.leftMm !== undefined ? { left_mm: op.leftMm } : {}),
            ...(op.rightMm !== undefined ? { right_mm: op.rightMm } : {}),
            ...(op.topMm !== undefined ? { top_mm: op.topMm } : {}),
            ...(op.bottomMm !== undefined ? { bottom_mm: op.bottomMm } : {}),
            ...(op.marginMm !== undefined ? { margin_mm: op.marginMm } : {}),
          });
          break;
        }

        case "fit_table_to_page": {
          out.push({
            type: "fit_table_to_page",
            table: op.tableIndex,
            ...(op.targetWidthMm !== undefined ? { target_width_mm: op.targetWidthMm } : {}),
          });
          break;
        }

        case "set_page_margin": {
          out.push({
            type: "set_page_margin",
            ...(op.leftMm !== undefined ? { left_mm: op.leftMm } : {}),
            ...(op.rightMm !== undefined ? { right_mm: op.rightMm } : {}),
            ...(op.topMm !== undefined ? { top_mm: op.topMm } : {}),
            ...(op.bottomMm !== undefined ? { bottom_mm: op.bottomMm } : {}),
            ...(op.marginMm !== undefined ? { margin_mm: op.marginMm } : {}),
          });
          break;
        }

        case "shrink_cell_font": {
          out.push({
            type: "shrink_cell_font",
            table: op.tableIndex,
            row: op.row,
            col: op.col,
            size_pt: op.sizePt,
            ...(op.minPt !== undefined ? { min_pt: op.minPt } : {}),
          });
          break;
        }
      }
    }

    return out;
  }
}
