#!/usr/bin/env node
/**
 * Generates the shared HOP-Claw golden corpus under tests/fixtures/golden-corpus/.
 *
 * Captures Node sidecar outputs + wall-clock timings for:
 *   1. resources/claw-hwp/scripts/extract_text.js --inspect --with-cell-text
 *   2. resources/claw-hwp/scripts/extract_text.js --format markdown
 *   3. (HWPX only) resources/claw-hwp/scripts/hwpx-edit.js { inspect: "cells" }
 *   4. (HWPX only) pre-order TableWalker structure matching src/core/hwpxTableIndex.ts
 */

import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { unzipSync, strFromU8 } from '../../../resources/claw-hwp/scripts/vendor/fflate/index.mjs';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(__dirname, '../../..');
const EXTRACT_SCRIPT = path.join(REPO_ROOT, 'resources/claw-hwp/scripts/extract_text.js');
const HWPX_EDIT_SCRIPT = path.join(REPO_ROOT, 'resources/claw-hwp/scripts/hwpx-edit.js');
const OUTPUTS_DIR = path.join(__dirname, 'outputs');
const FIXTURES_DIR = path.join(__dirname, 'fixtures');

fs.mkdirSync(OUTPUTS_DIR, { recursive: true });
fs.mkdirSync(FIXTURES_DIR, { recursive: true });

// ── 1. Build nested_depth2.hwpx fixture ────────────────────────────────────
function buildNestedDepth2Fixture() {
  const srcSample = path.join(REPO_ROOT, 'samples/(날짜)(교시)(과목명)교수.학습과정안(이름).hwpx');
  const destFixture = path.join(FIXTURES_DIR, 'nested_depth2.hwpx');
  fs.copyFileSync(srcSample, destFixture);

  const step1 = spawnSync('node', [HWPX_EDIT_SCRIPT], {
    input: JSON.stringify({
      path: destFixture,
      output: destFixture,
      operations: [
        {
          type: 'insert_nested_table',
          table: 0,
          row: 8,
          col: 1,
          rows: 2,
          cols: 2,
          data: [
            ['세부과정A', '시간'],
            ['단계1', '10분'],
          ],
          headerRow: true,
          widthMode: 'fit_cell',
        },
      ],
    }),
    encoding: 'utf8',
    cwd: REPO_ROOT,
  });
  if (step1.status !== 0) {
    throw new Error(`Failed step1 nested_depth2: ${step1.stderr}`);
  }

  const step2 = spawnSync('node', [HWPX_EDIT_SCRIPT], {
    input: JSON.stringify({
      path: destFixture,
      output: destFixture,
      operations: [
        {
          type: 'insert_nested_table',
          table: 1,
          row: 1,
          col: 0,
          rows: 1,
          cols: 2,
          data: [['L2_Key', 'L2_Val']],
          widthMode: 'fit_cell',
        },
      ],
    }),
    encoding: 'utf8',
    cwd: REPO_ROOT,
  });
  if (step2.status !== 0) {
    throw new Error(`Failed step2 nested_depth2: ${step2.stderr}`);
  }
}

// ── 2. Exact mirror of src/core/hwpxTableIndex.ts parseHwpxTablesFromBytes ─
function scanTopLevel(xml, tag) {
  const t = tag.replace(/:/g, '\\:');
  const re = new RegExp(`<${t}((?:\\s[^>]*?)?)(/?)>|</${t}\\s*>`, 'g');
  const out = [];
  let depth = 0;
  let start = -1;
  let openEnd = -1;
  let attrs = '';
  let m;
  while ((m = re.exec(xml)) !== null) {
    const isClose = m[0].startsWith('</');
    if (!isClose && m[2] === '/') {
      if (depth === 0) {
        out.push({ start: m.index, end: re.lastIndex, openEnd: re.lastIndex, attrs: m[1] || '', inner: '', selfClosing: true });
      }
      continue;
    }
    if (!isClose) {
      if (depth === 0) {
        start = m.index;
        openEnd = re.lastIndex;
        attrs = m[1] || '';
      }
      depth++;
    } else if (depth > 0) {
      depth--;
      if (depth === 0) {
        out.push({ start, end: re.lastIndex, openEnd, attrs, inner: xml.slice(openEnd, m.index) });
      }
    }
  }
  return out;
}

