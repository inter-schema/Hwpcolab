import { unzipSync, strFromU8 } from "fflate";
import type { TableCoordinateMeta, CellBBox } from "../types/hwp.ts";

export interface ScannedElement {
  start: number;
  end: number;
  openEnd: number;
  attrs: string;
  inner: string;
  selfClosing?: boolean;
}

/**
 * Scan top-level matching XML tags, skipping nested same-name tags.
 * Mirrors scanTopLevel from claw-hwp/scripts/hwpx-edit.js.
 */
export function scanTopLevel(xml: string, tag: string): ScannedElement[] {
  const t = tag.replace(/:/g, "\\:");
  const re = new RegExp(`<${t}((?:\\s[^>]*?)?)(/?)>|</${t}\\s*>`, "g");
  const out: ScannedElement[] = [];
  let depth = 0;
  let start = -1;
  let openEnd = -1;
  let attrs = "";
  let m: RegExpExecArray | null;

  while ((m = re.exec(xml)) !== null) {
    const isClose = m[0].startsWith("</");
    if (!isClose && m[2] === "/") {
      if (depth === 0) {
        out.push({
          start: m.index,
          end: re.lastIndex,
          openEnd: re.lastIndex,
          attrs: m[1] || "",
          inner: "",
          selfClosing: true,
        });
      }
      continue;
    }
    if (!isClose) {
      if (depth === 0) {
        start = m.index;
        openEnd = re.lastIndex;
        attrs = m[1] || "";
      }
      depth++;
    } else {
      if (depth > 0) {
        depth--;
        if (depth === 0) {
          out.push({
            start,
            end: re.lastIndex,
            openEnd,
            attrs,
            inner: xml.slice(openEnd, m.index),
          });
        }
      }
    }
  }
  return out;
}

export function xmlUnescape(s: string): string {
  return String(s)
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&amp;/g, "&");
}

export interface HwpxParsedCell {
  idx: number;
  row: number;
  col: number;
  rowSpan: number;
  colSpan: number;
  text: string;
  innerStart: number;
  innerEnd: number;
}

export interface HwpxParsedTable {
  sectionIndex: number;
  tableIndex: number; // document pre-order index
  depth: number;
  parentTableIndex?: number;
  parentCell?: { row: number; col: number };
  rowCount: number;
  colCount: number;
  cells: HwpxParsedCell[];
}

/**
 * Parses all tables from an HWPX section XML string in pre-order traversal order,
 * computing nesting depth and parent cell coordinates.
 */
export function parseHwpxSectionTables(
  xml: string,
  sectionIndex: number,
  startIndexOffset = 0
): HwpxParsedTable[] {
  const result: HwpxParsedTable[] = [];

  function walk(
    xmlFragment: string,
    depth: number,
    parentTableIndex?: number,
    parentCell?: { row: number; col: number }
  ) {
    const tblList = scanTopLevel(xmlFragment, "hp:tbl");
    for (const tbl of tblList) {
      const currentTableIndex = startIndexOffset + result.length;

      // Scan rows and cells
      const trList = scanTopLevel(tbl.inner, "hp:tr");
      const cells: HwpxParsedCell[] = [];
      let cellIdx = 0;
      let maxRow = 0;
      let maxCol = 0;

      for (let ri = 0; ri < trList.length; ri++) {
        const rw = trList[ri];
        const tcList = scanTopLevel(rw.inner, "hp:tc");

        for (let ci = 0; ci < tcList.length; ci++) {
          const tc = tcList[ci];
          const innerStart = rw.openEnd + tc.start;
          const innerEnd = rw.openEnd + tc.end;
          const subEnd = tc.inner.lastIndexOf("</hp:subList>");
          const directInner = subEnd >= 0 ? tc.inner.slice(subEnd) : tc.inner;

          const am = directInner.match(
            /<hp:cellAddr [^>]*colAddr="(\d+)"[^>]*rowAddr="(\d+)"|<hp:cellAddr [^>]*rowAddr="(\d+)"[^>]*colAddr="(\d+)"/
          );
          const col = am
            ? am[1] !== undefined
              ? Number(am[1])
              : Number(am[4])
            : ci;
          const row = am
            ? am[2] !== undefined
              ? Number(am[2])
              : Number(am[3])
            : ri;

          const sm = directInner.match(/<hp:cellSpan [^>]*colSpan="(\d+)"[^>]*rowSpan="(\d+)"/);
          const colSpan = sm ? Number(sm[1]) : 1;
          const rowSpan = sm ? Number(sm[2]) : 1;

          const text = xmlUnescape(
            (tc.inner.match(/<hp:t>([^<]*)<\/hp:t>/g) || [])
              .map((x) => x.replace(/<\/?hp:t>/g, ""))
              .join("")
          );

          maxRow = Math.max(maxRow, row + rowSpan);
          maxCol = Math.max(maxCol, col + colSpan);

          cells.push({
            idx: cellIdx++,
            row,
            col,
            rowSpan,
            colSpan,
            text,
            innerStart,
            innerEnd,
          });
        }
      }

      if (maxRow === 0) maxRow = trList.length;

      const parsedTable: HwpxParsedTable = {
        sectionIndex,
        tableIndex: currentTableIndex,
        depth,
        parentTableIndex,
        parentCell,
        rowCount: maxRow,
        colCount: maxCol,
        cells,
      };

      result.push(parsedTable);

      // Now check for child tables nested within this table's cells
      const childTbls = scanTopLevel(tbl.inner, "hp:tbl");
      for (const childTbl of childTbls) {
        // Find which cell contains this child table
        const parentC = cells.find(
          (c) => childTbl.start >= c.innerStart && childTbl.end <= c.innerEnd
        );
        const childParentCell = parentC ? { row: parentC.row, col: parentC.col } : undefined;

        walk(
          tbl.inner.slice(childTbl.start, childTbl.end),
          depth + 1,
          currentTableIndex,
          childParentCell
        );
      }
    }
  }

  walk(xml, 0);
  return result;
}

/**
 * Extracts and parses all tables across all section XML files from HWPX package bytes.
 */
export function parseHwpxTablesFromBytes(fileBytes: Uint8Array): HwpxParsedTable[] {
  const unzipped = unzipSync(fileBytes);
  const sectionFiles = Object.keys(unzipped)
    .filter((n) => /^Contents\/section\d+\.xml$/i.test(n))
    .sort((a, b) => {
      const ma = a.match(/section(\d+)\.xml/i);
      const mb = b.match(/section(\d+)\.xml/i);
      return (ma ? parseInt(ma[1], 10) : 0) - (mb ? parseInt(mb[1], 10) : 0);
    });

  const allTables: HwpxParsedTable[] = [];
  for (let secIdx = 0; secIdx < sectionFiles.length; secIdx++) {
    const name = sectionFiles[secIdx];
    const xml = strFromU8(unzipped[name]);
    const secTables = parseHwpxSectionTables(xml, secIdx, allTables.length);
    allTables.push(...secTables);
  }
  return allTables;
}

/**
 * Merges pre-order HWPX parsed tables with rhwp top-level tables.
 * Top-level tables retain bbox and section/para/control metadata.
 * Nested tables receive depth > 0 and parent coordinate metadata.
 * If mismatch occurs, logs warning and falls back safely to rhwp tables.
 */