function xmlUnescape(s) {
  return String(s).replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&amp;/g, '&');
}

function parseHwpxSectionTables(xml, sectionIndex, startIndexOffset = 0) {
  const result = [];
  function walk(xmlFragment, depth, parentTableIndex, parentCell) {
    const tblList = scanTopLevel(xmlFragment, 'hp:tbl');
    for (const tbl of tblList) {
      const currentTableIndex = startIndexOffset + result.length;
      const trList = scanTopLevel(tbl.inner, 'hp:tr');
      const cells = [];
      let cellIdx = 0;
      let maxRow = 0;
      let maxCol = 0;
      for (let ri = 0; ri < trList.length; ri++) {
        const rw = trList[ri];
        const tcList = scanTopLevel(rw.inner, 'hp:tc');
        for (let ci = 0; ci < tcList.length; ci++) {
          const tc = tcList[ci];
          const innerStart = rw.openEnd + tc.start;
          const innerEnd = rw.openEnd + tc.end;
          const subEnd = tc.inner.lastIndexOf('</hp:subList>');
          const directInner = subEnd >= 0 ? tc.inner.slice(subEnd) : tc.inner;
          const am = directInner.match(
            /<hp:cellAddr [^>]*colAddr="(\d+)"[^>]*rowAddr="(\d+)"|<hp:cellAddr [^>]*rowAddr="(\d+)"[^>]*colAddr="(\d+)"/
          );
          const col = am ? (am[1] !== undefined ? Number(am[1]) : Number(am[4])) : ci;
          const row = am ? (am[2] !== undefined ? Number(am[2]) : Number(am[3])) : ri;
          const sm = directInner.match(/<hp:cellSpan [^>]*colSpan="(\d+)"[^>]*rowSpan="(\d+)"/);
          const colSpan = sm ? Number(sm[1]) : 1;
          const rowSpan = sm ? Number(sm[2]) : 1;
          const text = xmlUnescape(
            (tc.inner.match(/<hp:t>([^<]*)<\/hp:t>/g) || [])
              .map((x) => x.replace(/<\/?hp:t>/g, ''))
              .join('')
          );
          maxRow = Math.max(maxRow, row + rowSpan);
          maxCol = Math.max(maxCol, col + colSpan);
          cells.push({ idx: cellIdx++, row, col, rowSpan, colSpan, text, innerStart, innerEnd });
        }
      }
      if (maxRow === 0) maxRow = trList.length;
      result.push({
        sectionIndex,
        tableIndex: currentTableIndex,
        depth,
        parentTableIndex: parentTableIndex ?? null,
        parentCell: parentCell ?? null,
        rowCount: maxRow,
        colCount: maxCol,
        cells: cells.map(({ innerStart, innerEnd, ...rest }) => rest),
        _cellSpans: cells.map((c) => ({ innerStart: c.innerStart, innerEnd: c.innerEnd, row: c.row, col: c.col })),
      });
      const childTbls = scanTopLevel(tbl.inner, 'hp:tbl');
      for (const childTbl of childTbls) {
        const parentC = cells.find((c) => childTbl.start >= c.innerStart && childTbl.end <= c.innerEnd);
        const childParentCell = parentC ? { row: parentC.row, col: parentC.col } : undefined;
        walk(tbl.inner.slice(childTbl.start, childTbl.end), depth + 1, currentTableIndex, childParentCell);
      }
    }
  }
  walk(xml, 0);
  return result.map(({ _cellSpans, ...rest }) => rest);
}

function parseHwpxTablesFromBytes(fileBytes) {
  const unzipped = unzipSync(fileBytes);
  const sectionFiles = Object.keys(unzipped)
    .filter((n) => /^Contents\/section\d+\.xml$/i.test(n))
    .sort((a, b) => {
      const ma = a.match(/section(\d+)\.xml/i);
      const mb = b.match(/section(\d+)\.xml/i);
      return (ma ? parseInt(ma[1], 10) : 0) - (mb ? parseInt(mb[1], 10) : 0);
    });
  const allTables = [];
  for (let secIdx = 0; secIdx < sectionFiles.length; secIdx++) {
    const xml = strFromU8(unzipped[sectionFiles[secIdx]]);
    allTables.push(...parseHwpxSectionTables(xml, secIdx, allTables.length));
  }
  return allTables;
}

// ── 3. Run corpus capture ──────────────────────────────────────────────────
buildNestedDepth2Fixture();

const SOURCES = [
  { id: 'sample_lesson_plan_hwp', relPath: 'samples/(날짜)(교시)(과목명)교수.학습과정안(이름).hwp' },
  { id: 'sample_lesson_plan_hwpx', relPath: 'samples/(날짜)(교시)(과목명)교수.학습과정안(이름).hwpx' },
  { id: 'sample_ai_plan_hwpx', relPath: 'samples/2026_AI_행정업무_혁신계획(기획안).hwpx' },
  { id: 'sample_official_report_hwp', relPath: 'samples/demo_official_report.hwp' },
  { id: 'sample_official_report_hwpx', relPath: 'samples/demo_official_report.hwpx' },
  { id: 'template_hancom_native_stub_hwpx', relPath: 'resources/claw-hwp/scripts/templates/hancom_native_stub.hwpx' },
  { id: 'fixture_nested_depth2_hwpx', relPath: 'tests/fixtures/golden-corpus/fixtures/nested_depth2.hwpx' },
  { id: 'rhwp_form_002_hwpx', relPath: 'third_party/rhwp/rhwp-studio/public/samples/form-002.hwpx' },
  { id: 'rhwp_ktx_hwp', relPath: 'third_party/rhwp/rhwp-studio/public/samples/basic/KTX.hwp' },
  { id: 'rhwp_biz_plan_hwp', relPath: 'third_party/rhwp/rhwp-studio/public/samples/biz_plan.hwp' },
  { id: 'rhwp_book_review_hwp', relPath: 'third_party/rhwp/rhwp-studio/public/samples/BlogForm_BookReview.hwp' },
  { id: 'rhwp_field_01_hwp', relPath: 'third_party/rhwp/rhwp-studio/public/samples/field-01.hwp' },
  { id: 'rhwp_footnote_01_hwp', relPath: 'third_party/rhwp/rhwp-studio/public/samples/footnote-01.hwp' },
  { id: 'rhwp_issue1949_nested_hwp', relPath: 'third_party/rhwp/rhwp-studio/public/samples/issue1949_giant_cell_nested_tables_perf.hwp' },
  { id: 'rhwp_kps_ai_hwp', relPath: 'third_party/rhwp/rhwp-studio/public/samples/kps-ai.hwp' },
  { id: 'rhwp_number_bullet_hwp', relPath: 'third_party/rhwp/rhwp-studio/public/samples/number-bullet.hwp' },
  { id: 'rhwp_oullim_01_hwp', relPath: 'third_party/rhwp/rhwp-studio/public/samples/oullim-01.hwp' },
  { id: 'rhwp_para_head_num_2_hwp', relPath: 'third_party/rhwp/rhwp-studio/public/samples/para-head-num-2.hwp' },
  { id: 'rhwp_shift_return_hwp', relPath: 'third_party/rhwp/rhwp-studio/public/samples/shift-return.hwp' },
  { id: 'rhwp_tac_case_001_hwp', relPath: 'third_party/rhwp/rhwp-studio/public/samples/tac-case-001.hwp' },
];