export function mergeHwpxTablesWithRhwp(
  hwpxTables: HwpxParsedTable[],
  rhwpTopTables: TableCoordinateMeta[]
): TableCoordinateMeta[] {
  const hwpxTopTables = hwpxTables.filter((t) => t.depth === 0);

  // Mismatch detection
  let mismatch = false;
  if (hwpxTopTables.length !== rhwpTopTables.length) {
    mismatch = true;
  } else {
    for (let i = 0; i < hwpxTopTables.length; i++) {
      if (
        hwpxTopTables[i].rowCount !== rhwpTopTables[i].rowCount ||
        hwpxTopTables[i].colCount !== rhwpTopTables[i].colCount
      ) {
        mismatch = true;
        break;
      }
    }
  }

  if (mismatch) {
    console.warn(
      `[hwpxTableIndex] Table count or dimension mismatch: HWPX has ${hwpxTopTables.length} top-level tables, rhwpEngine found ${rhwpTopTables.length}. Falling back safely to rhwpEngine tables.`
    );
    return rhwpTopTables.map((t) => ({ ...t, depth: 0 }));
  }

  const merged: TableCoordinateMeta[] = [];
  let topIdx = 0;

  for (let i = 0; i < hwpxTables.length; i++) {
    const t = hwpxTables[i];

    if (t.depth === 0) {
      const topMeta = rhwpTopTables[topIdx++];
      merged.push({
        tableIndex: t.tableIndex ?? i,
        sec: topMeta?.sec ?? t.sectionIndex ?? 0,
        para: topMeta?.para ?? 0,
        ctrl: topMeta?.ctrl ?? 0,
        rowCount: t.rowCount,
        colCount: t.colCount,
        depth: 0,
        parentTableIndex: undefined,
        parentCell: undefined,
        cells: t.cells.map((c, cIdx) => {
          const matchingRhwpCell = topMeta?.cells?.find(
            (rc) => rc.row === c.row && rc.col === c.col
          );
          return {
            idx: c.idx ?? cIdx,
            row: c.row,
            col: c.col,
            rowSpan: c.rowSpan,
            colSpan: c.colSpan,
            text: c.text,
            bbox: matchingRhwpCell?.bbox,
          };
        }),
      });
    } else {
      const parentMeta = t.parentTableIndex !== undefined ? merged[t.parentTableIndex] : undefined;
      merged.push({
        tableIndex: t.tableIndex ?? i,
        sec: parentMeta ? parentMeta.sec : t.sectionIndex ?? 0,
        para: parentMeta ? parentMeta.para : 0,
        ctrl: parentMeta ? parentMeta.ctrl : 0,
        rowCount: t.rowCount,
        colCount: t.colCount,
        depth: t.depth,
        parentTableIndex: t.parentTableIndex,
        parentCell: t.parentCell,
        cells: t.cells.map((c, cIdx) => ({
          idx: c.idx ?? cIdx,
          row: c.row,
          col: c.col,
          rowSpan: c.rowSpan,
          colSpan: c.colSpan,
          text: c.text,
          bbox: undefined,
        })),
      });
    }
  }

  return merged;
}

/**
 * Builds full TableCoordinateMeta list (pre-order, depth, parent coordinates)
 * for an HWPX document from package bytes, preserving bboxes from existing/rhwp tables if available.
 */