const entries = [];
for (const src of SOURCES) {
  const absPath = path.join(REPO_ROOT, src.relPath);
  const bytes = fs.readFileSync(absPath);
  const isHwpx = bytes[0] === 0x50 && bytes[1] === 0x4b;

  const t0 = performance.now();
  const inspectRes = spawnSync('node', [EXTRACT_SCRIPT, '--inspect', '--with-cell-text', absPath], {
    encoding: 'utf8',
    cwd: REPO_ROOT,
    maxBuffer: 32 * 1024 * 1024,
  });
  const t1 = performance.now();
  if (inspectRes.status !== 0) {
    throw new Error(`inspect failed on ${src.relPath}: ${inspectRes.stderr}`);
  }

  const t2 = performance.now();
  const mdRes = spawnSync('node', [EXTRACT_SCRIPT, '--format', 'markdown', absPath], {
    encoding: 'utf8',
    cwd: REPO_ROOT,
    maxBuffer: 32 * 1024 * 1024,
  });
  const t3 = performance.now();
  if (mdRes.status !== 0) {
    throw new Error(`markdown failed on ${src.relPath}: ${mdRes.stderr}`);
  }

  const inspectRel = `outputs/${src.id}.inspect.json`;
  const mdRel = `outputs/${src.id}.markdown.md`;
  fs.writeFileSync(path.join(__dirname, inspectRel), inspectRes.stdout, 'utf8');
  fs.writeFileSync(path.join(__dirname, mdRel), mdRes.stdout, 'utf8');

  let hwpxTablesRel = null;
  let hwpxCellsRel = null;
  if (isHwpx) {
    const tables = parseHwpxTablesFromBytes(new Uint8Array(bytes));
    hwpxTablesRel = `outputs/${src.id}.hwpx-tables.json`;
    fs.writeFileSync(path.join(__dirname, hwpxTablesRel), JSON.stringify(tables, null, 2) + '\n', 'utf8');

    const cellsRes = spawnSync('node', [HWPX_EDIT_SCRIPT], {
      input: JSON.stringify({ path: absPath, inspect: 'cells' }),
      encoding: 'utf8',
      cwd: REPO_ROOT,
      maxBuffer: 32 * 1024 * 1024,
    });
    if (cellsRes.status !== 0) {
      throw new Error(`hwpx-edit inspect cells failed on ${src.relPath}: ${cellsRes.stderr}`);
    }
    hwpxCellsRel = `outputs/${src.id}.hwpx-cells.json`;
    fs.writeFileSync(path.join(__dirname, hwpxCellsRel), cellsRes.stdout, 'utf8');
  }

  const inspectJson = JSON.parse(inspectRes.stdout);
  entries.push({
    id: src.id,
    source_path: src.relPath,
    file_type: isHwpx ? 'hwpx' : 'hwp',
    bytes: bytes.length,
    section_count: inspectJson.sectionCount,
    paragraph_count: inspectJson.paragraphCount,
    table_count: inspectJson.tableCount,
    cell_count: inspectJson.cellCount,
    outputs: {
      inspect_json: inspectRel,
      markdown_md: mdRel,
      hwpx_tables_json: hwpxTablesRel,
      hwpx_cells_json: hwpxCellsRel,
    },
    timings_ms: {
      node_inspect_ms: Math.round((t1 - t0) * 100) / 100,
      node_markdown_ms: Math.round((t3 - t2) * 100) / 100,
    },
  });
}

const manifest = {
  generated_at: new Date().toISOString(),
  node_version: process.version,
  platform: `${process.platform}-${process.arch}`,
  corpus_count: entries.length,
  entries,
};

fs.writeFileSync(path.join(__dirname, 'manifest.json'), JSON.stringify(manifest, null, 2) + '\n', 'utf8');
console.log(`Generated golden corpus: ${entries.length} documents.`);