export function buildTablesMetaForHwpx(
  fileBytes: Uint8Array,
  rhwpTables?: TableCoordinateMeta[]
): TableCoordinateMeta[] {
  let hwpxTables: HwpxParsedTable[];
  try {
    hwpxTables = parseHwpxTablesFromBytes(fileBytes);
  } catch (err) {
    console.warn("[hwpxTableIndex] Failed to parse HWPX tables from bytes:", err);
    return rhwpTables ? rhwpTables.map((t) => ({ ...t, depth: 0 })) : [];
  }

  // If rhwp/existing tables provided with top-level tables, merge them
  if (rhwpTables && rhwpTables.length > 0) {
    const rhwpTopTables = rhwpTables.filter((t) => !t.depth || t.depth === 0);
    if (rhwpTopTables.length > 0) {
      return mergeHwpxTablesWithRhwp(hwpxTables, rhwpTopTables);
    }
  }

  // Otherwise, construct TableCoordinateMeta directly from HWPX parsed tables
  const result: TableCoordinateMeta[] = [];
  for (let i = 0; i < hwpxTables.length; i++) {
    const t = hwpxTables[i];
    if (t.depth === 0) {
      result.push({
        tableIndex: t.tableIndex ?? i,
        sec: t.sectionIndex ?? 0,
        para: 0,
        ctrl: 0,
        rowCount: t.rowCount,
        colCount: t.colCount,
        depth: 0,
        parentTableIndex: undefined,
        parentCell: undefined,
        cells: t.cells.map((c, cIdx) => ({
          idx: c.idx ?? cIdx,
          row: c.row,
          col: c.col,
          rowSpan: c.rowSpan,
          colSpan: c.colSpan,
          text: c.text,
          bbox: undefined,
        })),
      });
    } else {
      const parentMeta = t.parentTableIndex !== undefined ? result[t.parentTableIndex] : undefined;
      result.push({
        tableIndex: t.tableIndex ?? i,
        sec: parentMeta ? parentMeta.sec : t.sectionIndex ?? 0,
        para: parentMeta ? parentMeta.para : 0,
        ctrl: parentMeta ? parentMeta.ctrl : 0,
        rowCount: t.rowCount,
        colCount: t.colCount,
        depth: t.depth,
        parentTableIndex: t.parentTableIndex,
        parentCell: t.parentCell,
        cells: t.cells.map((c, cIdx) => ({
          idx: c.idx ?? cIdx,
          row: c.row,
          col: c.col,
          rowSpan: c.rowSpan,
          colSpan: c.colSpan,
          text: c.text,
          bbox: undefined,
        })),
      });
    }
  }

  return result;
}

/**
 * Maps inspect_document output tables into TableCoordinateMeta,
 * preserving tableIndex, depth, parentTableIndex, parentCell, and cell bboxes.
 */
export function mapInspectedTables(
  inspectedTables: any[],
  existingMeta: TableCoordinateMeta[] = []
): TableCoordinateMeta[] {
  return inspectedTables.map((t: any, idx: number) => {
    const prevTable =
      existingMeta.find((et) => et.tableIndex === (t.tableIndex ?? idx)) ??
      existingMeta[idx];
    return {
      tableIndex: t.tableIndex ?? idx,
      sec: t.sec ?? prevTable?.sec ?? 0,
      para: t.para ?? prevTable?.para ?? 0,
      ctrl: t.ctrl ?? prevTable?.ctrl ?? 0,
      rowCount: t.rowCount ?? prevTable?.rowCount ?? 0,
      colCount: t.colCount ?? prevTable?.colCount ?? 0,
      depth: t.depth ?? prevTable?.depth ?? 0,
      parentTableIndex: t.parentTableIndex ?? prevTable?.parentTableIndex,
      parentCell: t.parentCell ?? prevTable?.parentCell,
      cells: Array.isArray(t.cells)
        ? t.cells.map((c: any, cIdx: number) => {
            const prevCell = prevTable?.cells?.find((pc) => pc.row === c.row && pc.col === c.col);
            return {
              idx: c.idx ?? prevCell?.idx ?? cIdx,
              row: c.row,
              col: c.col,
              rowSpan: c.rowSpan ?? 1,
              colSpan: c.colSpan ?? 1,
              text: c.text ?? prevCell?.text ?? "",
              bbox: c.bbox ?? prevCell?.bbox,
            };
          })
        : prevTable?.cells || [],
    };
  });
}

/**
 * Synchronizes HWPX table indices with rhwpEngine tables by parsing HWPX section XMLs.
 */
export function syncHwpxTableIndex(
  fileBytes: Uint8Array,
  rhwpTopTables: TableCoordinateMeta[]
): TableCoordinateMeta[] {
  return buildTablesMetaForHwpx(fileBytes, rhwpTopTables);
}
